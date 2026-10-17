//! Visualizer backend.
//!
//! * [`Analyzer`]: FFT → log-spaced dB bands with smoothing and falling peaks
//!   (audioMotion-analyzer defaults: 20 Hz–22 kHz, −85…−25 dB, smoothing 0.6).
//! * [`capture`]: Listen's own playback streams only (Linux: `pw-record` on mpv's node,
//!   pre-volume), one recorder and watchdog per deck.

mod analyzer;
pub use analyzer::*;

pub mod capture {
    //! Audio capture of Listen's own output. Each deck has its own `pw-record` node
    //! ([`recorder_name`]) whose watchdog accepts links only from that deck's stream.
    #[cfg(target_os = "linux")]
    use tokio::io::AsyncReadExt;
    #[cfg(target_os = "linux")]
    use tokio::process::Command;
    use tokio::sync::mpsc;

    /// `node.name` of the recorder for deck `index`: `listen-visualizer`, `listen-visualizer-b`, …
    pub fn recorder_name(index: usize) -> String {
        match index {
            0 => "listen-visualizer".into(),
            1..26 => format!("listen-visualizer-{}", char::from(b'a' + index as u8)),
            _ => format!("listen-visualizer-{index}"),
        }
    }

    /// A PipeWire node: `id` (used by links) and `object.serial` (used by `--target`).
    #[cfg(target_os = "linux")]
    #[derive(Debug, Clone, PartialEq)]
    pub struct Node {
        pub id: u64,
        pub serial: String,
    }

    /// Watchdog check interval for a recorder's links before and after they are confirmed.
    #[cfg(target_os = "linux")]
    const CHECK_UNVERIFIED: std::time::Duration = std::time::Duration::from_millis(500);
    #[cfg(target_os = "linux")]
    const CHECK_VERIFIED: std::time::Duration = std::time::Duration::from_secs(3);
    /// Longest pause between looks for a deck's stream that doesn't exist yet.
    #[cfg(target_os = "linux")]
    const WAIT_MAX: std::time::Duration = std::time::Duration::from_secs(4);

    #[cfg(target_os = "linux")]
    async fn dump() -> Option<Vec<serde_json::Value>> {
        let out = Command::new("pw-dump").output().await.ok()?;
        serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()?.as_array().cloned()
    }

    /// The `Stream/Output/Audio` node whose `node.name` is exactly `name` (mpv's
    /// `--audio-client-name`); `None` while it doesn't exist.
    #[cfg(target_os = "linux")]
    pub fn find_stream_in(objects: &[serde_json::Value], name: &str) -> Option<Node> {
        objects.iter().find_map(|o| {
            let props = &o["info"]["props"];
            (props["media.class"] == "Stream/Output/Audio" && props["node.name"] == name).then(|| Node {
                id: o["id"].as_u64().unwrap_or(0),
                serial: props["object.serial"].to_string().trim_matches('"').to_string(),
            })
        })
    }

    #[cfg(target_os = "linux")]
    pub async fn find_stream(name: &str) -> Option<Node> {
        find_stream_in(&dump().await?, name)
    }

    /// Output node ids of all links into any node named `recorder`.
    #[cfg(target_os = "linux")]
    pub fn recorder_sources(objects: &[serde_json::Value], recorder: &str) -> Vec<u64> {
        let recorders: Vec<u64> = objects
            .iter()
            .filter(|o| o["info"]["props"]["node.name"] == recorder)
            .filter_map(|o| o["id"].as_u64())
            .collect();
        objects
            .iter()
            .filter(|o| o["type"] == "PipeWire:Interface:Link")
            .filter(|o| o["info"]["input-node-id"].as_u64().is_some_and(|i| recorders.contains(&i)))
            .filter_map(|o| o["info"]["output-node-id"].as_u64())
            .collect()
    }

    /// Is it still safe for `recorder` to record? Only while the stream `name`
    /// exists with the same id and the recorder is connected to nothing else.
    #[cfg(target_os = "linux")]
    pub fn safe(objects: &[serde_json::Value], name: &str, recorder: &str, target: &Node) -> bool {
        find_stream_in(objects, name).is_some_and(|n| n.id == target.id)
            && recorder_sources(objects, recorder).iter().all(|&src| src == target.id)
    }

    /// Streams interleaved f32 stereo from the playback stream `name` only. The watchdog stops
    /// the recorder when the stream goes or anything else is linked (PipeWire would fall back
    /// to the microphone). Returns when `tx` is closed.
    #[cfg(target_os = "linux")]
    pub async fn pipewire(name: &str, tx: mpsc::Sender<Vec<f32>>) -> Result<(), String> {
        capture(name, &recorder_name(0), &tx, |samples| samples).await
    }

    /// [`pipewire`] for several decks (e.g. `["Listen", "Listen B"]`), each independent;
    /// chunks are tagged with the deck index. Returns when `tx` is closed or on the first error.
    #[cfg(target_os = "linux")]
    pub async fn pipewire_many(names: &[&str], tx: mpsc::Sender<(usize, Vec<f32>)>) -> Result<(), String> {
        let mut decks = tokio::task::JoinSet::new();
        for (i, name) in names.iter().enumerate() {
            let (name, tx) = (name.to_string(), tx.clone());
            decks.spawn(async move { capture(&name, &recorder_name(i), &tx, |samples| (i, samples)).await });
        }
        drop(tx);
        // Dropping the set on an error aborts the other decks, which kills
        // their recorders (`kill_on_drop`).
        while let Some(done) = decks.join_next().await {
            done.map_err(|e| e.to_string())??;
        }
        Ok(())
    }

    /// Waits for the stream `name`, records it, and repeats when it comes back.
    #[cfg(target_os = "linux")]
    async fn capture<T>(
        name: &str,
        recorder: &str,
        tx: &mpsc::Sender<T>,
        wrap: impl Fn(Vec<f32>) -> T,
    ) -> Result<(), String> {
        loop {
            // Back off from 1 s: every look runs `pw-dump`, and deck B may idle for minutes.
            let mut wait = std::time::Duration::from_secs(1);
            let target = loop {
                if tx.is_closed() {
                    return Ok(());
                }
                if let Some(node) = find_stream(name).await {
                    break node;
                }
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(WAIT_MAX);
            };
            record(name, recorder, &target, tx, &wrap).await?;
            if tx.is_closed() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    /// One recording of `target`, until it disappears or the watchdog trips.
    #[cfg(target_os = "linux")]
    async fn record<T>(
        name: &str,
        recorder: &str,
        target: &Node,
        tx: &mpsc::Sender<T>,
        wrap: &impl Fn(Vec<f32>) -> T,
    ) -> Result<(), String> {
        let props =
            format!("{{ node.name = {recorder} node.dont-reconnect = true node.autoconnect = true }}");
        let mut child = Command::new("pw-record")
            // s16: pw-record's raw f32 output doesn't decode reliably; 16 bits is plenty here.
            .args(["--target", &target.serial, "--format", "s16", "--rate", "48000", "--channels", "2"])
            .args(["--latency", "20ms", "-P", &props, "-"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("pw-record not available ({e}); install pipewire-utils"))?;
        let mut out = child.stdout.take().ok_or("no stdout")?;
        let mut buf = vec![0u8; 4096 * 4];
        let mut carry: Vec<u8> = Vec::new();
        let mut check = tokio::time::interval(CHECK_UNVERIFIED);
        // Samples are dropped until the links are confirmed to come from this deck's stream only.
        let mut verified = false;
        loop {
            tokio::select! {
                read = out.read(&mut buf) => {
                    let n = read.map_err(|e| e.to_string())?;
                    if n == 0 {
                        return Ok(());
                    }
                    carry.extend_from_slice(&buf[..n]);
                    let usable = carry.len() / 4 * 4; // whole stereo s16 frames
                    let samples: Vec<f32> = carry[..usable]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&b| f32::from(i16::from_le_bytes(b)) / 32768.0)
                        .collect();
                    carry.drain(..usable);
                    if !verified {
                        continue;
                    }
                    if tx.send(wrap(samples)).await.is_err() {
                        return Ok(());
                    }
                }
                _ = check.tick() => {
                    if tx.is_closed() {
                        // Nobody listens any more (and while paused no sample would tell).
                        let _ = child.kill().await;
                        return Ok(());
                    }
                    let ok = match dump().await {
                        Some(objects) => {
                            let ok = safe(&objects, name, recorder, target);
                            if ok && !verified && !recorder_sources(&objects, recorder).is_empty() {
                                verified = true;
                                // `node.dont-reconnect` keeps the links put; check less often.
                                check = tokio::time::interval_at(
                                    tokio::time::Instant::now() + CHECK_VERIFIED,
                                    CHECK_VERIFIED,
                                );
                            }
                            ok
                        }
                        None => false,
                    };
                    if !ok {
                        tracing::info!("visualizer: audio stream {name:?} is gone; stopped recording");
                        let _ = child.kill().await;
                        return Ok(());
                    }
                }
            }
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    pub async fn pipewire(_target: &str, _tx: mpsc::Sender<Vec<f32>>) -> Result<(), String> {
        Err("audio capture is not implemented on this platform yet".into())
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    pub async fn pipewire_many(_names: &[&str], _tx: mpsc::Sender<(usize, Vec<f32>)>) -> Result<(), String> {
        Err("audio capture is not implemented on this platform yet".into())
    }

    /// Windows: WASAPI loopback of the default *output* device (never the microphone).
    /// All decks share the output, so everything arrives as deck 0.
    #[cfg(windows)]
    pub async fn pipewire(_name: &str, tx: mpsc::Sender<Vec<f32>>) -> Result<(), String> {
        let (wrapped, mut rx) = mpsc::channel::<(usize, Vec<f32>)>(64);
        let forward = async move {
            while let Some((_, samples)) = rx.recv().await {
                if tx.send(samples).await.is_err() {
                    break;
                }
            }
        };
        let (result, ()) = tokio::join!(pipewire_many(&[], wrapped), forward);
        result
    }

    #[cfg(windows)]
    pub async fn pipewire_many(_names: &[&str], tx: mpsc::Sender<(usize, Vec<f32>)>) -> Result<(), String> {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Stop(Arc<AtomicBool>);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let _guard = Stop(stop.clone());
        let (first, first_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("loopback".into())
            .spawn(move || loopback_thread(tx, stop, first))
            .map_err(|e| e.to_string())?;
        // Resolves once with the first attempt's outcome; then runs until dropped.
        first_rx.await.map_err(|_| "audio capture stopped".to_string())??;
        std::future::pending::<()>().await;
        Ok(())
    }

    #[cfg(windows)]
    fn loopback_thread(
        tx: mpsc::Sender<(usize, Vec<f32>)>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        first: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
        use std::sync::atomic::Ordering;
        let mut first = Some(first);
        while !stop.load(Ordering::Relaxed) && !tx.is_closed() {
            let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            match open_loopback(&tx, &broken) {
                Ok(_stream) => {
                    if let Some(f) = first.take() {
                        let _ = f.send(Ok(()));
                    }
                    // Rebuild when the stream fails (e.g. the default device changed).
                    while !stop.load(Ordering::Relaxed) && !tx.is_closed() && !broken.load(Ordering::Relaxed)
                    {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                }
                Err(e) => {
                    tracing::info!("visualizer: no loopback capture: {e}");
                    if let Some(f) = first.take() {
                        let _ = f.send(Err(e));
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    }

    #[cfg(windows)]
    fn open_loopback(
        tx: &mpsc::Sender<(usize, Vec<f32>)>,
        broken: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<cpal::Stream, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!("unsupported output format {:?}", config.sample_format()));
        }
        let (channels, rate) = (config.channels() as usize, config.sample_rate());
        let mut resampler = crate::convert::Resampler::new(rate);
        let (data_tx, flag) = (tx.clone(), broken.clone());
        let stream = device
            .build_input_stream(
                &config.config(),
                move |data: &[f32], _| {
                    let out = resampler.push(data, channels);
                    if !out.is_empty() {
                        let _ = data_tx.try_send((0, out));
                    }
                },
                move |e| {
                    tracing::info!("visualizer: loopback error: {e}");
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(stream)
    }
}

/// Interleaved device samples to 48 kHz interleaved stereo.
#[cfg_attr(not(windows), allow(dead_code))]
mod convert {
    use super::SAMPLE_RATE;

    pub struct Resampler {
        step: f64,
        /// Position of the next output sample, in input frames relative to `prev`.
        pos: f64,
        prev: [f32; 2],
    }

    impl Resampler {
        pub fn new(input_rate: u32) -> Self {
            Self { step: f64::from(input_rate) / f64::from(SAMPLE_RATE), pos: 0.0, prev: [0.0; 2] }
        }

        /// Stereo frames of `data` (`channels` interleaved; mono is doubled, extra channels dropped).
        pub fn push(&mut self, data: &[f32], channels: usize) -> Vec<f32> {
            let channels = channels.max(1);
            let frames: Vec<[f32; 2]> = data
                .chunks_exact(channels)
                .map(|f| if channels == 1 { [f[0], f[0]] } else { [f[0], f[1]] })
                .collect();
            let mut out = Vec::with_capacity((frames.len() as f64 / self.step) as usize * 2 + 2);
            // Frame index -1 is `prev`.
            let at = |i: i64, prev: [f32; 2]| if i < 0 { prev } else { frames[i as usize] };
            while self.pos < frames.len() as f64 - 1.0 {
                let base = self.pos.floor();
                let frac = (self.pos - base) as f32;
                let (a, b) = (at(base as i64, self.prev), at(base as i64 + 1, self.prev));
                out.push(a[0] + (b[0] - a[0]) * frac);
                out.push(a[1] + (b[1] - a[1]) * frac);
                self.pos += self.step;
            }
            if let Some(last) = frames.last() {
                self.prev = *last;
                self.pos -= frames.len() as f64;
            }
            out
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn same_rate_is_passthrough_and_mono_is_doubled() {
            let mut r = Resampler::new(48_000);
            let out = r.push(&[1.0, 2.0, 3.0, 4.0], 2);
            assert_eq!(out, [1.0, 2.0], "the last frame waits for the next chunk");
            assert_eq!(r.push(&[5.0, 6.0], 2), [3.0, 4.0]);
            let mut m = Resampler::new(48_000);
            assert_eq!(m.push(&[0.5, 0.25], 1), [0.5, 0.5]);
        }

        #[test]
        fn rate_conversion_keeps_the_duration_across_chunks() {
            let mut r = Resampler::new(44_100);
            let total: usize = (0..100).map(|_| r.push(&vec![0.1; 441 * 2], 2).len() / 2).sum();
            // 100 x 10 ms at 44.1 kHz is about 1 s at 48 kHz.
            assert!((47_900..=48_100).contains(&total), "{total}");
            let mut down = Resampler::new(96_000);
            let n: usize = (0..100).map(|_| down.push(&vec![0.1; 960 * 2], 2).len() / 2).sum();
            assert!((47_900..=48_100).contains(&n), "{n}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(target_os = "linux")]
    const A: &str = "listen-visualizer";
    #[cfg(target_os = "linux")]
    const B: &str = "listen-visualizer-b";

    #[cfg(target_os = "linux")]
    fn pw(links: &[(u64, u64)], with_mpv: bool) -> Vec<serde_json::Value> {
        use serde_json::json;
        let mut v = vec![
            json!({"id": 50, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Source", "node.name": "alsa_input.mic"}}}),
            json!({"id": 90, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Input/Audio", "node.name": "listen-visualizer"}}}),
        ];
        if with_mpv {
            v.push(json!({"id": 70, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "node.name": "Listen", "object.serial": "1234"}}}));
        }
        for (out, input) in links {
            v.push(json!({"id": 100 + out, "type": "PipeWire:Interface:Link", "info": {"output-node-id": out, "input-node-id": input}}));
        }
        v
    }

    /// The watchdog: recording is only allowed from Listen's own stream.
    #[cfg(target_os = "linux")]
    #[test]
    fn never_records_the_microphone() {
        use capture::{Node, find_stream_in, recorder_sources, safe};
        let target = Node { id: 70, serial: "1234".into() };
        assert_eq!(find_stream_in(&pw(&[], true), "Listen"), Some(target.clone()));

        // Linked to mpv: fine.
        assert!(safe(&pw(&[(70, 90)], true), "Listen", A, &target));
        assert_eq!(recorder_sources(&pw(&[(70, 90)], true), A), vec![70]);
        // PipeWire moved the recorder to the microphone: stop.
        assert!(!safe(&pw(&[(50, 90)], true), "Listen", A, &target));
        assert!(!safe(&pw(&[(70, 90), (50, 90)], true), "Listen", A, &target));
        // mpv's stream is gone (stopped / idle): stop, even before any relink.
        assert!(!safe(&pw(&[(70, 90)], false), "Listen", A, &target));
        assert!(!safe(&pw(&[(50, 90)], false), "Listen", A, &target));
        // A new mpv stream with another id is a different target: stop and re-attach.
        assert!(!safe(&pw(&[(70, 90)], true), "Listen", A, &Node { id: 71, serial: "9".into() }));
    }

    /// Two decks plus the AI DJ voice: mic 50, recorders A 90 / B 91, decks
    /// "Listen" 70 / "Listen B" 71, "Listen DJ" 72 (never captured).
    #[cfg(target_os = "linux")]
    fn pw2(links: &[(u64, u64)], deck_a: bool, deck_b: bool) -> Vec<serde_json::Value> {
        use serde_json::json;
        let node = |id: u64, class: &str, name: &str| json!({"id": id, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": class, "node.name": name, "object.serial": (id + 1000).to_string()}}});
        let mut v = vec![
            node(50, "Audio/Source", "alsa_input.mic"),
            node(90, "Stream/Input/Audio", "listen-visualizer"),
            node(91, "Stream/Input/Audio", "listen-visualizer-b"),
            node(72, "Stream/Output/Audio", "Listen DJ"),
        ];
        if deck_a {
            v.push(node(70, "Stream/Output/Audio", "Listen"));
        }
        if deck_b {
            v.push(node(71, "Stream/Output/Audio", "Listen B"));
        }
        for (i, (out, input)) in links.iter().enumerate() {
            v.push(json!({"id": 200 + i, "type": "PipeWire:Interface:Link", "info": {"output-node-id": out, "input-node-id": input}}));
        }
        v
    }

    /// Two decks: each recorder accepts only its own deck, and trouble on one doesn't stop the other.
    #[cfg(target_os = "linux")]
    #[test]
    fn each_deck_recorder_only_records_its_own_deck() {
        use capture::{Node, find_stream_in, recorder_name, recorder_sources, safe};
        let (a, b) = (Node { id: 70, serial: "1070".into() }, Node { id: 71, serial: "1071".into() });
        assert_eq!((recorder_name(0).as_str(), recorder_name(1).as_str()), (A, B));

        // Exact names: "Listen" is neither "Listen B" nor "Listen DJ".
        let all = pw2(&[], true, true);
        assert_eq!(find_stream_in(&all, "Listen"), Some(a.clone()));
        assert_eq!(find_stream_in(&all, "Listen B"), Some(b.clone()));
        assert_eq!(find_stream_in(&pw2(&[], false, true), "Listen"), None);
        assert_eq!(find_stream_in(&pw2(&[], true, false), "Listen B"), None);
        assert_eq!(find_stream_in(&all, "Listen D"), None);
        assert_eq!(find_stream_in(&all, "listen"), None);
        // Recorder names are exact too: B's links never count for A.
        assert_eq!(recorder_sources(&pw2(&[(71, 91)], true, true), A), Vec::<u64>::new());
        assert_eq!(recorder_sources(&pw2(&[(71, 91)], true, true), B), vec![71]);

        // Both decks, each linked to its own recorder: both fine.
        let ok = pw2(&[(70, 90), (71, 91)], true, true);
        assert!(safe(&ok, "Listen", A, &a) && safe(&ok, "Listen B", B, &b));
        // Deck B alone (A idle): B fine, A must not record.
        let only_b = pw2(&[(71, 91)], false, true);
        assert!(safe(&only_b, "Listen B", B, &b));
        assert!(!safe(&only_b, "Listen", A, &a));

        // A microphone linked into recorder B trips B only.
        let mic_b = pw2(&[(70, 90), (71, 91), (50, 91)], true, true);
        assert!(!safe(&mic_b, "Listen B", B, &b));
        assert!(safe(&mic_b, "Listen", A, &a));
        // And into recorder A: A only.
        let mic_a = pw2(&[(70, 90), (50, 90), (71, 91)], true, true);
        assert!(!safe(&mic_a, "Listen", A, &a));
        assert!(safe(&mic_a, "Listen B", B, &b));

        // Cross-linked decks: neither recorder accepts the other deck.
        assert!(!safe(&pw2(&[(71, 90)], true, true), "Listen", A, &a));
        assert!(!safe(&pw2(&[(70, 90), (71, 90)], true, true), "Listen", A, &a));
        assert!(!safe(&pw2(&[(70, 91)], true, true), "Listen B", B, &b));
        assert!(!safe(&pw2(&[(71, 91), (70, 91)], true, true), "Listen B", B, &b));
        // The DJ voice is never accepted by either recorder.
        assert!(!safe(&pw2(&[(72, 90)], true, true), "Listen", A, &a));
        assert!(!safe(&pw2(&[(72, 91)], true, true), "Listen B", B, &b));
        // A recorder never takes the other deck as its target either.
        assert!(!safe(&pw2(&[(71, 90)], true, true), "Listen", A, &b));
        assert!(!safe(&pw2(&[(70, 91)], true, true), "Listen B", B, &a));
    }

    /// Two recorder nodes with the same name: links into either count.
    #[cfg(target_os = "linux")]
    #[test]
    fn duplicate_recorder_nodes_are_all_checked() {
        use capture::{Node, safe};
        let mut v = pw2(&[(70, 90), (50, 92)], true, true);
        v.push(serde_json::json!({"id": 92, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Input/Audio", "node.name": "listen-visualizer"}}}));
        assert!(!safe(&v, "Listen", A, &Node { id: 70, serial: "1070".into() }));
    }
}
