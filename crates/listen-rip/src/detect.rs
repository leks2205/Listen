//! Audio CD insert/remove detection: UDisks2 media-change signals on the system bus, with the
//! object list re-read and compared; without UDisks2, `/dev/sr*` is polled every two seconds.

use crate::drive::{self, DiscState, DriveInfo};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveEvent {
    /// The optical drives (for "Rip CD"; empty = no drive).
    Drives(Vec<DriveInfo>),
    /// An audio CD was already in the drive when watching started (or the drive was plugged in with one).
    Present { device: PathBuf },
    /// An audio CD was inserted.
    Inserted { device: PathBuf },
    /// The audio CD left the drive (ejected or pulled).
    Removed { device: PathBuf },
}

/// One look at the drives: each drive and whether an audio CD is in it.
pub type Snapshot = Vec<(DriveInfo, bool)>;

/// Turns successive snapshots into events.
#[derive(Debug, Default)]
pub struct Tracker {
    audio: BTreeMap<PathBuf, bool>,
    drives: Option<Vec<DriveInfo>>,
}

impl Tracker {
    pub fn update(&mut self, snapshot: &Snapshot) -> Vec<DriveEvent> {
        let mut events = Vec::new();
        let drives: Vec<DriveInfo> = snapshot.iter().map(|(d, _)| d.clone()).collect();
        if self.drives.as_ref() != Some(&drives) {
            events.push(DriveEvent::Drives(drives.clone()));
        }
        for (d, audio) in snapshot {
            let known = self.audio.get(&d.device).copied();
            match (known, *audio) {
                // At start, or a drive plugged in with a disc already in it.
                (None, true) => events.push(DriveEvent::Present { device: d.device.clone() }),
                (Some(false), true) => events.push(DriveEvent::Inserted { device: d.device.clone() }),
                (Some(true), false) => events.push(DriveEvent::Removed { device: d.device.clone() }),
                _ => {}
            }
        }
        for (dev, was) in &self.audio {
            if *was && !snapshot.iter().any(|(d, _)| &d.device == dev) {
                events.push(DriveEvent::Removed { device: dev.clone() });
            }
        }
        self.audio = snapshot.iter().map(|(d, a)| (d.device.clone(), *a)).collect();
        self.drives = Some(drives);
        events
    }
}

/// Polls `/dev/sr*` (the fallback without UDisks2).
pub fn poll_snapshot() -> Snapshot {
    drive::list()
        .into_iter()
        .map(|d| {
            let audio = drive::disc_state(&d.device) == DiscState::Audio;
            (d, audio)
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub use udisks::{eject_on, eject_udisks, snapshot_from, watch, watch_on};

#[cfg(target_os = "linux")]
mod udisks {
    use super::*;
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;
    use zbus::fdo::{ManagedObjects, ObjectManagerProxy};
    use zbus::zvariant::{OwnedValue, Value};

    const SERVICE: &str = "org.freedesktop.UDisks2";
    const ROOT: &str = "/org/freedesktop/UDisks2";
    const BLOCK: &str = "org.freedesktop.UDisks2.Block";
    const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

    fn unwrap<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
        match v {
            Value::Value(inner) => unwrap(inner),
            v => v,
        }
    }
    fn get<'a>(props: &'a HashMap<String, OwnedValue>, key: &str) -> Option<&'a Value<'static>> {
        props.get(key).map(|v| &**v)
    }
    fn as_bool(v: Option<&Value<'_>>) -> bool {
        matches!(v.map(unwrap), Some(Value::Bool(true)))
    }
    fn as_u32(v: Option<&Value<'_>>) -> u32 {
        match v.map(unwrap) {
            Some(Value::U32(n)) => *n,
            _ => 0,
        }
    }
    fn as_string(v: Option<&Value<'_>>) -> String {
        match v.map(unwrap) {
            Some(Value::Str(s)) => s.as_str().trim().to_string(),
            _ => String::new(),
        }
    }
    fn as_path(v: Option<&Value<'_>>) -> Option<String> {
        match v.map(unwrap) {
            Some(Value::ObjectPath(p)) => Some(p.as_str().to_string()),
            _ => None,
        }
    }
    /// `ay` with a trailing NUL (UDisks2's `Device`).
    fn as_bytes_path(v: Option<&Value<'_>>) -> Option<PathBuf> {
        let Some(Value::Array(a)) = v.map(unwrap) else { return None };
        let bytes: Vec<u8> = a
            .inner()
            .iter()
            .filter_map(|b| if let Value::U8(b) = b { Some(*b) } else { None })
            .take_while(|b| *b != 0)
            .collect();
        (!bytes.is_empty()).then(|| PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
    }

    /// Optical drives in UDisks2's object list and whether each holds an audio CD
    /// (`MediaAvailable` and at least one audio track).
    pub fn snapshot_from(objects: &ManagedObjects) -> Snapshot {
        let mut out = Vec::new();
        for ifaces in objects.values() {
            let Some(block) = ifaces.iter().find(|(n, _)| n.as_str() == BLOCK).map(|(_, p)| p) else {
                continue;
            };
            let Some(device) = as_bytes_path(get(block, "Device")) else { continue };
            if !device.to_string_lossy().starts_with("/dev/sr") {
                continue;
            }
            let drive = as_path(get(block, "Drive")).and_then(|dp| {
                objects
                    .iter()
                    .find(|(p, _)| p.as_str() == dp)
                    .and_then(|(_, i)| i.iter().find(|(n, _)| n.as_str() == DRIVE).map(|(_, p)| p))
            });
            let (info, audio) = match drive {
                Some(d) => (
                    DriveInfo {
                        device,
                        vendor: as_string(get(d, "Vendor")),
                        model: as_string(get(d, "Model")),
                    },
                    as_bool(get(d, "MediaAvailable")) && as_u32(get(d, "OpticalNumAudioTracks")) > 0,
                ),
                None => (DriveInfo { device, vendor: String::new(), model: String::new() }, false),
            };
            out.push((info, audio));
        }
        out.sort_by(|a, b| a.0.device.cmp(&b.0.device));
        out
    }

    async fn manager(conn: &zbus::Connection) -> zbus::Result<ObjectManagerProxy<'static>> {
        ObjectManagerProxy::builder(conn).destination(SERVICE)?.path(ROOT)?.build().await
    }

    /// Watches drives on `conn` until the receiver is dropped. Errors when UDisks2 is missing or goes away.
    pub async fn watch_on(
        conn: &zbus::Connection,
        tx: &tokio::sync::mpsc::Sender<DriveEvent>,
    ) -> zbus::Result<()> {
        let om = manager(conn).await?;
        let rule =
            zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).path_namespace(ROOT)?.build();
        let mut stream = zbus::MessageStream::for_match_rule(rule, conn, Some(256)).await?;
        let mut tracker = Tracker::default();
        let objects = om.get_managed_objects().await?;
        for ev in tracker.update(&snapshot_from(&objects)) {
            if tx.send(ev).await.is_err() {
                return Ok(());
            }
        }
        loop {
            // A signal, or a resync every 5 s in case one was missed.
            if let Ok(None) = tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                return Err(zbus::Error::Failure("UDisks2 signal stream ended".into()));
            }
            // Media changes come in bursts (MediaAvailable, then the track counts).
            tokio::time::sleep(Duration::from_millis(300)).await;
            while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(1), stream.next()).await {}
            if tx.is_closed() {
                return Ok(());
            }
            let objects = om.get_managed_objects().await?;
            for ev in tracker.update(&snapshot_from(&objects)) {
                tracing::info!("CD drive: {ev:?}");
                if tx.send(ev).await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    /// Watches the drives until the receiver is dropped: UDisks2 on the system bus, else polling.
    pub async fn watch(tx: tokio::sync::mpsc::Sender<DriveEvent>) {
        match zbus::Connection::system().await {
            Ok(conn) => match watch_on(&conn, &tx).await {
                Ok(()) => return,
                Err(e) => tracing::warn!("UDisks2 not usable ({e}); polling the CD drives instead"),
            },
            Err(e) => tracing::warn!("no system bus ({e}); polling the CD drives instead"),
        }
        let mut tracker = Tracker::default();
        loop {
            let snap = tokio::task::spawn_blocking(poll_snapshot).await.unwrap_or_default();
            for ev in tracker.update(&snap) {
                tracing::info!("CD drive (polled): {ev:?}");
                if tx.send(ev).await.is_err() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// `Drive.Eject` for the drive behind `device` (system bus).
    pub async fn eject_udisks(device: &Path) -> Result<(), String> {
        let conn = zbus::Connection::system().await.map_err(|e| e.to_string())?;
        eject_on(&conn, device).await
    }

    pub async fn eject_on(conn: &zbus::Connection, device: &Path) -> Result<(), String> {
        let om = manager(conn).await.map_err(|e| e.to_string())?;
        let objects = om.get_managed_objects().await.map_err(|e| e.to_string())?;
        let drive = objects
            .values()
            .filter_map(|i| i.iter().find(|(n, _)| n.as_str() == BLOCK).map(|(_, p)| p))
            .find(|b| as_bytes_path(get(b, "Device")).as_deref() == Some(device))
            .and_then(|b| as_path(get(b, "Drive")))
            .ok_or_else(|| format!("{} not known to UDisks2", device.display()))?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        conn.call_method(Some(SERVICE), drive.as_str(), Some(DRIVE), "Eject", &(options,))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Non-Linux: nothing to watch.
#[cfg(not(target_os = "linux"))]
pub async fn watch(_tx: tokio::sync::mpsc::Sender<DriveEvent>) {
    std::future::pending::<()>().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(dev: &str) -> DriveInfo {
        DriveInfo { device: dev.into(), vendor: "HL-DT-ST".into(), model: "DVDRAM GU70N".into() }
    }

    #[test]
    fn tracker_reports_changes_only() {
        let mut t = Tracker::default();
        // Start: drive empty.
        let ev = t.update(&vec![(drive("/dev/sr0"), false)]);
        assert_eq!(ev, vec![DriveEvent::Drives(vec![drive("/dev/sr0")])]);
        assert!(t.update(&vec![(drive("/dev/sr0"), false)]).is_empty());
        // Insert, stay, remove.
        assert_eq!(
            t.update(&vec![(drive("/dev/sr0"), true)]),
            vec![DriveEvent::Inserted { device: "/dev/sr0".into() }]
        );
        assert!(t.update(&vec![(drive("/dev/sr0"), true)]).is_empty());
        assert_eq!(
            t.update(&vec![(drive("/dev/sr0"), false)]),
            vec![DriveEvent::Removed { device: "/dev/sr0".into() }]
        );
        // USB drive unplugged with a disc in it.
        t.update(&vec![(drive("/dev/sr0"), true)]);
        let ev = t.update(&vec![]);
        assert!(ev.contains(&DriveEvent::Removed { device: "/dev/sr0".into() }));
        assert!(ev.contains(&DriveEvent::Drives(vec![])));
    }

    #[test]
    fn disc_already_in_at_start_is_present_not_inserted() {
        let mut t = Tracker::default();
        let ev = t.update(&vec![(drive("/dev/sr0"), true)]);
        assert!(ev.contains(&DriveEvent::Present { device: "/dev/sr0".into() }));
    }
}
