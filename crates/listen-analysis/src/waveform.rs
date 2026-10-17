//! Waveforms for the player bar's seek bar: the loudest sample of each slice
//! of a song, from the same mpv decode the analysis uses, cached on disk.

use crate::{AnalysisError, Cancel, Pcm};
use std::path::{Path, PathBuf};

/// Slices stored per song: enough for a wide bar of 2 px bars with gaps.
pub const BINS: usize = 1000;
const MAGIC: &[u8; 4] = b"LWF1";
/// The cache keeps at most this many files.
const MAX_FILES: usize = 4000;

/// Peak amplitude (0..=255 of full scale) per slice of the song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    pub peaks: Vec<u8>,
    pub duration_ms: u32,
}

impl Waveform {
    /// The loudest sample (either channel) of each of `bins` equal slices.
    pub fn from_pcm(pcm: &Pcm, bins: usize) -> Waveform {
        let n = pcm.left.len().min(pcm.right.len());
        let bins = bins.max(1);
        let mut peaks = vec![0u8; bins];
        for (b, peak) in peaks.iter_mut().enumerate() {
            let (lo, hi) = (b * n / bins, ((b + 1) * n / bins).max(b * n / bins + 1).min(n));
            let loudest = (lo..hi).map(|i| pcm.left[i].abs().max(pcm.right[i].abs())).fold(0.0f32, f32::max);
            *peak = (loudest.min(1.0) * 255.0).round() as u8;
        }
        Waveform { peaks, duration_ms: (pcm.duration() * 1000.0) as u32 }
    }

    /// `bars` values in 0..=1 for drawing: the loudest slice under each bar.
    /// With `stretch` the loudest bar of the song reaches the full height.
    pub fn bars(&self, bars: usize, stretch: bool) -> Vec<f32> {
        let out = downsample(&self.peaks, bars);
        let top = if stretch { out.iter().cloned().fold(0.0, f32::max).max(1.0 / 255.0) } else { 1.0 };
        out.into_iter().map(|v| (v / top).min(1.0)).collect()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.peaks.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.peaks.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.duration_ms.to_le_bytes());
        out.extend_from_slice(&self.peaks);
        out
    }

    /// `None` for anything that isn't a complete file written by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Option<Waveform> {
        let (head, rest) = bytes.split_at_checked(12)?;
        if &head[..4] != MAGIC {
            return None;
        }
        let len = u32::from_le_bytes(head[4..8].try_into().ok()?) as usize;
        let duration_ms = u32::from_le_bytes(head[8..12].try_into().ok()?);
        (len > 0 && len == rest.len()).then(|| Waveform { peaks: rest.to_vec(), duration_ms })
    }
}

/// Resamples `peaks` (0..=255) to `n` values in 0..=1: the loudest value under each
/// output slot, values repeated when `n` is larger.
pub fn downsample(peaks: &[u8], n: usize) -> Vec<f32> {
    if peaks.is_empty() || n == 0 {
        return vec![0.0; n];
    }
    let len = peaks.len();
    (0..n)
        .map(|i| {
            let (lo, hi) = (i * len / n, ((i + 1) * len / n).max(i * len / n + 1).min(len));
            f32::from(*peaks[lo..hi].iter().max().unwrap_or(&0)) / 255.0
        })
        .collect()
}

/// Decodes `input` (URL or path) with mpv and computes its waveform.
pub async fn compute(input: &str, cancel: &Cancel) -> Result<Waveform, AnalysisError> {
    let pcm = crate::decode::decode(input, cancel).await?;
    Ok(Waveform::from_pcm(&pcm, BINS))
}

/// Waveforms on disk, one small file per song.
#[derive(Debug, Clone)]
pub struct WaveformCache {
    dir: PathBuf,
}

impl WaveformCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, key: &str) -> PathBuf {
        let name: String =
            key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        // Keys that only differ in odd characters must not share a file.
        let tag = key
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
        self.dir.join(format!("{}-{tag:016x}.wf", &name[..name.len().min(60)]))
    }

    pub fn get(&self, key: &str) -> Option<Waveform> {
        Waveform::from_bytes(&std::fs::read(self.path(key)).ok()?)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.path(key).is_file()
    }

    pub fn put(&self, key: &str, wf: &Waveform) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path(key);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, wf.to_bytes())?;
        std::fs::rename(tmp, path)?;
        self.prune();
        Ok(())
    }

    /// Deletes the oldest files beyond [`MAX_FILES`].
    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "wf"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        if files.len() <= MAX_FILES {
            return;
        }
        files.sort();
        for (_, path) in files.iter().take(files.len() - MAX_FILES) {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(left: Vec<f32>, right: Vec<f32>) -> Pcm {
        Pcm { sample_rate: 1000, left, right }
    }

    #[test]
    fn peaks_are_the_loudest_sample_of_each_slice() {
        // 4 slices of 2 samples; the right channel is louder in slice 2.
        let wf = Waveform::from_pcm(
            &pcm(
                vec![0.0, 0.5, -0.25, 0.0, 0.1, 0.1, 0.0, -1.0],
                vec![0.0, 0.0, 0.0, -0.75, 0.0, 0.0, 0.0, 0.0],
            ),
            4,
        );
        assert_eq!(wf.peaks, [128, 191, 26, 255]);
        assert_eq!(wf.duration_ms, 8);
    }

    #[test]
    fn short_and_empty_input() {
        let wf = Waveform::from_pcm(&pcm(vec![0.5], vec![0.5]), 4);
        assert_eq!(wf.peaks.len(), 4);
        assert!(wf.peaks.contains(&128));
        let empty = Waveform::from_pcm(&pcm(vec![], vec![]), 3);
        assert_eq!(empty.peaks, [0, 0, 0]);
    }

    #[test]
    fn downsampling_keeps_the_loudest_and_repeats_when_upsampling() {
        let peaks = [0, 255, 0, 0, 51, 0, 0, 0];
        let d = downsample(&peaks, 4);
        assert_eq!(d, [1.0, 0.0, 0.2, 0.0]);
        assert_eq!(downsample(&peaks, 2), [1.0, 0.2]);
        assert_eq!(downsample(&[255, 0], 4), [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(downsample(&[], 3), [0.0; 3]);
        assert!(downsample(&peaks, 0).is_empty());
    }

    #[test]
    fn stretch_scales_the_loudest_bar_to_full_height() {
        let wf = Waveform { peaks: vec![51, 102, 0, 25], duration_ms: 1000 };
        let plain = wf.bars(4, false);
        assert!((plain[1] - 0.4).abs() < 1e-6);
        let stretched = wf.bars(4, true);
        assert!((stretched[1] - 1.0).abs() < 1e-6 && (stretched[0] - 0.5).abs() < 1e-6);
        // Silence stays silent.
        assert!(Waveform { peaks: vec![0; 4], duration_ms: 1 }.bars(4, true).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn bytes_round_trip_and_garbage_is_rejected() {
        let wf = Waveform { peaks: (0..=255).collect(), duration_ms: 215_000 };
        assert_eq!(Waveform::from_bytes(&wf.to_bytes()), Some(wf.clone()));
        let mut bytes = wf.to_bytes();
        bytes.pop();
        assert_eq!(Waveform::from_bytes(&bytes), None, "truncated");
        assert_eq!(Waveform::from_bytes(b"nope"), None);
        let mut bad = wf.to_bytes();
        bad[0] = b'X';
        assert_eq!(Waveform::from_bytes(&bad), None, "wrong magic");
    }

    #[test]
    fn cache_stores_per_key_and_survives_odd_keys() {
        let dir = std::env::temp_dir().join(format!("listen-waveform-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = WaveformCache::new(&dir);
        let a = Waveform { peaks: vec![1, 2, 3], duration_ms: 10 };
        let b = Waveform { peaks: vec![9, 9], duration_ms: 20 };
        assert!(cache.get("srv-song1").is_none() && !cache.contains("srv-song1"));
        cache.put("srv-song1", &a).unwrap();
        cache.put("ytm:a/b?c", &b).unwrap();
        cache.put("ytm:a_b_c", &a).unwrap();
        assert_eq!(cache.get("srv-song1"), Some(a.clone()));
        assert_eq!(cache.get("ytm:a/b?c"), Some(b), "keys that sanitise the same stay apart");
        assert_eq!(cache.get("ytm:a_b_c"), Some(a));
        // A damaged file is a miss, not an error.
        std::fs::write(cache.path("srv-song1"), b"junk").unwrap();
        assert!(cache.get("srv-song1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn computes_from_a_real_file_when_mpv_is_there() {
        if !std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("mpv").is_file()))
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("listen-waveform-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 2 s mono 8 kHz WAV: silence, then a loud second.
        let mut data = Vec::new();
        for i in 0..16000u32 {
            let v: i16 = if i < 8000 { 0 } else { (8000.0 * (i as f32 * 0.3).sin()) as i16 };
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 1, 0]);
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        let path = dir.join("loud-half.wav");
        std::fs::write(&path, wav).unwrap();
        let wf = compute(path.to_str().unwrap(), &Cancel::default()).await.unwrap();
        assert_eq!(wf.peaks.len(), BINS);
        assert!(wf.peaks[..BINS / 2 - 20].iter().all(|&p| p < 5), "first half silent");
        assert!(
            wf.peaks[BINS / 2 + 20..].iter().all(|&p| p > 30),
            "second half loud: {:?}",
            &wf.peaks[BINS / 2 + 20..BINS / 2 + 30]
        );
        assert!((1900..=2100).contains(&wf.duration_ms), "about 2 s, got {}", wf.duration_ms);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
