# DJ transitions

Design notes and measurements for beat-matched, phrase-aligned mixing of
Navidrome and YouTube Music songs (Auto-DJ and AI DJ). This file collects
what the prototypes found.

## S0a analyser bench

**Question:** where do beats, tempo, key and the timbre vector come from?
Candidates: `beat-this` 1.0 (neural beats + downbeats, MIT code and weights,
pure-Rust `rten`), `stratum-dsp` 1.0 (tempo, HMM grid, key; MIT/Apache),
an own Ellis-style dynamic-programming tracker, and `bliss-audio` 0.13 for
timbre (GPL-3.0).

**How:** `crates/listen-analysis/examples/bench.rs`, release build, desktop
(Ryzen 7 9800X3D, 16 threads), mpv 0.41.0 decoding to 22050 Hz stereo float.

```sh
cargo run --release -p listen-analysis --example bench -- --n 60        # Navidrome songs
cargo run --release -p listen-analysis --example bench -- --synthetic 40  # ground truth
cargo run --release -p listen-analysis --example inspect -- <file|url> target/analysis-bench/models
```

### What could be measured

The test account (from `.env.test`) only sees the "Game Music"
folder: **37 songs, all one orchestral game soundtrack, none with a BPM
tag.** So the planned "BPM vs tags on K-pop" comparison was impossible.
Instead:

1. **Synthetic ground truth** (`examples/synth`): 40 generated pop songs,
   75–175 BPM, random key and major/minor, kick on 1&3 or four-on-the-floor,
   intro 8 bars (pad + hats) → verse 16 (band + vocal-like melody) →
   breakdown 8 (no kick/bass) → chorus 16 → outro 8 (no vocal). Encoded to
   MP3 by ffmpeg and decoded back by mpv, exactly like a real song.
2. **The 37 real tracks** for timing and agreement between analysers.

The Stage 1 gate (≥ 85 % octave-tolerant agreement with BPM tags) still has
to be measured on a larger tagged library (`--validate-analysis`, or this bench
with an account that sees the K-pop library and its tags).

### Results: synthetic songs (40, MP3 via mpv)

| Analyser | Grid | Tempo exact (±2 %) | Octave-tolerant | Downbeat right | Beat error, median of songs |
|---|---|---|---|---|---|
| beat-this (small model) | 40/40 | **40/40** | 40/40 | **40/40** | 2.0 ms (p95 6.1 ms) |
| own DP tracker | 40/40 | 21/40 (rest at half tempo) | 40/40 | 21/21 | 0.2 ms |
| stratum-dsp | 2/40 (low confidence) | 1/40 | 1/40 | 1/1 | 35 ms, −26 ms bias |

- Beat errors are after a constant offset calibration: raw beat-this beats
  are 7 ms late and the own tracker's 13 ms early (its 93 ms STFT window
  sees the onset before its centre). Both are corrected in code
  (`beats::BEAT_THIS_OFFSET`, `beats::OWN_OFFSET`) so grids from either source
  line up; on the real tracks the two then agree within a median of 2.9 ms.
- **mpv removes the MP3 encoder delay exactly**: decoded audio vs source
  cross-correlates at 0 ms offset for all 40 songs. Decoding with mpv keeps
  the beat grid in the same time base as playback.
- Key (own Temperley-profile detector on peak chroma): 24/40 exact,
  **40/40 exact or relative major/minor** (the misses are minor songs whose
  i–VI–III–VII progression is literally the relative major's chord set;
  Camelot treats relative keys as compatible, distance 0.15).
  stratum-dsp: 19/40 exact.
- Vocal heuristic: 98 % of bars right, instrumental intro/outro length
  (8 bars) right ±1 in 40/40. Sections came out
  `Intro 8 · Body 16 · Breakdown 8 · Drop 24` in 39/40 (the synthetic outro
  keeps the full band, so "Body/Drop" is correct by the rules).

### Results: 37 real tracks (orchestral game soundtrack, 122 s average)

Mean time per track, release build:

| Stage | Wall | CPU (all threads) |
|---|---|---|
| decode (mpv, FLAC over the network) | 2.7 s | mpv ~0.2 s per 2.5 min |
| STFT features (bands, chroma, flux, MFCC) | 0.20 s | 0.20 s |
| **beat-this small** (8 rten threads) | **1.18 s** | **5.8 s** |
| own DP tracker | 3 ms | 3 ms |
| stratum-dsp (single thread) | 0.74 s | 0.73 s |
| bliss-audio vector (from our samples) | 37 ms | 88 ms |
| loudness (EBU R128) | 27 ms | — |
| bars, vocal, novelty, phrases, sections, key | 4 ms | — |
| **full `analyze()`** | **1.46 s** | **6.2 s** |

So a 3.5-minute song costs about 2.5 s wall / 10 s CPU on the desktop: a
10 000-song library is roughly 7 hours of analysis for one niced background
worker, plus download time
(less if both machines crawl; shards are shared). The laptop is not
reachable from here, so its numbers are still open; expect 2–3× the CPU.

Agreement (octave-tolerant / exact): beat-this vs stratum 18/6 of 36,
beat-this vs own 15/10 of 35, own vs stratum 24/15 of 36. On this rubato
orchestral material beat-this rejects 15 of 37 grids (stability < 0.9 or
confidence < 0.35), which is the desired outcome: those songs get plain
fades. The tempo octave is often ambiguous here (72 vs 146 BPM); the
server's `bpm` tag, when present, is used as an octave prior
(`Opts::bpm_hint`).

### Decision

- **Beat source: beat-this (small model).** Perfect tempo and downbeats on
  the synthetic set, ~2 ms beat error, and the only candidate that gives
  downbeats directly. It is the expensive stage (≈ 50 ms CPU per second of
  audio), acceptable for a niced background crawl.
  - Model files (`mel_spectrogram.onnx` 271 KB + `beat_this_small.onnx`
    10.6 MB, MIT, weights by CPJKU/JKU Linz, ONNX conversion by
    beat-this-rs) are **downloaded once on first use** into the cache dir
    from a pinned beat-this-rs commit and SHA-256 checked
    (`BeatModel::ensure`); nothing is fetched at build time and the repo
    stays small. The full 83 MB model was not considered (too slow and big).
  - The **own DP tracker** stays as the fallback (no model yet, model
    error): correct downbeats and timing, but it picks half tempo above
    ~140 BPM about half the time; the BPM tag hint corrects that.
- **stratum-dsp: not used.** Low confidence on almost everything
  synthetic, 25–45 ms beat bias, slower than our whole pipeline minus the
  model, and its Camelot labels are wrong (C major → "1A"). Kept only as a
  dev-dependency of the bench.
- **Timbre: bliss-audio** `Song::analyze(&[f32])` on our own 22050 Hz mono
  samples (no ffmpeg feature needed): 23 dimensions already normalised
  for similarity, ~0.1 s CPU. An own 20-dim MFCC vector was prototyped and
  dropped.
- **Key: own** (Temperley profiles on spectral-peak chroma from 100 Hz,
  confidence = correlation × margin over the best non-relative rival).
  Reworked in analyser v3, see "Key detection and mood calibration".

### Known limits

- No real ground truth for K-pop yet (see above). The vocal heuristic
  (tonal, centre-panned 500–3400 Hz energy with abrupt 3–8 Hz syllable
  modulation) is tuned on synthetic songs only; on real mixes expect
  false "vocal" on centred synth leads, which errs on the safe side (fewer
  long blends).
- Tempo changes inside a song: the grid follows them (local ±8-beat fits),
  but `bpm` is the average.
- `rten` uses a pool of one thread per physical core, created by the first
  analysis thread (which is niced, so the pool inherits nice 19). The pool
  size can only be changed with `RTEN_NUM_THREADS` before the first
  analysis.

## Key detection and mood calibration

**Problem (analyser v2 on the real library):** against tunebat/songbpm,
about 5 of 11 comparable K-pop songs matched exactly. Three of the four
minor references came out as a major key: SORRY 12A → 12B, Tell 'em
boys 12A → 1B, PIZZA 11A → 10B. The fourth, Spicy, came out as 10A, which
is right, but at confidence 0.2995, so `describe()` hid it. Only 11 of
49 sampled v2 records were minor at all.
Also `mood.brightness` was **0.000 for all 49 sampled v2 records**.

**Bench:** `cargo run --release -p listen-analysis --example keys` runs
synthetic songs with known keys through MP3 and mpv and then the full
`analyze()` (own beat tracker). There are two sets: `basic`, the 40 S0a songs
(I–V–vi–IV or i–VI–III–VII only), and `pop`, 120 denser arrangements
(`synth::key_song`). The `pop` set uses 11 progressions: 5 major, including
the K-pop "royal road" IV–V–iii–vi, and 6 minor, including i–VII–VI–V and
i–i–iv–v. It also has saw pads, a bass on roots and fifths, a scale-wise
vocal whose phrases end on the tonic, a sustained lead in the chorus, loud
drums, and a ±30 cent tuning offset in a third of the songs.

| | basic exact | pop exact | songs called minor (pop, truth 69) | mean confidence right / wrong (pop) |
|---|---|---|---|---|
| v2 (Temperley, 12-bin chroma) | 24/40 | 60/120 | 9 | 0.85 / 0.78 |
| v3 | **36/40** | **113/120** | 68 | 0.74 / 0.38 |

In v2, every error except one was the relative major. In v3, the 11
remaining errors are 9 relative and 2 fifth confusions, mostly
i–VI–III–VII. That loop is a rotation of I–V–vi–IV with the same notes
and bass, so only the phrase-start cue can tell them apart. Tempo and
beat results don't change because the key stage only reads the grid.

What changed (`key.rs`, `spectral.rs`), each step checked on the bench:

1. **Natural-minor profile.** Temperley's minor gives the leading tone
   4.0 and the ♭7 1.5. Pop minor mostly uses ♭7, so the relative major
   always fit better. v3 uses ♭7 4.0 and leading tone 2.5. This change
   alone took `pop` from 50 % to ~80 %.
2. **Bassline profile.** Peaks from 30–250 Hz go into a separate bass
   chroma, which is correlated with chord-root profiles (I, V, IV, vi
   for major; i, iv, v, VI, VII, III for minor; tonic strongest), with
   weight 0.5. This removes most fifth confusions.
3. **Phrase-start bass.** The tonic's share of the bass in the first half
   of every 4th bar from the phrase lattice gets weight 0.5. Pop loops
   start on the tonic chord. This is used only with a beat grid. Weights
   of 2 or more hurt, because the phrase offset is sometimes off.
4. **Tuning estimation.** Peaks are soft-binned into 36 bins per octave.
   The tuning is the circular mean within the semitone (mean error 0.7
   cent on the bench), and the bins are folded to 12 around the tuned
   centres.
5. **Things tried and dropped:** a prominence filter on peaks against
   drum noise (it dropped real partials and caused more fifth errors),
   per-frame L1 normalisation (worse than per-frame peak normalisation),
   and the Krumhansl, edma and Sha'ath profiles (all lose to
   Temperley-natural plus bass).
6. **Confidence** is the profile fit × (0.25 + 0.75 × margin / 0.3),
   where the margin is measured over the runner-up, whatever key it is.
   A close relative or neighbour on the circle of fifths now reads as
   unsure. On the bench, margins under 0.1 are right 72 % of the time and
   margins of 0.1 or more are right 97 % of the time. `describe()` still
   hides keys below 0.3.

The reference songs can't be re-run from here (no library audio in the
sandbox). Their v2 failures are the pattern the bench fixed: minor songs
read as the relative major, which the natural-minor profile and bass
address. Judge v3 by `--check-analysis` on those songs once v3 records
exist.

**Brightness.** The 85 % power roll-off sits below 2 kHz for every pop
song, because kick and bass hold the power. Its old mapping
(log2(f / 2 kHz) / 2) therefore clamped to 0, which `describe()` shows as
"dark". The stored bliss vectors already contain a magnitude-weighted
mean spectral centroid. Across 74 stored library records (49 v2 + 25
v1, game soundtrack excluded) it spreads well: min 0.46, p10 1.46,
median 1.97, p90 2.33, max 2.46 kHz. Ballads sit low, dance-pop high.
v3 sets brightness = log2(centroid / 1.4 kHz) / log2(2.6 / 1.4), clamped
to 0..1. On those records that gives 22 dark (< 0.33), 30 neutral and 22
bright (> 0.66), median 0.55. `describe()` thresholds stay at 0.33 and
0.66. Without a bliss vector (songs too short), brightness is 0.5.

**Energy** (unchanged): v2 energy on the 49 records has p10 0.39,
median 0.59 and p90 0.72, with a minimum of 0.22 (a ballad) and a maximum
of 0.81, which is "energy 4–7/10" for most songs. LUFS p10/p50/p90 is
−8.9/−7.3/−5.9 and LRA is 3.1/5.4/8.7. The spread is narrow but usable,
and `listen-mix` scores energy steps against it, so it was left alone.

All of this ships in analyser **v3**, together with the beat-grid onset
snapping, so the library is re-analysed once.

## YouTube Music analysis (on demand)

- **No crawl.** YouTube Music songs are analysed only when wanted: the queue (current + next at `Now`, the rest at `Queue`) and the DJ's candidate pool at `Pool`. When a YouTube Music song starts with the Auto-DJ or AI DJ on, its radio (`similar_songs`, one request) is fetched once per song, plus the AI DJ seed's radio once per seed, and the top 20 not yet analysed are queued. The Navidrome library crawl is unchanged.
- **Resolve at job start.** A job carries the song and its source. The worker calls `analysis_source(song, urgency)` when the job starts (URLs expire; fetches are budgeted). `Now`/`Queue` → `Urgency::Now`, `Pool`/`Library` → `Urgency::Background`. A background `Ok(None)` (budget used up) puts the job back for 2 min without counting it as tried. A resolve error is retried twice (after 30 s and 60 s), then given up for the session. The record's `FileId` is the source's (itag, `clen`, duration), not `FileId::of(song)`.
- **Store key: `ytm:<videoId>`.** Navidrome ids are 22-character base62 strings, the same shape as videoIds, and the store is shared. So YouTube Music records are stored and looked up under `ytm:` + videoId (`listen_analysis::record_key`). Navidrome records keep their bare id, so nothing is re-analysed. Lookups match a YouTube Music song by key with `FileId::of(song)`, whose listing duration is often 0 (= match any) and otherwise within 1 s of the analysed one.
- **CLI:** `listen --check-analysis ytm:<videoId>` (or a music.youtube.com / youtu.be link) analyses one song through the configured YouTube Music server and prints the analysis, input and FileId. `listen --check-analysis ytm [N]` analyses N random library songs that aren't analysed yet (default 10). Both write to the shared store.

## S0b mpv deck prototype

**Question:** can two mpv processes, driven over JSON IPC, be lined up to a beat
precisely enough for DJ blends (Stage 4 gate: median ≤ 10 ms, p95 ≤ 20 ms)? What
do tempo changes, live EQ and different codecs do to timing?

**Answer:** yes, if the engine measures and corrects instead of trusting send
times. Both decks report where they are through `audio-pts` to within about
0.1 ms. A start on cue lands within one or two graph quanta (p95 11 ms at a
256-frame quantum, 16 ms at 1024). A ±0.5 % speed nudge then brings the error to
**p95 ≤ 0.5 ms** in the recording. Use rubberband (R3 "finer") as the permanent
tempo filter: the WSOLA filters (scaletempo2, scaletempo) move every beat by
±5–7 ms when the speed is not 1.0. Filter commands take effect about
`audio_buffer` + 140 ms after sending, ±25 ms. Plan EQ moves as ramps sent
early, not as hits on a beat.

### Setup
- mpv 0.41.0 (FFmpeg 8.0, librubberband) on Fedora 44 (16-thread Ryzen desktop).
  Other builds were running at the same time (load average 6–12).
- A private PipeWire 1.6.9 + WirePlumber (`tools/s0b-pipewire.sh`) with one null
  sink at 48 kHz. Both decks play into it, and `pw-record` captures its monitor
  as raw f32 into the test program. The private daemon has no RT scheduling
  (no D-Bus/rtkit), so it drops more cycles under load than a desktop session would.
- `crates/listen-player/examples/twodeck` (run:
  `XDG_RUNTIME_DIR=/tmp/listen-pw cargo run --release -p listen-player --example twodeck -- all`)
  generates the test files:
  - 120 BPM click tracks: 5 ms 2 kHz bursts at 0.8 with an abrupt onset, plus a
    150 Hz bed at 0.02. Deck A's clicks are on the left channel, deck B's on the
    right.
  - A click + 440 Hz tone file.
  - An 80 Hz / 6 kHz file.
  - FLAC, MP3 (LAME 192k), AAC (m4a 192k) and Opus (128k) encodes.

  Click onsets are found by threshold crossing with sub-sample interpolation.
- **Clock mapping.** Each read from `pw-record` ends at frame `f` and arrives at
  host time `t`. The minimum of `t − f/48000` within each 0.1 s window maps
  recorded frames to `Instant`, so a recorded click can be compared with an IPC
  call. The null sink runs on CLOCK_MONOTONIC. The report prints the mapping's
  spread and "jumps" (lost cycles); runs with jumps are noted below.
- All decks run the production chain from
  `listen_core::settings::deck_audio_filters`, except where filters are compared.
- **Absolute offset.** The recorded click comes a fixed 5.3 ms (scaletempo2 or no
  tempo filter) or 10.7 ms (rubberband, 256-frame quantum) after what `audio-pts`
  predicts. This offset is identical for both decks, every codec and every
  trial, so it cancels out in deck-to-deck alignment. Part of it is the monitor
  capture, part is latency mpv doesn't report.

### 1. `audio-pts` (60 s, two decks)
| | 256-frame quantum, buffer 0.2 s | 1024, buffer 0.1 s |
|---|---|---|
| IPC round trip (median / p95 / max) | 0.03 / 0.04 / 0.6 ms | 0.04 / 0.38 / 5.5 ms |
| reading jitter around a straight line (sd) | 0.01 ms | 0.11 ms |
| slope vs host clock | 1.000000 | 0.999999 |
| recorded click − `audio-pts` prediction | 5.34 ms, sd < 0.01 | 5.4 ms, sd 0.2 |
| deck offset in recording − offset implied by both `audio-pts` | 0.00 ms, sd < 0.01 | 0.00 ms, sd 0.19 |
| drift between decks | 0 | 0 |

- `audio-pts` is interpolated: every reading is distinct.
- In mpv 0.41, `time-pos` is also an *output* position, 1–3 ms behind
  `audio-pts`, not the decoder position. Listen's `Position` events are therefore
  already latency-corrected.
- **Underruns.** In a run with `--audio-buffer=0.1` under load, deck B lost
  exactly one 256-frame cycle at 42 s: a 5.33 ms step in the recorded deck
  offset, which `audio-pts` tracked (error sd 0.06 ms). Decks don't drift, but an
  underrun shifts one deck by whole quanta. **The phase lock has to keep reading
  during the whole blend, not correct once.**

### 2. Load paused at `start=`, then unpause (20 trials)
| | q256, buffer 0.1 | q1024, buffer 0.1 |
|---|---|---|
| `load` → `playback-restart` (local WAV) | median 28 ms, max 50 | median 99 ms, max 151 |
| `audio-pts` after the paused load − requested start | 0 (one trial +4 ms) | same |
| `set pause=no` reply | 0.1 ms | 0.3 ms |
| unpause call → first click (recorded, includes the constant offset) | median 13.2, sd 1.8, range 9.7–16.4 ms | median 13.4, sd 5.9, range 6.5–26.5 ms |
| unpause → first sample according to `audio-pts` afterwards | median 2.5, sd 1.8 ms | median 7.9, sd 5.9 ms |
| recorded click − `audio-pts` prediction after the start | sd 0.03 ms | sd 0.3 ms |

- **Where the deck starts is quantized.** Audio starts at a graph cycle, so the
  same unpause lands anywhere in roughly one quantum (5.3 ms at 256, 21 ms at
  1024).
- **Measure after starting.** The reading after the start is exact. So a
  "sleep until deadline − L" start is good to about one quantum. Read the actual
  phase afterwards; don't trust the deadline.
- L is learned per machine from `audio-pts` after each start (the median of
  "when did audio-pts say the first sample played"). It came out at 2 ms (q256)
  and 12–16 ms (q1024).
- With rubberband, `audio-pts` sometimes reads a few ms past `start=` right after
  `playback-restart` (up to 6 ms in the `--ao=null` test). Re-read after unpausing.

### 3. Speed (tempo filters at 0.94–1.06)
Each run plays 4.5 s from 20 s at a fixed speed. Measured over the clicks; ranges
cover two runs (buffer 0.2 without lost cycles, and buffer 0.1):

| `@ts:` filter | beat interval error sd at 0.94–1.06 | `audio-pts` error vs speed (median, sd) | click shape | CPU for 60 s of audio |
|---|---|---|---|---|
| `scaletempo2` | **2.4–6.8 ms** | −5…−10 ms below 1.0, +18…+22 ms above; sd 2–5 | intact | 0.13 s |
| `scaletempo2=search-interval=15` | 2–7 ms, dropped/doubled clicks, 22–38 ms tone dropouts | 3–10 ms, sd 2–4 | intact | — |
| `scaletempo` | 4.2–7.2 ms | −8 … +2 ms, sd 3–5 | intact | — |
| `rubberband=engine=finer` | **0.03–0.15 ms** | 7.8 (0.94), 9.5, 10.7 (1.0), 11.9, 12.9 (1.06); sd ≤ 0.1 | peak 0.71–0.85 (source 0.82), spread 5.1–7.3 ms (source 4.9) | 3.2 s |
| `rubberband=engine=faster` | 0.02–1.0 ms | 8.8–16.0 ms | smeared: 22–27 ms spread, pre-echo ×40–65 | 0.9–1.5 s |
| none (mpv adds its own `scaletempo2`) | 1–9 ms | like scaletempo2 | intact | — |

- **WSOLA filters move beats.** scaletempo2 and scaletempo cut and splice at
  waveform-similar points, so every beat moves by up to ±10 ms and the mean
  tempo is off by 0.2–0.35 % over a few seconds. Their `audio-pts` bias also
  flips sign between slowing down and speeding up. In a blend where one deck
  runs at 0.97 and the other at 1.03, that alone is a ~30 ms misalignment.
- **Rubberband finer keeps beats on time** to 0.15 ms with a stable bias. The
  bias is linear in speed, about 0.42 ms per 1 % (+5.4 ms over scaletempo2 at
  1.0). At 1.0 it is effectively transparent on clicks. It costs ~5 % of one core
  on this desktop; expect 10–15 % on the laptop.

**Changing speed while playing** (12 steps every 0.6 s through 0.97…1.03, crossing
1.0 six times; the 440 Hz tone was checked away from the clicks):

| filter | changes with a dropout | largest sample step (clean sine = 1.0×) | `audio-pts` error max |
|---|---|---|---|
| `scaletempo2` (permanent) | 0/11 | 1.10–1.22× | 8.7–9.8 ms |
| `rubberband=engine=finer` (permanent) | 0/11 | 1.17–1.22× | 13.8–16.2 ms (median 10.7 = its bias) |
| `rubberband=engine=faster` | 1–2/11 (2–14 ms) | 1.14–1.19× | 18–21 ms |
| none (mpv's auto filter) | 0–2/11 (≤ 6 ms) | **26–33×** (an audible click) | 20–21 ms |

- **Confirmed:** without a permanent tempo filter, mpv logs "adding scaletempo2"
  / "removing audio speed filter" every time the speed leaves or returns to 1.0,
  and the output jumps.
- With `@ts:` in the chain the `af` property is untouched and nothing is rebuilt.

### 4. `af-command`
- **Syntax.** mpv `af-command <label> <cmd> <arg> <target>`. The target is the
  ffmpeg instance name as written in the graph: `bass@lo`, `highpass@hp`,
  `lowpass@lp`. A bare class name (`bass`) hits every filter of that class.
  `lo`, `Parsed_bass_0@lo` and `all` are rejected ("error running command").
  - `bass`, `highpass`, `lowpass` and `equalizer` take `f`, `g`, `w` and `mix`
    live. `aecho` takes no commands.
  - The reply comes in about 0.1 ms. It is a reply, not an acknowledgement that
    the change is audible.
- **Latency to audible.** Lowpass 20 kHz → 500 Hz on a 6 kHz tone:

  | `--audio-buffer` | send → audible (median, sd, range) |
  |---|---|
  | 0.2 s | 283–333 ms, sd 25, 245–371 |
  | 0.1 s | 240–246 ms, sd 23, 196–275 |
  | 0.05 s | 186 ms, sd 25, 147–224 |

  - Rubberband adds about 40 ms compared with scaletempo2.
  - The ±25 ms comes from mpv filtering in decode-ahead blocks. It can't be
    observed over IPC, so a command can't be pinned to a sample.
  - The same applies to `speed` changes: they go through the same filter queue.
- **Clicks** on an 80 Hz tone at 0.5, measured as the largest second difference
  relative to the clean sine:
  - bass shelf `g` 0 → −24 dB at once: 17.5×. Over 12 steps: 17.9×. This is
    the same as the f-sweep below, so it is inaudible and stepping buys nothing.
  - highpass `f` 10 → 200 Hz in 16 steps, with `mix` at 1: 21×, fine.
  - **switching `mix` 0 → 1 clicks loudly**: 1 100–6 950× at once and
    2 800–3 800× over 8 steps, even at f = 10 Hz.
- **Idle settings.** The passes therefore idle at `mix=1` with f = 10 Hz
  (highpass) and f = 20 kHz (lowpass), and the engine only sweeps `f`.
  - Measured transparency of that idle setting: RMS within 0.1 %, and a 5 kHz
    tone unchanged at 22.05, 32, 44.1 and 48 kHz.
  - ffmpeg stays stable with f above Nyquist, so one chain serves every sample
    rate.

### 5. Decode (`mpv --ao=pcm`) vs playback, per codec
Clicks from the source WAV were compared with their position after decoding and
after playback on a deck (`start=10`):

| codec | decoded onset − source (48 kHz) | at 22 050 Hz | cued `audio-pts` | heard − prediction (from the source time) |
|---|---|---|---|---|
| WAV | 0.000 ms | −0.001 ms | 10.000000 | 10.68 ms, sd 0.04 |
| FLAC | 0.000 | −0.001 | 10.000000 | 10.68, sd 0.00 |
| MP3 (LAME) | 0.000 | 0.000 | 10.000000 | 10.68, sd 0.04 |
| AAC (m4a) | 0.000 | −0.001 | 10.000000 | 10.68, sd 0.00 |
| Opus | 0.000 | 0.000 | 10.005333 in 2 of 3 runs (256 samples late, and reported as such) | 10.68, sd 0.00 |

- **Encoder delay is handled.** mpv/FFmpeg remove encoder delay and padding (LAME
  tag, MP4 edit list, Opus pre-skip) the same way in `--ao=pcm` decoding and in
  playback. Beat times from the analyser's mpv decode are exactly the times the
  deck plays. Resampling to 22 050 Hz doesn't move them.
- Decoding 60 s took 0.11–0.24 s.
- **Not tested:** Navidrome `format=raw` over HTTP. It is the original file, so
  the same holds as long as both the analyser and the deck decode it with mpv.
  Headerless MP3 or ADTS AAC would carry the encoder delay into both decode and
  playback equally. Seeking and `start=` on HTTP streams still need a check
  against a real server.

### 6. Two-deck beat alignment (20 trials each)
**Procedure:**
1. Deck A plays the left-channel click track.
2. Deck B is loaded paused exactly on a bar of the right-channel track and waits
   for `Restarted`.
3. The engine reads A's `audio-pts`, takes the next bar ≥ 0.8 s ahead, sleeps
   until that moment minus the learned unpause latency, and unpauses B.
4. After 1.1 s it reads A and B back to back and computes the phase error
   (B's progress past its bar minus A's past its bar, extrapolated to one
   instant).
5. It corrects that error, at most 3 times, by an exact relative seek or a
   ±0.5 % speed nudge for |e|/0.005 s.
6. "Recorded" is the median left−right click offset in the recording.

| | timed unpause, by `audio-pts` | timed unpause, recorded | after correction, recorded |
|---|---|---|---|
| q256, buffer 0.1, nudge | median 0.0, p95 10.7, max 12.6 ms | median 0.0, **p95 10.7 ms** | median 0.0, **p95 0.04, max 0.04 ms** |
| q1024, buffer 0.2, nudge | median 10.6, p95 16 ms | median 10.7, p95 16 ms | median 0.0, **p95 0.13, max 0.5 ms** |
| q1024, buffer 0.1, nudge (load, 141 lost cycles) | median 5.2, p95 16 | median 5.3, p95 16 | median 0.0, p95 0.5, max 1.4 ms |
| q256, buffer 0.2, **seek** | median 0.0, p95 5.4 | median 0.0, p95 5.4 | median 0.0, p95 5.9, **max 15.5 ms** |

- **`audio-pts` matches the recording** in every row: the error it computes is
  the error you hear.
- **Seeking is the wrong correction.** It restarts B's output, which gets
  quantized again plus a ~18 ms flush shift that varies, so it can make things
  worse.
- **Nudging is continuous** and lands within a fraction of a millisecond. A
  10 ms error takes 2 s at 0.5 % (0.6 s more to settle).

### Recommendations for the mix engine
- **`audio_buffer`: 0.1 s** on both decks (`MpvOptions::audio_buffer`).
  - 0.2 s works but adds ~90 ms to every filter or speed command.
  - At 0.05 s commands are faster, but underruns become likely under load (one
    showed up at 0.1 s already, and was tracked).
- **Filter chain:** `deck_audio_filters()` =
  user EQ + `@mix:lavfi=[bass@lo=g=0:f=120:w=0.7,highpass@hp=f=10,lowpass@lp=f=20000]`
  + `@ts:rubberband=engine=finer`.
  - Fall back to `deck_audio_filters_with(p, DECK_TEMPO_FALLBACK)` (scaletempo2)
    when mpv rejects rubberband. With the fallback, don't tempo-match beat
    blends: cap at 1.0× or use `Fade`.
  - Drive the filters only through `f` and `g` (sweeps or jumps). Never toggle
    `mix`.
- **Lead time:** send EQ and speed changes **~240 ms early** at a 0.1 s buffer
  (≈ `audio_buffer` + 140 ms) and expect ±25 ms. Make bass swaps and filter
  sweeps ramps of ≥ 1 beat (steps every 25 ms), so the jitter is inaudible.
- **Unpause:** compensate with a learned latency. Start with 0 and keep the
  median of "first sample according to `audio-pts`" per machine; it came out at
  2–16 ms depending on the graph quantum. Expect the start to land within one
  quantum (p95 11 ms at 256, 16 ms at 1024).
- **Phase lock** — this is what meets the gate:
  1. In the silent pre-roll bar, read both decks' `audio-pts` back to back.
     Discard samples with `rtt` > 1 ms and compare with `PtsSample::pts_at`.
  2. Nudge B's speed by ±0.5 % (up to ±2 % while B is still at volume 0) for
     |e|/nudge seconds, then wait ~0.6 s and read again.
  3. Keep this loop running through the blend to catch underruns, which shift a
     deck by whole quanta.
  4. Don't correct with seeks.
- **Rubberband bias:** add ≈ 0.42 ms per 1 % of speed to the phase error when the
  two decks run at different speeds. At 0.97 vs 1.03 it is ~2.5 ms; tiny, but
  free to remove.
- **Expected alignment error:** median < 0.1 ms, p95 < 1 ms after the lock has
  converged, against the gate's median ≤ 10 / p95 ≤ 20 ms.
  - A one-shot timed start without correction would already pass at a 256
    quantum (p95 11 ms) but not reliably at 1024 (p95 16 ms + underruns).
- **Analyser:** decode with `mpv --ao=pcm` at 22 050 Hz as planned. Its beat
  times equal playback times for FLAC, MP3, AAC and Opus.

### Open risks
- **Not tested on real hardware** (ALSA period, Bluetooth). The extra latency is
  the same on both decks, but a desktop session's quantum and RT scheduling
  differ from the private daemon. Repeat this run there; once the real sink is
  in the graph, the `--check-decks` harness in J does exactly that.
- **Rubberband CPU on the laptop** isn't measured; 3.2 s of CPU per minute here.
  The `rubberband=engine=faster` engine smears transients and drops out on speed
  changes, so it is not a good fallback; scaletempo2 is.
- **Filter command timing (±25 ms)** can't be observed through IPC; only ramps
  hide it.
- **HTTP streams** (Navidrome raw): `start=`, `playback-restart` time and exact
  seeks over the network weren't measured. Network stalls are underruns that the
  phase lock has to catch.

## End-to-end test

**Question:** do the transitions work in the real app, with a real server,
real audio output and the Auto-DJ choosing songs?

```sh
tools/dj-e2e.sh 13                      # minutes of playback
DJ_E2E_ACTIONS=1 DJ_E2E_KEEP=1 tools/dj-e2e.sh 13
```

What the script does (everything under `/tmp/dj-e2e`, nothing in the real
config; `--login` writes no credentials):
1. Generates the test library once with
   `cargo run --release -p listen-analysis --example djset -- <dir>`: twelve
   60–95 s synthetic pop songs (`examples/synth`) with known beat grids
   (`truth.json`), tagged MP3 and FLAC. A three-track album "Run" at 124 BPM
   (must stay gapless), and nine singles on their own albums: 118, 120, 122,
   123, 125, 126, 128 BPM (2–5 % apart), 136 BPM (≥ 6 % from all: a Fade) and
   a 62 BPM half-time song. Layouts: drum intro and instrumental outro of 8
   or 16 bars, or a vocal-like melody from the first bar to the last.
2. Starts the latest Navidrome (`~/navidrome/navidrome`, override with
   `NAVIDROME`) on a fresh database, creates `admin`/`listen-test` and waits
   for the scan.
3. Starts the private PipeWire (`tools/s0b-pipewire.sh`) and records the
   null sink's monitor with `pw-record`.
4. Runs `listen --login http://127.0.0.1:4533 admin --autoplay` under
   `xvfb-run` and `dbus-run-session`, with `LISTEN_ANALYSIS=1` (analysis on
   in an ephemeral session) and `LISTEN_AUTOPLAY_ALBUM=Run` (a test hook:
   `--autoplay` plays this album instead of the first one on Home). Auto-DJ
   and mixing are on by default. The beat model is cached in
   `~/.cache/listen-dj-e2e/models` between runs.
5. With `DJ_E2E_ACTIONS=1` it presses buttons over MPRIS at moments taken
   from the log: Next 3 s into the 2nd blend, Pause/Play in the 3rd, a seek
   8 s back in the 4th, Next while the 5th incoming song preloads.
6. Summarises the log (plans per kind, blends, handovers, cancels, gapless
   advances, phase error after lock-in) and checks the recording with
   `examples/djrec.rs`: per blend the quietest 50 ms (gap), the loudest
   half second against the 10 s before (two full-level decks), and hi-hat
   onsets doubled 8–100 ms apart (beats that don't line up).
7. Stops everything and deletes the run folder (`DJ_E2E_KEEP=1` keeps it).

### Results (desktop, 2026-10-02, 13 minutes, with button presses)

| | |
|---|---|
| analysis | all 12 songs in 1.1–2.4 s each, tempo right on all (62 BPM as 62) |
| plans | 11: 2 BeatBlend (8 bars, bass swap), 3 ShortBlend, 2 FilterOut, 4 Fade (136 BPM pair, and the replans late in a song) |
| album "Run" | gapless both times, never planned |
| blends started / handovers / finished | 9 / 7 / 7, no failed preload, no missed start |
| cancels | 4, all from the button presses (pause, seek, Next, the restart that follows) |
| phase error after lock-in (26 readings) | median 0.21 ms, p95 4.2 ms, max 5.9 ms (the larger ones: the half-time pair, ×2 time) |
| recording | no gap and no loudness bump in any blend (loudest half second −1.5…+2.9 dB against before), no flams in a beat-matched blend |
| after the buttons | music goes on from one deck; the only silences are the pause itself and mpv's seek |

### Bugs the test found (fixed)
- **No mix was ever planned after a gapless track change** when both songs
  were already analysed: `mix_consider` only ran on `Analysed` and queue
  edits. Now every track change considers a mix (`on_track_changed`), and
  the next slot isn't prefetched when a mix owns it (`StreamsResolved`,
  `Advanced`).
- **`set_next` could remove the song that had just started**: right after a
  gapless switch the finished entry is still at index 0, and clearing
  "everything after index 0" hit the new song. It now clears after
  `playlist-pos` (test: `clearing_the_next_slot_right_after_a_switch_keeps_the_new_song`).
- **Beat grids up to 8 ms off** (Beat This! is quantised to 20 ms frames; at
  120/125 BPM every beat lands on the same wrong frame), so two grids put a
  blend's beats ~10 ms apart: audible flams in the first recordings although
  the phase lock reported < 1 ms. `beats::refine_to_onsets` snaps the grid
  onto the attacks (median shift per ±8 beats); grids now agree with the
  synthetic truth to < 1 ms. `ANALYZER_VERSION` 3 re-analyses old records.
- **The last bar of a song counted as vocal** (one neighbour in the median),
  hiding the instrumental outro: Short blends instead of Beat blends.
- **The incoming deck was audible during its pre-roll bar** when the
  envelope starts at full (FilterOut).
- **Filter commands to a stopped or not yet loaded deck** failed ("error
  running command"); the incoming EQ is now set once the file is loaded.
- **A `Restarted` from the deck's previous file** could mark the incoming
  deck ready before its load was accepted.
- **The start task could unpause a deck after a cancel** (it sleeps until the
  deadline); it now checks a shared token first.
- **User actions gave the pair up for good** (seek, pause): now only failures
  do; a seek re-plans from the new position, a pause re-plans on play, and a
  plan is never made while paused.
- **Phase lock overshoot**: a 1 s reading interval with a 0.6 s settle
  corrected 167 % and rang; the nudge now spreads over ≥ 1.4 intervals.
- **Settle after a half-time blend lasted a minute** (16 bars at 62 BPM),
  longer than the rest of the song, so the next mix was lost; capped at 32 s.
- After the handover the song's duration was unknown (the `Duration` event
  went to the mixer); it is now carried over.

### Not covered
- Real hardware sinks (ALSA period, Bluetooth), the laptop, and Navidrome
  over a real network (stalls during a blend).
- Real songs: the synthetic set has clean attacks; on dense mixes the onset
  refinement keeps the grid unchanged when attacks are unclear.
- The AI DJ (voice windows, talk-over) and "Add to queue" songs.

## Vocal edges

The planner (`listen-mix::plan::best_candidate`) rejects long blends whose
overlap has vocals on both decks (`clash > 0.1 * bars`) and short blends with
`clash > 0.25`; without a quiet pair it falls to a filter-out or fade.
`settings.mixing.transition_edges` relaxes that:

| Mode | Outgoing outro | Incoming intro |
|---|---|---|
| `instrumental` | strict | strict |
| `always` | relaxed | relaxed |
| `smart` (default) | relaxed if the outgoing song is a club genre | relaxed if the incoming song is a club genre |

A relaxed edge contributes no clash, so a blend is never refused for its
vocals; the cover term in the score still favours quieter sections. Tempo,
key and beat-confidence rules are untouched. Club genres live in
`listen-mix::genre::CLUB_GENRE_TERMS` (whole-word, case-insensitive match per
tag part; tags with rock, punk, metal, pop, folk or country words are
excluded). Genres come from the song's tags (Navidrome, Jellyfin, local files).
YouTube Music songs carry none, so Smart treats them as strict. `--check-mix`
has no song metadata and behaves the same way.

## YouTube Music
Pairs are eligible when both sources
are analysable (Navidrome, YouTube Music, and mixed pairs: both decks decode
with mpv, so beat times line up).
- When a pair is planned, the app starts `MusicSource::mix_input(next)` as a
  task (YouTube Music: a local copy fetched ahead; Navidrome: the raw URL).
  The result is kept per queue uid, so a re-plan (seek) doesn't fetch again.
- Not fetched when the incoming deck should load (`PRELOAD_LEAD_S` before
  its start), or the fetch failed: the pair plays gapless (never a blend
  from a half-fetched file, no retry storm against YouTube's budget).
- `MusicSource::pin_mix` keeps a fetched file out of cache eviction while a
  deck may play it: the incoming song from its fetch on, the outgoing one
  until its deck stops, and the current song while it plays from the file
  it came in with.
- After the handover the duration falls back to the analysis' audio end
  (YouTube Music listings often say 0).
- Offline preview of any pair: `listen --check-mix <a> <b> [--render out.wav]`
  (Navidrome id, `ytm:<videoId>`, file or URL; both analysed afresh).
