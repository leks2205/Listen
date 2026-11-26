//! Optical drives: which exist, whether an audio CD is in, and its TOC
//! (Linux CD-ROM ioctls; fast, no subprocess). Other platforms get stubs.

use crate::tags::CdText;
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

    const SG_IO: libc::c_ulong = 0x2285;
    const SG_DXFER_FROM_DEV: libc::c_int = -3;

    /// `struct sg_io_hdr` (scsi/sg.h).
    #[repr(C)]
    struct SgIoHdr {
        interface_id: libc::c_int,
        dxfer_direction: libc::c_int,
        cmd_len: u8,
        mx_sb_len: u8,
        iovec_count: libc::c_ushort,
        dxfer_len: libc::c_uint,
        dxferp: *mut libc::c_void,
        cmdp: *const u8,
        sbp: *mut u8,
        timeout: libc::c_uint,
        flags: libc::c_uint,
        pack_id: libc::c_int,
        usr_ptr: *mut libc::c_void,
        status: u8,
        masked_status: u8,
        msg_status: u8,
        sb_len_wr: u8,
        host_status: libc::c_ushort,
        driver_status: libc::c_ushort,
        resid: libc::c_int,
        duration: libc::c_uint,
        info: libc::c_uint,
    }

    /// READ TOC/PMA/ATIP, format 5 (CD-Text), `len` bytes.
    fn read_toc_format5(f: &std::fs::File, len: u16) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; usize::from(len)];
        let mut sense = [0u8; 32];
        let [hi, lo] = len.to_be_bytes();
        let cmd: [u8; 10] = [0x43, 0, 0x05, 0, 0, 0, 0, hi, lo, 0];
        let mut hdr = SgIoHdr {
            interface_id: b'S' as libc::c_int,
            dxfer_direction: SG_DXFER_FROM_DEV,
            cmd_len: cmd.len() as u8,
            mx_sb_len: sense.len() as u8,
            iovec_count: 0,
            dxfer_len: libc::c_uint::from(len),
            dxferp: buf.as_mut_ptr().cast(),
            cmdp: cmd.as_ptr(),
            sbp: sense.as_mut_ptr(),
            timeout: 10_000,
            flags: 0,
            pack_id: 0,
            usr_ptr: std::ptr::null_mut(),
            status: 0,
            masked_status: 0,
            msg_status: 0,
            sb_len_wr: 0,
            host_status: 0,
            driver_status: 0,
            resid: 0,
            duration: 0,
            info: 0,
        };
        // SAFETY: SG_IO with a header whose buffers (`buf`, `cmd`, `sense`) outlive the call and whose
        // lengths are the buffers' lengths; the kernel writes at most `dxfer_len` bytes into `buf`.
        let r = unsafe { libc::ioctl(f.as_raw_fd(), SG_IO, &mut hdr as *mut SgIoHdr) };
        if r != 0 || hdr.status != 0 || hdr.host_status != 0 || hdr.driver_status != 0 {
            return None;
        }
        let got = usize::from(len).saturating_sub(hdr.resid.max(0) as usize);
        buf.truncate(got);
        Some(buf)
    }

    /// The disc's CD-Text, read from the drive (`None`: the drive or the disc has none).
    pub fn read_cdtext(device: &Path) -> Option<CdText> {
        let f = open(device).ok()?;
        let head = read_toc_format5(&f, 4)?;
        if head.len() < 4 {
            return None;
        }
        // The length field counts what follows it.
        let len = u16::from_be_bytes([head[0], head[1]]).saturating_add(2);
        if len <= 4 {
            return None;
        }
        let data = read_toc_format5(&f, len)?;
        let text = super::parse_cdtext(data.get(4..)?);
        (!text.is_empty()).then_some(text)
    }

    #[cfg(test)]
    #[test]
    fn sg_io_header_layout_matches_the_kernel() {
        assert_eq!(std::mem::size_of::<SgIoHdr>(), 88);
        assert_eq!(std::mem::offset_of!(SgIoHdr, dxferp), 16);
        assert_eq!(std::mem::offset_of!(SgIoHdr, status), 64);
        assert_eq!(std::mem::offset_of!(SgIoHdr, resid), 72);
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
pub use sys::{disc_state, read_cdtext, read_toc};

#[cfg(not(target_os = "linux"))]
pub fn read_cdtext(_device: &Path) -> Option<CdText> {
    None
}

/// CD-Text from the packs of READ TOC format 5 (18 bytes each, after the 4-byte header): block 0
/// (the first language), album/track titles (0x80) and performers (0x81). Character code
/// ISO-8859-1 or ASCII; a string that is valid UTF-8 is taken as UTF-8 (as with cdrdao's `.toc`).
/// Double-byte text (Japanese, Korean, Chinese) is not decoded and gives no CD-Text.
pub fn parse_cdtext(packs: &[u8]) -> CdText {
    let mut text = CdText::default();
    // Pack type → (first track number, concatenated text).
    let mut fields: [Option<(u8, Vec<u8>)>; 2] = [None, None];
    for p in packs.as_chunks::<18>().0 {
        let (kind, track, block) = (p[0], p[1] & 0x7F, (p[3] >> 4) & 0x07);
        if block != 0 {
            continue;
        }
        if kind == 0x8F && p[2] == 0 {
            // Size information: its first byte is the character code of the block.
            if p[4] >= 0x80 {
                return CdText::default();
            }
            continue;
        }
        if p[3] & 0x80 != 0 {
            return CdText::default();
        }
        let slot = match kind {
            0x80 => 0,
            0x81 => 1,
            _ => continue,
        };
        fields[slot].get_or_insert_with(|| (track, Vec::new())).1.extend_from_slice(&p[4..16]);
    }
    let decode = |b: &[u8]| match std::str::from_utf8(b) {
        Ok(s) => s.trim().to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect::<String>().trim().to_string(),
    };
    for (slot, field) in fields.iter().enumerate() {
        let Some((first, bytes)) = field else { continue };
        let mut previous = String::new();
        // The last piece after the final NUL is padding.
        let mut pieces: Vec<&[u8]> = bytes.split(|&b| b == 0).collect();
        pieces.pop();
        for (i, piece) in pieces.into_iter().enumerate() {
            let number = usize::from(*first) + i;
            // A single TAB: the same as the previous track.
            let value = if piece == b"\t" { previous.clone() } else { decode(piece) };
            previous = value.clone();
            if number == 0 {
                if slot == 0 {
                    text.title = value;
                } else {
                    text.performer = value;
                }
                continue;
            }
            if number > 99 {
                break;
            }
            if text.tracks.len() < number {
                text.tracks.resize(number, Default::default());
            }
            let t = &mut text.tracks[number - 1];
            if slot == 0 {
                t.0 = value;
            } else {
                t.1 = value;
            }
        }
    }
    // Packs are padded with NULs: empty "tracks" at the end are padding.
    while text.tracks.last().is_some_and(|(t, p)| t.is_empty() && p.is_empty()) {
        text.tracks.pop();
    }
    text
}

/// The drive's top speed from `/proc/sys/dev/cdrom/info` (`drive name:` and `drive speed:`
/// columns, one per drive).
pub fn max_speed(info: &str, device: &Path) -> Option<u32> {
    let name = device.file_name()?.to_str()?;
    let column = |key: &str| -> Option<Vec<&str>> {
        info.lines().find_map(|l| l.strip_prefix(key)).map(|rest| rest.split_whitespace().collect())
    };
    let names = column("drive name:")?;
    let speeds = column("drive speed:")?;
    let i = names.iter().position(|n| *n == name)?;
    speeds.get(i)?.parse().ok().filter(|s| *s > 0)
}

/// Sets the drive to its full read speed (`eject -x`); `None` when the speed isn't known, which
/// leaves the drive alone.
pub async fn full_speed(device: &Path) -> Option<u32> {
    let info = std::fs::read_to_string("/proc/sys/dev/cdrom/info").ok()?;
    let speed = max_speed(&info, device)?;
    match tokio::process::Command::new("eject")
        .arg("-x")
        .arg(speed.to_string())
        .arg(device)
        .kill_on_drop(true)
        .output()
        .await
    {
        Ok(o) if o.status.success() => Some(speed),
        Ok(o) => {
            tracing::info!("eject -x {speed}: {}", String::from_utf8_lossy(&o.stderr).trim());
            None
        }
        Err(e) => {
            tracing::info!("eject -x {speed}: {e}");
            None
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// CD-Text packs of `kind` for these strings (disc first, then the tracks), 12 bytes each.
    fn packs(kind: u8, strings: &[&[u8]], seq: &mut u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        // The track each pack's first character belongs to.
        let mut owner = Vec::new();
        for (track, s) in strings.iter().enumerate() {
            for _ in 0..=s.len() {
                owner.push(track as u8);
            }
            bytes.extend_from_slice(s);
            bytes.push(0);
        }
        let mut out = Vec::new();
        for (i, chunk) in bytes.chunks(12).enumerate() {
            let mut p = [0u8; 18];
            p[0] = kind;
            p[1] = owner[i * 12];
            p[2] = *seq;
            *seq += 1;
            p[4..4 + chunk.len()].copy_from_slice(chunk);
            out.extend_from_slice(&p);
        }
        out
    }

    #[test]
    fn cdtext_from_the_drive() {
        let mut seq = 0;
        let mut data = packs(0x80, &[b"Mix Tape", b"Intro", b"M\xfcde", b"\t"], &mut seq);
        data.extend(packs(0x81, &[b"Caf\xc3\xa9 Band", b"", b"Guest", b"Guest"], &mut seq));
        // Another language's block is ignored.
        let mut other = packs(0x80, &[b"Other", b"X"], &mut seq);
        other[3] |= 0x10;
        data.extend(&other[..18]);
        let mut size = [0u8; 18];
        size[0] = 0x8F;
        data.extend(size);
        let t = parse_cdtext(&data);
        assert_eq!(t.title, "Mix Tape");
        assert_eq!(t.performer, "Café Band");
        assert_eq!(
            t.tracks,
            vec![
                ("Intro".into(), String::new()),
                ("Müde".into(), "Guest".into()),
                ("Müde".into(), "Guest".into())
            ]
        );
        assert!(parse_cdtext(&[]).is_empty());
        // Double-byte (here MS-JIS) text isn't decoded: no CD-Text, as on a disc without it.
        size[4] = 0x80;
        let mut jis = packs(0x80, &[b"\x83\x65", b"\x83\x67"], &mut seq);
        jis.extend(size);
        assert!(parse_cdtext(&jis).is_empty());
    }

    #[test]
    fn top_speed_of_the_drive() {
        let info = "CD-ROM information, Id: cdrom.c 3.20 2003/12/17\n\ndrive name:\t\tsr1\tsr0\ndrive speed:\t\t48\t24\nCan change speed:\t1\t1\n";
        assert_eq!(max_speed(info, Path::new("/dev/sr0")), Some(24));
        assert_eq!(max_speed(info, Path::new("/dev/sr1")), Some(48));
        assert_eq!(max_speed(info, Path::new("/dev/sr2")), None);
        assert_eq!(max_speed("drive name:\tsr0\ndrive speed:\t0\n", Path::new("/dev/sr0")), None);
        assert_eq!(max_speed("", Path::new("/dev/sr0")), None);
    }
}
