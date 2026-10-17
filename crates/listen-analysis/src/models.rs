//! Beat This! model files (MIT, CPJKU / JKU Linz; ONNX by beat-this-rs): downloaded once (~11 MB)
//! from a pinned commit into the cache directory and checked by SHA-256.

use crate::AnalysisError;
use crate::beats::BeatEstimate;
use beat_this::{BeatThis, RtenRuntime, Runtime};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const BASE: &str =
    "https://raw.githubusercontent.com/danigb/beat-this-rs/089b509247e6fdcec666511c0dcf0d5f39c21e73/models";
/// (file name, SHA-256, size in bytes).
const FILES: [(&str, &str, u64); 2] = [
    ("mel_spectrogram.onnx", "fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9", 270_742),
    ("beat_this_small.onnx", "a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f", 10_555_592),
];

/// Paths of a verified model pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatModel {
    pub mel: PathBuf,
    pub beat: PathBuf,
}

impl BeatModel {
    /// The models in `dir` if both are present with the right size (the hash is checked on download).
    pub fn in_dir(dir: &Path) -> Option<Self> {
        let ok = |name: &str, size: u64| std::fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == size);
        FILES
            .iter()
            .all(|(name, _, size)| ok(name, *size))
            .then(|| BeatModel { mel: dir.join(FILES[0].0), beat: dir.join(FILES[1].0) })
    }

    /// Downloads missing files into `dir` (~11 MB), renaming atomically after the hash check.
    pub async fn ensure(dir: &Path) -> Result<Self, AnalysisError> {
        if let Some(m) = Self::in_dir(dir) {
            return Ok(m);
        }
        let err = |reason: String| AnalysisError::Model(reason);
        std::fs::create_dir_all(dir).map_err(|e| err(format!("{}: {e}", dir.display())))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| err(e.to_string()))?;
        for (name, sha, size) in FILES {
            let path = dir.join(name);
            if std::fs::metadata(&path).is_ok_and(|m| m.len() == size) {
                continue;
            }
            let bytes = http
                .get(format!("{BASE}/{name}"))
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| err(format!("downloading {name}: {e}")))?
                .bytes()
                .await
                .map_err(|e| err(format!("downloading {name}: {e}")))?;
            let got = format!("{:x}", Sha256::digest(&bytes));
            if got != sha {
                return Err(err(format!("{name}: checksum mismatch")));
            }
            let tmp = path.with_extension("onnx.part");
            std::fs::write(&tmp, &bytes).map_err(|e| err(format!("{}: {e}", tmp.display())))?;
            std::fs::rename(&tmp, &path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        }
        Self::in_dir(dir).ok_or_else(|| err("model files incomplete".into()))
    }

    /// Beats and downbeats of a 22050 Hz mono signal. Loaded models are pooled process-wide
    /// (each analysis runs on a fresh thread; parsing the model costs ~0.1 s).
    pub fn track(&self, mono: &[f32], sample_rate: u32) -> Result<BeatEstimate, AnalysisError> {
        type Loaded = BeatThis<<RtenRuntime as Runtime>::Model>;
        static POOL: Mutex<Vec<(BeatModel, Loaded)>> = Mutex::new(Vec::new());
        let pooled = {
            let mut pool = POOL.lock().unwrap_or_else(|p| p.into_inner());
            pool.iter().position(|(m, _)| m == self).map(|i| pool.swap_remove(i).1)
        };
        let mut bt = match pooled {
            Some(bt) => bt,
            None => BeatThis::new(&RtenRuntime, &self.mel, &self.beat)
                .map_err(|e| AnalysisError::Model(format!("loading Beat This!: {e}")))?,
        };
        let result = bt.analyze_audio(mono, sample_rate);
        POOL.lock().unwrap_or_else(|p| p.into_inner()).push((self.clone(), bt));
        let a = result.map_err(|e| AnalysisError::Model(format!("Beat This!: {e}")))?;
        // Mean beat probability at the detected beats: the model's certainty.
        let fps = 50.0;
        let probs: Vec<f32> = a
            .beats
            .iter()
            .filter_map(|t| a.beat_logits.get((t * fps).round() as usize))
            .map(|l| 1.0 / (1.0 + (-l).exp()))
            .collect();
        let confidence = (!probs.is_empty()).then(|| probs.iter().sum::<f32>() / probs.len() as f32);
        let shift = |v: Vec<f32>| v.into_iter().map(|t| t + crate::beats::BEAT_THIS_OFFSET).collect();
        Ok(BeatEstimate { beats: shift(a.beats), downbeats: shift(a.downbeats), confidence })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_models_are_not_found() {
        assert!(BeatModel::in_dir(Path::new("/nonexistent")).is_none());
    }

    /// Runs only where the bench has downloaded the models.
    #[test]
    fn beat_this_tracks_a_click_track() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/analysis-bench/models");
        let Some(model) = BeatModel::in_dir(&dir) else { return };
        let pcm = crate::test_util::click_track(120.0, 20.0, 1.0, 0);
        let est = model.track(&pcm.mono(), pcm.sample_rate).unwrap();
        assert!(est.beats.len() >= 30, "{}", est.beats.len());
        let off = |t: f32| ((t - 1.0) / 0.5 - ((t - 1.0) / 0.5).round()).abs() * 0.5;
        let worst = est.beats.iter().map(|t| off(*t)).fold(0.0, f32::max);
        assert!(worst < 0.03, "beats off the clicks by {worst} s");
    }
}
