//! Simulated CD insertion against a private D-Bus daemon with a fake UDisks2 (one optical
//! drive, /dev/sr0): the watcher reports the drive, an inserted audio CD and its removal,
//! also after Listen's own Eject call.
#![cfg(target_os = "linux")]

use listen_rip::detect::{self, DriveEvent};
use std::collections::HashMap;
use std::io::BufRead;
use std::time::Duration;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

const DRIVE: &str = "/org/freedesktop/UDisks2/drives/HL_DT_ST_DVDRAM_GU70N_TEST";
const BLOCK: &str = "/org/freedesktop/UDisks2/block_devices/sr0";

struct FakeDrive {
    media: bool,
    audio_tracks: u32,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl FakeDrive {
    #[zbus(property)]
    fn vendor(&self) -> String {
        "HL-DT-ST".into()
    }
    #[zbus(property)]
    fn model(&self) -> String {
        "DVDRAM GU70N".into()
    }
    #[zbus(property)]
    fn optical(&self) -> bool {
        self.media
    }
    #[zbus(property)]
    fn media_available(&self) -> bool {
        self.media
    }
    #[zbus(property)]
    fn optical_num_audio_tracks(&self) -> u32 {
        self.audio_tracks
    }
    async fn eject(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.media = false;
        self.audio_tracks = 0;
        self.media_available_changed(&emitter).await?;
        self.optical_num_audio_tracks_changed(&emitter).await?;
        Ok(())
    }
}

struct FakeBlock;

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl FakeBlock {
    #[zbus(property)]
    fn device(&self) -> Vec<u8> {
        b"/dev/sr0\0".to_vec()
    }
    #[zbus(property)]
    fn drive(&self) -> OwnedObjectPath {
        ObjectPath::try_from(DRIVE).unwrap().into()
    }
}

struct Daemon(std::process::Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn private_bus() -> Option<(Daemon, String)> {
    let mut child = std::process::Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
    Some((Daemon(child), line.trim().to_string()))
}

async fn next(rx: &mut tokio::sync::mpsc::Receiver<DriveEvent>) -> DriveEvent {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("no event within 10 s")
        .expect("watcher ended")
}

#[tokio::test(flavor = "multi_thread")]
async fn insert_and_remove_an_audio_cd() {
    let Some((_daemon, address)) = private_bus() else {
        eprintln!("dbus-daemon not available; skipping");
        return;
    };
    let udisks = zbus::connection::Builder::address(address.as_str())
        .unwrap()
        .name("org.freedesktop.UDisks2")
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at(DRIVE, FakeDrive { media: false, audio_tracks: 0 })
        .unwrap()
        .serve_at(BLOCK, FakeBlock)
        .unwrap()
        .build()
        .await
        .unwrap();

    let client = zbus::connection::Builder::address(address.as_str()).unwrap().build().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let c2 = client.clone();
    tokio::spawn(async move { detect::watch_on(&c2, &tx).await });

    // The drive, empty.
    match next(&mut rx).await {
        DriveEvent::Drives(d) => {
            assert_eq!(d.len(), 1);
            assert_eq!(d[0].device.to_str(), Some("/dev/sr0"));
            assert_eq!(d[0].label(), "HL-DT-ST DVDRAM GU70N");
        }
        e => panic!("expected the drive list, got {e:?}"),
    }

    // Insert an audio CD: UDisks2 updates the drive's properties.
    let iface = udisks.object_server().interface::<_, FakeDrive>(DRIVE).await.unwrap();
    {
        let mut d = iface.get_mut().await;
        d.media = true;
        d.audio_tracks = 12;
    }
    {
        let d = iface.get().await;
        d.media_available_changed(iface.signal_emitter()).await.unwrap();
        d.optical_num_audio_tracks_changed(iface.signal_emitter()).await.unwrap();
    }
    assert_eq!(next(&mut rx).await, DriveEvent::Inserted { device: "/dev/sr0".into() });

    // Listen ejects it through UDisks2 → removed.
    detect::eject_on(&client, std::path::Path::new("/dev/sr0")).await.unwrap();
    assert_eq!(next(&mut rx).await, DriveEvent::Removed { device: "/dev/sr0".into() });
    assert!(!iface.get().await.media);
}
