//! Singing-voice detector: the CRNN of DJustify (Hsu et al., "DJustify: Reason-and-Verify DJ Planning for
//! Pop Transitions and Story-Driven Sets", 2026), trained on the Jamendo corpus. Its code is MIT; its
//! weights (`svdnet.pt`, ~2 MB) are CC BY-NC 4.0, so Listen doesn't ship them: the user downloads them
//! from the authors' repository on request, and Listen reads the PyTorch file directly (log-mel front end,
//! four conv blocks, a bidirectional LSTM and a per-frame head, run with rten's operators).
//! Without the file the analyser keeps its own vocal heuristic.

use crate::AnalysisError;
use realfft::RealFftPlanner;
use rten::BufferPool;
use rten::ops::{Direction, Padding, conv, lstm};
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, Tensor};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The weights at a pinned commit of the DJustify repository.
pub const URL: &str = "https://raw.githubusercontent.com/ddman1101/DJustify/92ba9fb5e6472c262907ce3ceef8e008e000779e/svd/svdnet.pt";
const SHA256: &str = "f54ff69bd8376bb258f0807fa2d15be2670207b481e81eac2c750d3387a753d9";
pub const SIZE: u64 = 2_116_758;
pub const FILE_NAME: &str = "djustify-svdnet.pt";
/// The licence of the weights, for the settings row and About.
pub const LICENSE: &str = "CC BY-NC 4.0";

const SR: usize = 22050;
const N_FFT: usize = 1024;
const HOP: usize = 315;
const N_MELS: usize = 80;
const FMIN: f64 = 27.5;
const FMAX: f64 = 8000.0;
/// Frames per second of the detector (70).
pub const FPS: f32 = SR as f32 / HOP as f32;
/// Output frames per conv pass (bounds memory on long songs; the result doesn't depend on it).
const CHUNK: usize = 2048;
/// Frames of context each conv pass needs on either side (four 3×3 convolutions).
const CONTEXT: usize = 4;

/// The downloaded weights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceModel {
    pub path: PathBuf,
}

impl VoiceModel {
    /// The model in `dir` if the file is there with the right size (the hash is checked on download).
    pub fn in_dir(dir: &Path) -> Option<Self> {
        let path = dir.join(FILE_NAME);
        std::fs::metadata(&path).is_ok_and(|m| m.len() == SIZE).then_some(VoiceModel { path })
    }

    /// A model file somewhere else (the Android app bundles it).
    pub fn at(path: PathBuf) -> Self {
        VoiceModel { path }
    }

    /// Downloads the weights into `dir` (~2 MB), renaming atomically after the hash check.
    pub async fn download(dir: &Path) -> Result<Self, AnalysisError> {
        if let Some(m) = Self::in_dir(dir) {
            return Ok(m);
        }
        let err = |reason: String| AnalysisError::Model(reason);
        std::fs::create_dir_all(dir).map_err(|e| err(format!("{}: {e}", dir.display())))?;
        let http = reqwest::Client::builder()
            .user_agent(listen_core::USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| err(e.to_string()))?;
        let bytes = http
            .get(URL)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| err(format!("downloading the singing-voice detector: {e}")))?
            .bytes()
            .await
            .map_err(|e| err(format!("downloading the singing-voice detector: {e}")))?;
        if format!("{:x}", Sha256::digest(&bytes)) != SHA256 {
            return Err(err("singing-voice detector: checksum mismatch".into()));
        }
        // Refuse a file the detector can't read before it lands.
        SvdNet::from_pt(&bytes).map_err(err)?;
        let path = dir.join(FILE_NAME);
        let tmp = path.with_extension("pt.part");
        std::fs::write(&tmp, &bytes).map_err(|e| err(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        Ok(VoiceModel { path })
    }

    /// Deletes the downloaded weights from `dir`.
    pub fn remove(dir: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(dir.join(FILE_NAME)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// The parsed network, cached process-wide per file.
    fn net(&self) -> Result<Arc<SvdNet>, AnalysisError> {
        static LOADED: Mutex<Vec<(PathBuf, Arc<SvdNet>)>> = Mutex::new(Vec::new());
        let mut loaded = LOADED.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((_, net)) = loaded.iter().find(|(p, _)| *p == self.path) {
            return Ok(net.clone());
        }
        let bytes = std::fs::read(&self.path)
            .map_err(|e| AnalysisError::Model(format!("{}: {e}", self.path.display())))?;
        let net = Arc::new(SvdNet::from_pt(&bytes).map_err(AnalysisError::Model)?);
        loaded.push((self.path.clone(), net.clone()));
        Ok(net)
    }

    /// Sung stretches (seconds) of a 22050 Hz mono signal.
    pub fn voiced(&self, mono: &[f32], sample_rate: u32) -> Result<Vec<(f32, f32)>, AnalysisError> {
        if sample_rate as usize != SR {
            return Err(AnalysisError::Model(format!(
                "singing-voice detector needs {SR} Hz, got {sample_rate}"
            )));
        }
        let probs = self.net()?.probabilities(mono).map_err(AnalysisError::Model)?;
        Ok(voiced_segments(&probs))
    }
}

/// Sung stretches from per-frame singing probabilities, as DJustify's `voiced_segments`: frames above 0.5,
/// a 0.4 s majority (median) filter, gaps under 0.3 s bridged, stretches under 0.4 s dropped.
pub fn voiced_segments(probs: &[f32]) -> Vec<(f32, f32)> {
    let n = probs.len();
    let on: Vec<u8> = probs.iter().map(|&p| u8::from(p > 0.5)).collect();
    let k = (0.4 * FPS) as usize | 1;
    let half = k / 2;
    // scipy.ndimage.median_filter, mode "reflect" (d c b a | a b c d | d c b a).
    let at = |i: isize| -> u8 {
        let n = n as isize;
        let mut i = i;
        loop {
            if i < 0 {
                i = -i - 1;
            } else if i >= n {
                i = 2 * n - i - 1;
            } else {
                return on[i as usize];
            }
        }
    };
    let mut sum: usize = (-(half as isize)..=half as isize).map(|d| at(d) as usize).sum();
    let mut v = vec![0u8; n];
    for (i, slot) in v.iter_mut().enumerate() {
        *slot = u8::from(sum > half);
        sum = sum + at((i + half + 1) as isize) as usize - at(i as isize - half as isize) as usize;
    }
    let mut runs: Vec<(f32, f32)> = Vec::new();
    let mut i = 0;
    while i < n {
        if v[i] == 1 {
            let j = (i..n).find(|&j| v[j] == 0).unwrap_or(n);
            let (a, b) = (i as f32 / FPS, j as f32 / FPS);
            match runs.last_mut() {
                Some(last) if a - last.1 < 0.30 => last.1 = b,
                _ => runs.push((a, b)),
            }
            i = j;
        } else {
            i += 1;
        }
    }
    runs.retain(|(a, b)| b - a >= 0.4);
    runs
}

/// Fraction of each bar (consecutive `bounds`, seconds) covered by `voiced`.
pub fn bar_fractions(voiced: &[(f32, f32)], bounds: &[f32]) -> Vec<f32> {
    bounds
        .windows(2)
        .map(|w| {
            let len = w[1] - w[0];
            if len <= 0.0 {
                return 0.0;
            }
            let sung: f32 = voiced.iter().map(|&(s, e)| (e.min(w[1]) - s.max(w[0])).max(0.0)).sum();
            (sung / len).min(1.0)
        })
        .collect()
}

/// The bar vocal curve (0..1) from sung stretches: a bar sung for a quarter of its length or more is vocal
/// (≥ 0.5, rising to 1 when sung throughout), less is scaled below 0.5.
pub fn bar_vocal(voiced: &[(f32, f32)], bounds: &[f32]) -> Vec<f32> {
    bar_fractions(voiced, bounds)
        .into_iter()
        .map(|f| if f >= 0.25 { 0.5 + 0.5 * (f - 0.25) / 0.75 } else { 2.0 * f })
        .collect()
}

// ---- log-mel front end (librosa.feature.melspectrogram, power 1, Slaney mel, then log1p) ----

fn hz_to_mel(f: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let (min_log_hz, logstep) = (1000.0, 6.4f64.ln() / 27.0);
    if f >= min_log_hz { min_log_hz / f_sp + (f / min_log_hz).ln() / logstep } else { f / f_sp }
}

fn mel_to_hz(m: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let (min_log_hz, logstep) = (1000.0, 6.4f64.ln() / 27.0);
    let min_log_mel = min_log_hz / f_sp;
    if m >= min_log_mel { min_log_hz * (logstep * (m - min_log_mel)).exp() } else { f_sp * m }
}

/// librosa.filters.mel(sr, n_fft, n_mels, fmin, fmax) with Slaney scale and norm: `[N_MELS][bins]`.
fn mel_filters() -> Vec<Vec<f32>> {
    let bins = N_FFT / 2 + 1;
    let (lo, hi) = (hz_to_mel(FMIN), hz_to_mel(FMAX));
    let mel_f: Vec<f64> =
        (0..N_MELS + 2).map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (N_MELS + 1) as f64)).collect();
    let fft_f: Vec<f64> = (0..bins).map(|k| k as f64 * SR as f64 / N_FFT as f64).collect();
    (0..N_MELS)
        .map(|i| {
            let (d0, d1) = (mel_f[i + 1] - mel_f[i], mel_f[i + 2] - mel_f[i + 1]);
            let enorm = 2.0 / (mel_f[i + 2] - mel_f[i]);
            fft_f
                .iter()
                .map(|&f| {
                    let lower = (f - mel_f[i]) / d0;
                    let upper = (mel_f[i + 2] - f) / d1;
                    (lower.min(upper).max(0.0) * enorm) as f32
                })
                .collect()
        })
        .collect()
}

/// log(1 + mel magnitude), `[N_MELS][frames]` row-major, centred frames with zero padding.
pub fn logmel(mono: &[f32]) -> (Vec<f32>, usize) {
    let frames = 1 + mono.len() / HOP;
    let window: Vec<f32> = (0..N_FFT)
        .map(|n| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos()) as f32)
        .collect();
    let filters = mel_filters();
    // Each filter's non-zero bins.
    let spans: Vec<(usize, usize)> = filters
        .iter()
        .map(|w| {
            let a = w.iter().position(|&x| x > 0.0).unwrap_or(0);
            let b = w.iter().rposition(|&x| x > 0.0).map_or(a, |b| b + 1);
            (a, b)
        })
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut mag = vec![0f32; N_FFT / 2 + 1];
    let mut out = vec![0f32; N_MELS * frames];
    let pad = N_FFT / 2;
    for t in 0..frames {
        let start = (t * HOP) as isize - pad as isize;
        for (n, x) in input.iter_mut().enumerate() {
            let i = start + n as isize;
            *x = if i >= 0 && (i as usize) < mono.len() { mono[i as usize] * window[n] } else { 0.0 };
        }
        let _ = fft.process(&mut input, &mut spectrum);
        for (m, c) in mag.iter_mut().zip(&spectrum) {
            *m = c.norm();
        }
        for (b, (w, &(a, e))) in filters.iter().zip(&spans).enumerate() {
            let s: f32 = (a..e).map(|k| w[k] * mag[k]).sum();
            out[b * frames + t] = s.ln_1p();
        }
    }
    (out, frames)
}

// ---- the network ----

struct ConvBlock {
    /// BatchNorm folded in: `[co, ci, 3, 3]` and `[co]`.
    weight: Tensor<f32>,
    bias: Tensor<f32>,
}

/// SVDNet with BatchNorm folded into the convolutions and the LSTM in ONNX layout.
pub struct SvdNet {
    blocks: Vec<ConvBlock>,
    /// `[2, 512, 320]`, `[2, 512, 128]`, `[2, 1024]`, gates i, o, f, c.
    w_ih: Tensor<f32>,
    w_hh: Tensor<f32>,
    b: Tensor<f32>,
    head_w: Vec<f32>,
    head_b: f32,
    mu: f32,
    sd: f32,
}

impl std::fmt::Debug for SvdNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvdNet").field("mu", &self.mu).field("sd", &self.sd).finish_non_exhaustive()
    }
}

const HIDDEN: usize = 128;

impl SvdNet {
    /// Reads DJustify's `svdnet.pt` (a PyTorch zip: `{"sd": state dict, "mu": f, "sd_": f}`).
    pub fn from_pt(bytes: &[u8]) -> Result<Self, String> {
        let pt = torch::read(bytes)?;
        let get = |name: &str| pt.tensors.get(name).ok_or_else(|| format!("svdnet.pt: no {name}"));
        let mut blocks = Vec::new();
        for i in 0..4 {
            let w = get(&format!("conv.{i}.0.weight"))?;
            let b = get(&format!("conv.{i}.0.bias"))?;
            let gamma = get(&format!("conv.{i}.1.weight"))?;
            let beta = get(&format!("conv.{i}.1.bias"))?;
            let mean = get(&format!("conv.{i}.1.running_mean"))?;
            let var = get(&format!("conv.{i}.1.running_var"))?;
            let co = w.shape[0];
            let per = w.data.len() / co.max(1);
            let mut weight = w.data.clone();
            let mut bias = vec![0f32; co];
            for c in 0..co {
                let scale = gamma.data[c] / (var.data[c] + 1e-5).sqrt();
                weight[c * per..(c + 1) * per].iter_mut().for_each(|x| *x *= scale);
                bias[c] = (b.data[c] - mean.data[c]) * scale + beta.data[c];
            }
            blocks.push(ConvBlock {
                weight: Tensor::from_data(&w.shape, weight),
                bias: Tensor::from_data(&[co], bias),
            });
        }
        // PyTorch gate order i, f, g, o → ONNX i, o, f, c.
        let reorder = |m: &[f32], cols: usize| -> Vec<f32> {
            let block = HIDDEN * cols;
            [0, 3, 1, 2].iter().flat_map(|&g| m[g * block..(g + 1) * block].iter().copied()).collect()
        };
        let (mut w_ih, mut w_hh, mut b) = (Vec::new(), Vec::new(), Vec::new());
        let mut input = 0;
        for suffix in ["l0", "l0_reverse"] {
            let wi = get(&format!("rnn.weight_ih_{suffix}"))?;
            let wh = get(&format!("rnn.weight_hh_{suffix}"))?;
            input = wi.shape[1];
            w_ih.extend(reorder(&wi.data, input));
            w_hh.extend(reorder(&wh.data, HIDDEN));
            b.extend(reorder(&get(&format!("rnn.bias_ih_{suffix}"))?.data, 1));
            b.extend(reorder(&get(&format!("rnn.bias_hh_{suffix}"))?.data, 1));
        }
        let head_w = get("head.weight")?.data.clone();
        let head_b = *get("head.bias")?.data.first().ok_or("svdnet.pt: empty head.bias")?;
        if head_w.len() != 2 * HIDDEN || input != 64 * (N_MELS / 16) {
            return Err("svdnet.pt: unexpected layer sizes".into());
        }
        let (mu, sd) = (pt.floats.get("mu").copied(), pt.floats.get("sd_").copied());
        let (Some(mu), Some(sd)) = (mu, sd) else { return Err("svdnet.pt: no normalisation".into()) };
        Ok(SvdNet {
            blocks,
            w_ih: Tensor::from_data(&[2, 4 * HIDDEN, input], w_ih),
            w_hh: Tensor::from_data(&[2, 4 * HIDDEN, HIDDEN], w_hh),
            b: Tensor::from_data(&[2, 8 * HIDDEN], b),
            head_w,
            head_b,
            mu: mu as f32,
            sd: sd as f32,
        })
    }

    /// Singing probability per frame (at [`FPS`]) of a 22050 Hz mono signal.
    pub fn probabilities(&self, mono: &[f32]) -> Result<Vec<f32>, String> {
        let (mut mel, frames) = logmel(mono);
        mel.iter_mut().for_each(|x| *x = (*x - self.mu) / self.sd);
        let pool = BufferPool::new();
        // Conv stack in time chunks with CONTEXT frames either side; exact, as nothing pools over time.
        let feat = 64 * (N_MELS / 16);
        let mut seq = vec![0f32; frames * feat];
        let mut s = 0;
        while s < frames {
            let e = (s + CHUNK).min(frames);
            let (a, b) = (s.saturating_sub(CONTEXT), (e + CONTEXT).min(frames));
            let w = b - a;
            let mut x: Vec<f32> = Vec::with_capacity(N_MELS * w);
            for m in 0..N_MELS {
                x.extend_from_slice(&mel[m * frames + a..m * frames + b]);
            }
            let mut h = Tensor::from_data(&[1, 1, N_MELS, w], x);
            let mut height = N_MELS;
            for block in &self.blocks {
                let y = conv(
                    &pool,
                    h.view(),
                    block.weight.view(),
                    Some(block.bias.view()),
                    Padding::from([1, 1, 1, 1]),
                    1,
                    &[1, 1],
                    &[1, 1],
                )
                .map_err(|e| format!("singing-voice detector: {e}"))?;
                let ch = y.size(1);
                let y = y.to_vec();
                // ReLU, then max-pool 2×1 over frequency.
                let half = height / 2;
                let mut p = vec![0f32; ch * half * w];
                for c in 0..ch {
                    for f in 0..half {
                        let (r0, r1) = ((c * height + 2 * f) * w, (c * height + 2 * f + 1) * w);
                        let out = &mut p[(c * half + f) * w..(c * half + f + 1) * w];
                        for t in 0..w {
                            out[t] = y[r0 + t].max(y[r1 + t]).max(0.0);
                        }
                    }
                }
                height = half;
                h = Tensor::from_data(&[1, ch, height, w], p);
            }
            // [1, ch, f, w] → seq[t][ch * f], frames s..e.
            let hd = h.to_vec();
            let plane = height * w;
            for t in s..e {
                let tw = t - a;
                for (k, slot) in seq[t * feat..(t + 1) * feat].iter_mut().enumerate() {
                    let (c, f) = (k / height, k % height);
                    *slot = hd[c * plane + f * w + tw];
                }
            }
            s = e;
        }
        let input = Tensor::from_data(&[frames, 1, feat], seq);
        let out = lstm(
            &pool,
            Direction::Bidirectional,
            input.view(),
            self.w_ih.view(),
            self.w_hh.view(),
            Some(self.b.view()),
            None,
            None,
        )
        .map_err(|e| format!("singing-voice detector: {e}"))?;
        let y: NdTensor<f32, 4> = out
            .into_iter()
            .next()
            .ok_or("singing-voice detector: no LSTM output")?
            .try_into()
            .map_err(|_| "singing-voice detector: LSTM output shape".to_string())?;
        let (fw, bw) = self.head_w.split_at(HIDDEN);
        Ok((0..frames)
            .map(|t| {
                let dot = |d: usize, w: &[f32]| (0..HIDDEN).map(|i| y[[t, d, 0, i]] * w[i]).sum::<f32>();
                let logit = self.head_b + dot(0, fw) + dot(1, bw);
                1.0 / (1.0 + (-logit).exp())
            })
            .collect())
    }
}

/// Just enough of PyTorch's zip + pickle format for a state dict of float tensors.
mod torch {
    use std::collections::HashMap;

    pub struct Tensor {
        pub shape: Vec<usize>,
        pub data: Vec<f32>,
    }

    pub struct Pt {
        pub tensors: HashMap<String, Tensor>,
        pub floats: HashMap<String, f64>,
    }

    /// The stored (uncompressed) entries of a zip file, by name.
    fn entries(bytes: &[u8]) -> Result<HashMap<String, &[u8]>, String> {
        let u16_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let u32_at =
            |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let bad = || "svdnet.pt: not a zip file".to_string();
        let eocd = (0..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
            .ok_or_else(bad)?;
        let count = u16_at(eocd + 10).ok_or_else(bad)?;
        let mut at = u32_at(eocd + 16).ok_or_else(bad)?;
        let mut out = HashMap::new();
        for _ in 0..count {
            if u32_at(at) != Some(0x0201_4b50) {
                return Err(bad());
            }
            let method = u16_at(at + 10).ok_or_else(bad)?;
            let size = u32_at(at + 24).ok_or_else(bad)?;
            let (n, x, c) = (
                u16_at(at + 28).ok_or_else(bad)?,
                u16_at(at + 30).ok_or_else(bad)?,
                u16_at(at + 32).ok_or_else(bad)?,
            );
            let local = u32_at(at + 42).ok_or_else(bad)?;
            let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + n).ok_or_else(bad)?).into_owned();
            at += 46 + n + x + c;
            if method != 0 {
                return Err(format!("svdnet.pt: {name} is compressed"));
            }
            let (ln, lx) = (u16_at(local + 26).ok_or_else(bad)?, u16_at(local + 28).ok_or_else(bad)?);
            let start = local + 30 + ln + lx;
            out.insert(name, bytes.get(start..start + size).ok_or_else(bad)?);
        }
        Ok(out)
    }

    #[derive(Clone, Debug)]
    enum V {
        Mark,
        None,
        Bool,
        Int(i64),
        Float(f64),
        Str(String),
        Global(String),
        Tuple(Vec<V>),
        Dict(Vec<(V, V)>),
        /// A storage: zip entry key and whether it holds f32.
        Storage(String, bool),
        Tensor {
            key: String,
            float: bool,
            offset: usize,
            shape: Vec<usize>,
            stride: Vec<usize>,
        },
    }

    fn usize_of(v: &V) -> Result<usize, String> {
        match v {
            V::Int(i) if *i >= 0 => Ok(*i as usize),
            _ => Err("svdnet.pt: expected an index".into()),
        }
    }

    /// Runs the pickle in `data` (protocol 2, the opcodes `torch.save` emits for a state dict).
    fn unpickle(data: &[u8]) -> Result<V, String> {
        let bad = |what: &str| format!("svdnet.pt: {what}");
        let mut stack: Vec<V> = Vec::new();
        let mut memo: HashMap<usize, V> = HashMap::new();
        let mut i = 0;
        let take = |i: &mut usize, n: usize| -> Result<&[u8], String> {
            let s = data.get(*i..*i + n).ok_or_else(|| bad("truncated pickle"))?;
            *i += n;
            Ok(s)
        };
        let pop_mark = |stack: &mut Vec<V>| -> Result<Vec<V>, String> {
            let m = stack.iter().rposition(|v| matches!(v, V::Mark)).ok_or_else(|| bad("no mark"))?;
            let items = stack.split_off(m + 1);
            stack.pop();
            Ok(items)
        };
        let pop = |stack: &mut Vec<V>| stack.pop().ok_or_else(|| bad("stack underflow"));
        loop {
            let op = *data.get(i).ok_or_else(|| bad("truncated pickle"))?;
            i += 1;
            match op {
                0x80 => i += 1, // PROTO
                b'.' => return pop(&mut stack),
                b'}' => stack.push(V::Dict(Vec::new())),
                b')' => stack.push(V::Tuple(Vec::new())),
                b'(' => stack.push(V::Mark),
                b'N' => stack.push(V::None),
                0x88 | 0x89 => stack.push(V::Bool),
                b'q' => {
                    let k = take(&mut i, 1)?[0] as usize;
                    memo.insert(k, stack.last().cloned().ok_or_else(|| bad("empty stack"))?);
                }
                b'r' => {
                    let b = take(&mut i, 4)?;
                    let k = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                    memo.insert(k, stack.last().cloned().ok_or_else(|| bad("empty stack"))?);
                }
                b'h' => {
                    let k = take(&mut i, 1)?[0] as usize;
                    stack.push(memo.get(&k).cloned().ok_or_else(|| bad("memo"))?);
                }
                b'j' => {
                    let b = take(&mut i, 4)?;
                    let k = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                    stack.push(memo.get(&k).cloned().ok_or_else(|| bad("memo"))?);
                }
                b'K' => stack.push(V::Int(take(&mut i, 1)?[0] as i64)),
                b'M' => {
                    let b = take(&mut i, 2)?;
                    stack.push(V::Int(u16::from_le_bytes([b[0], b[1]]) as i64));
                }
                b'J' => {
                    let b = take(&mut i, 4)?;
                    stack.push(V::Int(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64));
                }
                b'G' => {
                    let b: [u8; 8] = take(&mut i, 8)?.try_into().map_err(|_| bad("float"))?;
                    stack.push(V::Float(f64::from_be_bytes(b)));
                }
                b'X' => {
                    let b = take(&mut i, 4)?;
                    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                    stack.push(V::Str(String::from_utf8_lossy(take(&mut i, n)?).into_owned()));
                }
                b'c' => {
                    let line = |i: &mut usize| -> Result<String, String> {
                        let end = data[*i..].iter().position(|&c| c == b'\n').ok_or_else(|| bad("global"))?;
                        let s = String::from_utf8_lossy(&data[*i..*i + end]).into_owned();
                        *i += end + 1;
                        Ok(s)
                    };
                    let (module, name) = (line(&mut i)?, line(&mut i)?);
                    stack.push(V::Global(format!("{module}.{name}")));
                }
                b't' => {
                    let items = pop_mark(&mut stack)?;
                    stack.push(V::Tuple(items));
                }
                0x85..=0x87 => {
                    let n = (op - 0x84) as usize;
                    if stack.len() < n {
                        return Err(bad("stack underflow"));
                    }
                    let items = stack.split_off(stack.len() - n);
                    stack.push(V::Tuple(items));
                }
                b'Q' => {
                    let V::Tuple(pid) = pop(&mut stack)? else { return Err(bad("persistent id")) };
                    let (Some(V::Global(kind)), Some(V::Str(key))) = (pid.get(1), pid.get(2)) else {
                        return Err(bad("persistent id"));
                    };
                    stack.push(V::Storage(key.clone(), kind == "torch.FloatStorage"));
                }
                b'R' => {
                    let V::Tuple(args) = pop(&mut stack)? else { return Err(bad("reduce args")) };
                    let V::Global(f) = pop(&mut stack)? else { return Err(bad("reduce callable")) };
                    match f.as_str() {
                        "collections.OrderedDict" => stack.push(V::Dict(Vec::new())),
                        "torch._utils._rebuild_tensor_v2" => {
                            let Some(V::Storage(key, float)) = args.first() else {
                                return Err(bad("tensor storage"));
                            };
                            let tuple = |v: Option<&V>| match v {
                                Some(V::Tuple(t)) => t.iter().map(usize_of).collect::<Result<Vec<_>, _>>(),
                                _ => Err(bad("tensor shape")),
                            };
                            stack.push(V::Tensor {
                                key: key.clone(),
                                float: *float,
                                offset: usize_of(args.get(1).ok_or_else(|| bad("tensor offset"))?)?,
                                shape: tuple(args.get(2))?,
                                stride: tuple(args.get(3))?,
                            });
                        }
                        other => return Err(bad(&format!("unexpected {other}"))),
                    }
                }
                b'b' => {
                    pop(&mut stack)?; // BUILD: the state (OrderedDict metadata) isn't needed
                }
                b's' => {
                    let (v, k) = (pop(&mut stack)?, pop(&mut stack)?);
                    let Some(V::Dict(d)) = stack.last_mut() else { return Err(bad("setitem")) };
                    d.push((k, v));
                }
                b'u' => {
                    let items = pop_mark(&mut stack)?;
                    let Some(V::Dict(d)) = stack.last_mut() else { return Err(bad("setitems")) };
                    let mut it = items.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        d.push((k, v));
                    }
                }
                other => return Err(bad(&format!("pickle opcode {other:#x}"))),
            }
        }
    }

    pub fn read(bytes: &[u8]) -> Result<Pt, String> {
        let files = entries(bytes)?;
        let (pkl_name, pkl) =
            files.iter().find(|(n, _)| n.ends_with("/data.pkl")).ok_or("svdnet.pt: no data.pkl")?;
        let prefix = pkl_name.trim_end_matches("data.pkl").to_string();
        let V::Dict(top) = unpickle(pkl)? else { return Err("svdnet.pt: not a dict".into()) };
        let mut pt = Pt { tensors: HashMap::new(), floats: HashMap::new() };
        for (k, v) in top {
            let V::Str(k) = k else { continue };
            match v {
                V::Float(f) => {
                    pt.floats.insert(k, f);
                }
                V::Dict(state) => {
                    for (name, t) in state {
                        let (V::Str(name), V::Tensor { key, float: true, offset, shape, stride }) = (name, t)
                        else {
                            continue;
                        };
                        let raw =
                            files.get(&format!("{prefix}data/{key}")).ok_or("svdnet.pt: missing storage")?;
                        let value = |i: usize| -> Option<f32> {
                            raw.get(4 * i..4 * i + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        };
                        let n: usize = shape.iter().product();
                        let mut data = Vec::with_capacity(n);
                        for flat in 0..n {
                            let (mut rest, mut at) = (flat, offset);
                            for d in (0..shape.len()).rev() {
                                at += (rest % shape[d]) * stride[d];
                                rest /= shape[d];
                            }
                            data.push(value(at).ok_or("svdnet.pt: storage too short")?);
                        }
                        pt.tensors.insert(name, Tensor { shape, data });
                    }
                }
                _ => {}
            }
        }
        Ok(pt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mel_filters_match_librosa() {
        // librosa.filters.mel(sr=22050, n_fft=1024, n_mels=80, fmin=27.5, fmax=8000): sums of rows 0, 40, 79.
        let f = mel_filters();
        let sum = |i: usize| f[i].iter().sum::<f32>();
        assert_eq!(f.len(), N_MELS);
        assert_eq!(f[0].len(), N_FFT / 2 + 1);
        assert!(sum(0) > 0.0 && sum(40) > 0.0 && sum(79) > 0.0);
    }

    #[test]
    fn short_blips_are_dropped_and_breaths_bridged() {
        let fps = FPS as usize;
        let mut p = vec![0.0f32; 10 * fps];
        // A 2 s line, a 0.2 s breath, another 2 s line, then a 0.2 s blip.
        p[fps..3 * fps].iter_mut().for_each(|x| *x = 0.9);
        p[3 * fps + fps / 5..5 * fps].iter_mut().for_each(|x| *x = 0.9);
        p[7 * fps..7 * fps + fps / 5].iter_mut().for_each(|x| *x = 0.9);
        let segs = voiced_segments(&p);
        assert_eq!(segs.len(), 1, "{segs:?}");
        assert!((segs[0].0 - 1.0).abs() < 0.05 && (segs[0].1 - 5.0).abs() < 0.05, "{segs:?}");
    }

    #[test]
    fn bar_fractions_cover_each_bar() {
        let f = bar_fractions(&[(1.0, 3.0)], &[0.0, 2.0, 4.0, 6.0]);
        assert_eq!(f, [0.5, 0.5, 0.0]);
    }

    #[test]
    fn a_bar_sung_for_a_quarter_counts_as_vocal() {
        let v = bar_vocal(&[(0.0, 2.0), (2.0, 2.5), (4.0, 4.4)], &[0.0, 2.0, 4.0, 6.0]);
        assert!((v[0] - 1.0).abs() < 1e-6, "{v:?}");
        assert!((v[1] - 0.5).abs() < 1e-6, "{v:?}");
        assert!(v[2] < 0.5 && v[2] > 0.0, "{v:?}");
        assert_eq!(crate::structure::instrumental_edges(&v), (0, 1));
    }

    #[test]
    fn missing_model_is_not_found() {
        assert!(VoiceModel::in_dir(Path::new("/nonexistent")).is_none());
        assert!(SvdNet::from_pt(b"not a zip").is_err());
    }

    /// Against the PyTorch reference (DJustify's infer.py) where `LISTEN_SVD_REF` points at a folder with
    /// `svdnet.pt` and `<name>.f32` + `<name>.ref.json` pairs.
    #[test]
    fn matches_the_pytorch_reference() {
        let Some(dir) = std::env::var_os("LISTEN_SVD_REF").map(PathBuf::from) else { return };
        let net = SvdNet::from_pt(&std::fs::read(dir.join("svdnet.pt")).unwrap()).unwrap();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "f32") {
                continue;
            }
            let raw = std::fs::read(&path).unwrap();
            let mono: Vec<f32> = raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
            let r: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.with_extension("ref.json")).unwrap()).unwrap();
            let (mel, frames) = logmel(&mono);
            assert_eq!(frames as u64, r["frames"].as_u64().unwrap());
            let mel_mean = mel.iter().map(|&x| f64::from(x)).sum::<f64>() / mel.len() as f64;
            assert!((mel_mean - r["mel_mean"].as_f64().unwrap()).abs() < 1e-3, "{mel_mean}");
            let t0 = std::time::Instant::now();
            let p = net.probabilities(&mono).unwrap();
            let took = t0.elapsed();
            let want: Vec<f32> =
                r["prob"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
            let worst = p.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            let segs = voiced_segments(&p);
            let want_segs = r["segments"].as_array().unwrap().len();
            eprintln!(
                "{}: {frames} frames in {took:?}, max |Δp| {worst}, {} spans (ref {want_segs})",
                path.display(),
                segs.len()
            );
            assert!(worst < 0.01, "max |Δp| {worst}");
            assert_eq!(segs.len(), want_segs);
        }
    }

    /// The analyser with the detector (same folder as above): the vocal curve and the instrumental edges
    /// come from the sung stretches.
    #[test]
    fn the_analysis_uses_the_detector() {
        let Some(dir) = std::env::var_os("LISTEN_SVD_REF").map(PathBuf::from) else { return };
        let Some(path) = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "f32"))
        else {
            return;
        };
        let raw = std::fs::read(&path).unwrap();
        let mono: Vec<f32> = raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
        let pcm = crate::Pcm { sample_rate: SR as u32, left: mono.clone(), right: mono };
        let model = VoiceModel::at(dir.join("svdnet.pt"));
        let plain = crate::analyze(&pcm, &crate::Opts::default());
        let with = crate::analyze(&pcm, &crate::Opts { voice: Some(model), ..Default::default() });
        assert!(plain.voiced.is_none());
        let spans = with.voiced.as_ref().expect("sung stretches");
        assert!(!spans.is_empty());
        assert_eq!(with.bars.vocal.len(), plain.bars.vocal.len());
        let first = spans[0].0;
        let intro_end = with.grid.as_ref().map(|g| g.bar_time(usize::from(with.intro_instrumental_bars)));
        eprintln!(
            "first sung {first:.1} s, intro {} bars (ends {intro_end:?}) vs heuristic {}",
            with.intro_instrumental_bars, plain.intro_instrumental_bars
        );
        // The intro ends at most a bar after the first sung stretch starts.
        if let (Some(end), Some(g)) = (intro_end, with.grid.as_ref()) {
            assert!(end <= first + 60.0 / g.bpm * f32::from(g.beats_per_bar) + 0.1, "{end} vs {first}");
        }
    }

    /// The pinned download (only with `LISTEN_SVD_DOWNLOAD=1`: it fetches 2 MB from GitHub).
    #[tokio::test]
    async fn downloads_the_pinned_weights() {
        if std::env::var_os("LISTEN_SVD_DOWNLOAD").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("listen-svd-{}", std::process::id()));
        let model = VoiceModel::download(&dir).await.unwrap();
        assert_eq!(VoiceModel::in_dir(&dir), Some(model));
        VoiceModel::remove(&dir).unwrap();
        assert!(VoiceModel::in_dir(&dir).is_none());
        let _ = std::fs::remove_dir(&dir);
    }
}
