//! Heart-rate sources for the heart-rate tempo mode ([`crate::app::heart_rate`]): a Bluetooth LE heart-rate
//! sensor (the standard Heart Rate Service 0x180D, measurement characteristic 0x2A37, e.g. a chest strap)
//! through `btleplug` (BlueZ over D-Bus on Linux, WinRT on Windows), or a scripted fake source (demo mode,
//! tests, `LISTEN_HR_FAKE`). Each runs in its own task as an iced subscription and sends [`HrEvent`]s.
//!
//! The Bluetooth source scans for a device that offers the Heart Rate Service (the remembered one first),
//! connects, subscribes to the measurement notifications and reconnects when the link drops. Without
//! Bluetooth (no adapter, switched off, no BlueZ) it reports "not found" and tries again later; nothing else
//! in the app depends on it. Sensors are never paired: the link is a plain GATT connection.

use crate::app::Message;
use crate::app::heart_rate::HeartRateMsg;
use iced::Subscription;
use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, Stream};
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// Heart Rate Service.
pub const HR_SERVICE: u16 = 0x180D;
/// Heart Rate Measurement characteristic (notify).
pub const HR_MEASUREMENT: u16 = 0x2A37;
/// How long a scan looks for a sensor before it reports "not found".
const SCAN_FOR: Duration = Duration::from_secs(20);
/// How long connecting to a found sensor may take before it counts as not reachable.
const CONNECT_FOR: Duration = Duration::from_secs(20);
/// With a remembered sensor, other sensors are taken only after this long without it.
const PREFER_FOR: Duration = Duration::from_secs(6);
/// A connected sensor that sends nothing for this long counts as lost.
const SILENT_FOR: Duration = Duration::from_secs(15);
/// Wait before searching again after "not found" or a lost link.
const RETRY_AFTER: Duration = Duration::from_secs(10);

/// What a heart-rate source reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HrEvent {
    /// Looking for a sensor.
    Searching,
    /// Connected to sensor `id` (`name` as advertised).
    Connected { id: String, name: String },
    /// One reading, beats per minute.
    Sample(u16),
    /// The link dropped (it reconnects).
    Lost,
    /// No sensor (or no Bluetooth); the reason for the log.
    NotFound(String),
}

/// The heart rate in a Heart Rate Measurement value: flags byte (bit 0: the value is 16-bit), then the
/// value (little-endian). `None` for a short or zero reading (sensor without skin contact).
pub fn parse_measurement(data: &[u8]) -> Option<u16> {
    let flags = *data.first()?;
    let bpm = if flags & 1 == 0 {
        u16::from(*data.get(1)?)
    } else {
        u16::from_le_bytes([*data.get(1)?, *data.get(2)?])
    };
    (bpm > 0).then_some(bpm)
}

/// A scripted fake source: `bpm:seconds` steps (`"70:180,150:300,90:300"`), repeated.
pub fn parse_script(s: &str) -> Vec<(u16, u32)> {
    s.split(',')
        .filter_map(|step| {
            let (bpm, secs) = step.trim().split_once(':')?;
            Some((bpm.trim().parse().ok()?, secs.trim().parse().ok()?))
        })
        .filter(|(bpm, secs)| *bpm > 0 && *secs > 0)
        .collect()
}

/// The demo's fake curve (the acceptance curve of the phone's lab: rest, run, cool down).
pub const DEMO_SCRIPT: &str = "70:180,150:300,90:300";

/// Which source runs. Subscriptions are keyed by `generation` and the kind only, so remembering the
/// sensor after a connect doesn't restart the running source.
#[derive(Debug, Clone)]
pub struct SourceSpec {
    pub generation: u64,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// A Bluetooth LE sensor; the remembered device id is tried first.
    Bluetooth { preferred: Option<String> },
    /// Scripted readings, one per second.
    Fake(Vec<(u16, u32)>),
}

impl PartialEq for SourceSpec {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation
            && matches!(
                (&self.kind, &other.kind),
                (SourceKind::Bluetooth { .. }, SourceKind::Bluetooth { .. })
                    | (SourceKind::Fake(_), SourceKind::Fake(_))
            )
    }
}

impl Hash for SourceSpec {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
        matches!(self.kind, SourceKind::Fake(_)).hash(state);
    }
}

pub fn subscription(spec: SourceSpec) -> Subscription<Message> {
    Subscription::run_with(spec, worker)
}

fn send(ev: HrEvent) -> Message {
    Message::HeartRate(HeartRateMsg::Source(ev))
}

fn worker(spec: &SourceSpec) -> impl Stream<Item = Message> + use<> {
    let kind = spec.kind.clone();
    iced::stream::channel(16, async move |mut out: Sender<Message>| {
        match kind {
            SourceKind::Fake(script) => fake(script, &mut out).await,
            SourceKind::Bluetooth { preferred } => bluetooth(preferred, &mut out).await,
        }
        std::future::pending::<()>().await;
    })
}

/// Plays `script` (one reading per second, in a loop).
async fn fake(script: Vec<(u16, u32)>, out: &mut Sender<Message>) {
    let ev = HrEvent::Connected { id: "fake".into(), name: "Test heart-rate source".into() };
    if script.is_empty() || out.send(send(ev)).await.is_err() {
        return;
    }
    loop {
        for &(bpm, secs) in &script {
            for _ in 0..secs {
                if out.send(send(HrEvent::Sample(bpm))).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(any(target_os = "linux", windows))]
async fn bluetooth(preferred: Option<String>, out: &mut Sender<Message>) {
    loop {
        if out.send(send(HrEvent::Searching)).await.is_err() {
            return;
        }
        let ev = match ble::session(preferred.as_deref(), out).await {
            Ok(()) => HrEvent::Lost,
            Err(e) => HrEvent::NotFound(e),
        };
        if let HrEvent::NotFound(e) = &ev {
            tracing::info!("heart rate: no sensor ({e})");
        } else {
            tracing::info!("heart rate: the sensor's link dropped; reconnecting");
        }
        if out.send(send(ev)).await.is_err() {
            return;
        }
        tokio::time::sleep(RETRY_AFTER).await;
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
async fn bluetooth(_preferred: Option<String>, out: &mut Sender<Message>) {
    let _ = out.send(send(HrEvent::NotFound("Bluetooth is not supported on this platform".into()))).await;
}

#[cfg(any(target_os = "linux", windows))]
mod ble {
    use super::*;
    use btleplug::api::bleuuid::uuid_from_u16;
    use btleplug::api::{Central, CentralState, Manager as _, Peripheral as _, ScanFilter};
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use iced::futures::StreamExt;
    use std::time::Instant;

    fn err(e: impl std::fmt::Display) -> String {
        e.to_string()
    }

    /// One connection: find a sensor, connect, forward its readings until the link drops.
    pub(super) async fn session(preferred: Option<&str>, out: &mut Sender<Message>) -> Result<(), String> {
        let manager = Manager::new().await.map_err(err)?;
        let adapter =
            manager.adapters().await.map_err(err)?.into_iter().next().ok_or("no Bluetooth adapter")?;
        if matches!(adapter.adapter_state().await, Ok(CentralState::PoweredOff)) {
            return Err("Bluetooth is off".into());
        }
        let sensor = find(&adapter, preferred).await?;
        let id = sensor.id().to_string();
        let name = sensor
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|p| p.local_name.or(p.advertisement_name))
            .unwrap_or_else(|| id.clone());
        if !sensor.is_connected().await.unwrap_or(false) {
            tokio::time::timeout(CONNECT_FOR, sensor.connect())
                .await
                .map_err(|_| "connecting timed out".to_string())?
                .map_err(err)?;
        }
        let result = forward(&sensor, &id, &name, out).await;
        let _ = sensor.disconnect().await;
        result
    }

    async fn forward(
        sensor: &Peripheral,
        id: &str,
        name: &str,
        out: &mut Sender<Message>,
    ) -> Result<(), String> {
        sensor.discover_services().await.map_err(err)?;
        let measurement = uuid_from_u16(HR_MEASUREMENT);
        let ch = sensor
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == measurement)
            .ok_or("the device has no heart-rate measurement")?;
        sensor.subscribe(&ch).await.map_err(err)?;
        let mut notes = sensor.notifications().await.map_err(err)?;
        tracing::info!("heart rate: connected to “{name}”");
        let ev = HrEvent::Connected { id: id.to_string(), name: name.to_string() };
        out.send(send(ev)).await.map_err(err)?;
        while let Ok(Some(n)) = tokio::time::timeout(SILENT_FOR, notes.next()).await {
            if n.uuid != measurement {
                continue;
            }
            if let Some(bpm) = parse_measurement(&n.value) {
                out.send(send(HrEvent::Sample(bpm))).await.map_err(err)?;
            }
        }
        Ok(())
    }

    /// Scans for a device offering the Heart Rate Service: the `preferred` one when it shows up within
    /// [`PREFER_FOR`], else the first one seen.
    async fn find(adapter: &Adapter, preferred: Option<&str>) -> Result<Peripheral, String> {
        let service = uuid_from_u16(HR_SERVICE);
        adapter.start_scan(ScanFilter { services: vec![service] }).await.map_err(err)?;
        let started = Instant::now();
        let found = loop {
            let mut first: Option<Peripheral> = None;
            for p in adapter.peripherals().await.map_err(err)? {
                let offers = p
                    .properties()
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|props| props.services.contains(&service));
                if !offers {
                    continue;
                }
                if preferred == Some(p.id().to_string().as_str()) {
                    first = Some(p);
                    break;
                }
                first.get_or_insert(p);
            }
            let waited = started.elapsed();
            match first {
                Some(p) if preferred.is_none_or(|id| p.id().to_string() == id) || waited > PREFER_FOR => {
                    break Ok(p);
                }
                _ if waited > SCAN_FOR => break Err("no heart-rate sensor in range".to_string()),
                _ => tokio::time::sleep(Duration::from_secs(1)).await,
            }
        };
        let _ = adapter.stop_scan().await;
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurements_are_8_or_16_bit() {
        assert_eq!(parse_measurement(&[0x00, 72]), Some(72));
        // 16-bit value (flags bit 0), with RR intervals after it (flags bit 4) ignored.
        assert_eq!(parse_measurement(&[0x11, 0x2C, 0x01, 0x00, 0x04]), Some(300));
        assert_eq!(parse_measurement(&[0x06, 150, 0x10, 0x00]), Some(150), "contact bits don't matter");
        assert_eq!(parse_measurement(&[0x01, 0x2C]), None, "16-bit flag but one byte");
        assert_eq!(parse_measurement(&[0x00, 0]), None, "no skin contact");
        assert_eq!(parse_measurement(&[]), None);
    }

    #[test]
    fn scripts_parse_and_skip_bad_steps() {
        assert_eq!(parse_script(DEMO_SCRIPT), vec![(70, 180), (150, 300), (90, 300)]);
        assert_eq!(parse_script(" 120 : 5 , x:1, 0:4, 90:0, 100:2"), vec![(120, 5), (100, 2)]);
    }

    #[test]
    fn a_remembered_sensor_does_not_restart_the_source() {
        use std::hash::DefaultHasher;
        let h = |s: &SourceSpec| {
            let mut h = DefaultHasher::new();
            s.hash(&mut h);
            h.finish()
        };
        let a = SourceSpec { generation: 1, kind: SourceKind::Bluetooth { preferred: None } };
        let b = SourceSpec { generation: 1, kind: SourceKind::Bluetooth { preferred: Some("x".into()) } };
        assert!(a == b && h(&a) == h(&b));
        let c = SourceSpec { generation: 2, ..a.clone() };
        assert!(a != c && h(&a) != h(&c));
        let d = SourceSpec { generation: 1, kind: SourceKind::Fake(vec![]) };
        assert!(a != d);
    }
}
