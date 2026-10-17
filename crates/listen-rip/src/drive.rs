//! Optical drives: which exist, whether an audio CD is in, and its TOC
//! (Linux CD-ROM ioctls; fast, no subprocess). Other platforms get stubs.

use crate::toc::Toc;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscState {
    NoDisc,
    TrayOpen,
    NotReady,
    /// A disc with at least one audio track (audio or mixed/enhanced CD).
    Audio,
    /// A data-only disc (DVD, CD-ROM) — not ripped.
    Data,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveInfo {
    pub device: PathBuf,
    pub vendor: String,
    pub model: String,
}

impl DriveInfo {
    pub fn label(&self) -> String {
        format!("{} {}", self.vendor, self.model).trim().to_string()
    }
}

/// `/dev/sr*` devices, with vendor/model from sysfs.
pub fn list() -> Vec<DriveInfo> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/sys/block") else { return out };
    let mut names: Vec<String> = dir
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("sr"))
        .collect();
    names.sort();
    for name in names {
        let read = |f: &str| {
            std::fs::read_to_string(format!("/sys/block/{name}/device/{f}"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        out.push(DriveInfo {
            device: PathBuf::from(format!("/dev/{name}")),
            vendor: read("vendor"),
            model: read("model"),
        });
    }
    out
}

/// Vendor/model of one device (from sysfs).
pub fn info(device: &Path) -> DriveInfo {
    list().into_iter().find(|d| d.device == device).unwrap_or(DriveInfo {
        device: device.to_path_buf(),
        vendor: String::new(),
        model: String::new(),
    })
}

#[cfg(target_os = "linux")]
mod sys {
    use super::*;
    use crate::toc::TocTrack;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    const CDROMREADTOCHDR: libc::c_ulong = 0x5305;
    const CDROMREADTOCENTRY: libc::c_ulong = 0x5306;
    const CDROM_DRIVE_STATUS: libc::c_ulong = 0x5326;
    const CDROM_DISC_STATUS: libc::c_ulong = 0x5327;
    const CDSL_CURRENT: libc::c_int = i32::MAX;
    const CDROM_LBA: u8 = 0x01;
    const CDROM_LEADOUT: u8 = 0xAA;
    const CDROM_DATA_TRACK: u8 = 0x04;

    #[repr(C)]
    #[derive(Default)]
    struct TocHeader {
        first: u8,
        last: u8,
    }

    #[repr(C)]
    #[derive(Default)]
    struct TocEntry {
        track: u8,
        /// `cdte_adr:4` (low nibble), `cdte_ctrl:4` (high nibble).
        adr_ctrl: u8,
        format: u8,
        /// `union cdrom_addr` (LBA as int).
        lba: libc::c_int,
        datamode: u8,
    }

    fn open(device: &Path) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(device)
    }

    pub fn disc_state(device: &Path) -> DiscState {
        let Ok(f) = open(device) else { return DiscState::Unknown };
        // SAFETY: plain ioctls on an open CD-ROM fd, integer argument.
        let drive = unsafe { libc::ioctl(f.as_raw_fd(), CDROM_DRIVE_STATUS, CDSL_CURRENT) };
        match drive {
            1 => return DiscState::NoDisc,
            2 => return DiscState::TrayOpen,
            3 => return DiscState::NotReady,
            4 => {}
            _ => return DiscState::Unknown,
        }
        // SAFETY: as above.
        match unsafe { libc::ioctl(f.as_raw_fd(), CDROM_DISC_STATUS, 0) } {
            100 | 105 => DiscState::Audio, // CDS_AUDIO, CDS_MIXED
            101..=104 => DiscState::Data,
            1 => DiscState::NoDisc,
            _ => DiscState::Unknown,
        }
    }

    pub fn read_toc(device: &Path) -> Result<Toc, String> {
        let f = open(device).map_err(|e| format!("{}: {e}", device.display()))?;
        let mut hdr = TocHeader::default();
        // SAFETY: CDROMREADTOCHDR fills a `struct cdrom_tochdr` (two u8).
        if unsafe { libc::ioctl(f.as_raw_fd(), CDROMREADTOCHDR, &mut hdr as *mut TocHeader) } != 0 {
            return Err(format!("reading the TOC: {}", std::io::Error::last_os_error()));
        }
        let entry = |track: u8| -> Result<(u32, bool), String> {
            let mut e = TocEntry { track, format: CDROM_LBA, ..Default::default() };
            // SAFETY: CDROMREADTOCENTRY fills a `struct cdrom_tocentry` (layout above).
            if unsafe { libc::ioctl(f.as_raw_fd(), CDROMREADTOCENTRY, &mut e as *mut TocEntry) } != 0 {
                return Err(format!("TOC entry {track}: {}", std::io::Error::last_os_error()));
            }
            Ok((e.lba.max(0) as u32, (e.adr_ctrl >> 4) & CDROM_DATA_TRACK == 0))
        };
        let mut tracks = Vec::new();
        for n in hdr.first..=hdr.last {
            let (lba, audio) = entry(n)?;
            tracks.push(TocTrack { number: n, lba, audio });
        }
        let (leadout_lba, _) = entry(CDROM_LEADOUT)?;
        if tracks.iter().all(|t| !t.audio) {
            return Err("no audio tracks on this disc".into());
        }
        Ok(Toc { tracks, leadout_lba })
    }

    #[cfg(test)]
    #[test]
    fn toc_entry_layout_matches_the_kernel() {
        // struct cdrom_tocentry: 12 bytes, the address union at offset 4.
        assert_eq!(std::mem::size_of::<TocEntry>(), 12);
        assert_eq!(std::mem::offset_of!(TocEntry, lba), 4);
        assert_eq!(std::mem::offset_of!(TocEntry, datamode), 8);
    }
}

#[cfg(target_os = "linux")]
pub use sys::{disc_state, read_toc};

#[cfg(not(target_os = "linux"))]
pub fn disc_state(_device: &Path) -> DiscState {
    DiscState::Unknown
}

#[cfg(not(target_os = "linux"))]
pub fn read_toc(_device: &Path) -> Result<Toc, String> {
    Err("CD ripping is only available on Linux".into())
}

/// Ejects through UDisks2 (no root needed), falling back to `eject`.
pub async fn eject(device: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        match crate::detect::eject_udisks(device).await {
            Ok(()) => return Ok(()),
            Err(e) => tracing::debug!("UDisks2 eject failed ({e}), trying `eject`"),
        }
    }
    let out = tokio::process::Command::new("eject")
        .arg(device)
        .output()
        .await
        .map_err(|e| format!("eject: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("eject: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
