# DJ transitions internals

This page is a short overview of how Listen mixes one song into the next. The measurements, benchmark tables, prototype results and the end-to-end test report are in the full design notes: [docs/dj-transitions.md on GitHub](https://github.com/leks2205/Listen/blob/main/docs/dj-transitions.md). For the user side, see [DJ transitions](../dj/transitions.md) and [Song analysis](../dj/analysis.md).

## The pipeline

```text
 song --> analysis --> store --> planner --> two mpv decks
          (helper)     (shared)  (listen-mix)  (phase lock, EQ, speed, volume)
```

### 1. Analysis

`crates/listen-analysis` decodes each song through mpv to 22,050 Hz audio, so the beat grid is in the same time base as playback (mpv removes MP3 encoder delay exactly). From the samples it computes:

- **Beats and downbeats** with the neural beat tracker `beat-this` (small model, MIT-licensed, run with the pure-Rust `rten`). The model files are downloaded once on first use and checked against a pinned SHA-256. An own dynamic-programming tracker is the fallback when the model is not available; a BPM tag can correct its half-tempo errors.
- **Key**, with an own detector based on Temperley profiles over spectral-peak chroma, reported as Camelot keys with a confidence.
- **Structure**: intro and outro length in bars, vocal and breakdown sections, an energy curve.
- **Timbre** with the `bliss-audio` vector, **loudness** (EBU R128) and a waveform.

Analysis runs in a short-lived helper process (`listen --analysis-worker`) and results go to a shard-based store that can be shared between computers. YouTube Music songs are analysed on demand only, within a request budget; see [YouTube Music internals](youtube-music.md).

### 2. Planning

`crates/listen-mix` contains no DSP and no mpv code, which makes it cheap to test.

- `score` rates how well one song flows into another (tempo, key, energy, timbre, mixability, repetition) and orders DJ picks with a beam search.
- `plan` decides which pairs may be mixed at all (`mix_eligible`) and builds a `TransitionPlan`: the kind of transition (a beat-matched blend or a plain fade), where the outgoing song stops and the incoming one starts, envelopes for volume, bass and filters, and speed ramps. Songs more than a few percent apart in tempo get an echo-out or a cut (no overlap of the dry signals, each deck at its own tempo; `best_handover`) instead of a beat match, and an echo-out or cut also beats a filter-out over clashing vocals; a fade is left for songs without a beat grid and for AI DJ talk-overs. Each blend candidate fits its tempo to the overlapping bars (`local_match`): the octave from the overlap's length, then a least-squares speed ratio over every beat of the overlap, anchored at the shared first downbeat, and a linear glide (`TransitionPlan::in_glide`, read through `in_speed_at` by the desktop mixer, the preview renderer and the Android mixer) when that takes more than 2 ms of drift off and changes the speed by at most 4 %. When a grid slows down and picks up again inside the blend (a ritardando, a live drummer), a speed curve with one knot per outgoing bar (`fit_curve`: the speed linear between knots, its integral fitted to the incoming beats by least squares, second differences penalised more until the curve stays within 4 %) replaces the ramp if it takes another 2 ms off, or if the ramp leaves more than 4 ms and the curve gets any closer (`curve_wins`); it is `TransitionPlan::in_curve` (wall seconds after the mix point, speed), `in_glide` keeps its ends for older readers, and `in_speed_at` interpolates it. Beats past the last one the analyser found (the grid extrapolates at the nominal tempo) are left out of the fits. On the bug-129 pairs: DAUM → Being Freezed (8-bar BeatBlend) 25.6 → 1.3 ms, Falling Leaves are Beautiful → Mutuals (4-bar ShortBlend) 8.5 → 2.8 ms (4.1 ms with its 3.9 % ramp alone; measured on every grid beat between the mix point and the stop, in wall time). The drift that is left costs `DRIFT_PER_S` beyond 4 ms. On 427 real history pairs the plans played without phase nudges keep 193 of 210 beat-mixed pairs within 4 ms on every beat (98 of 222 with the songs' average tempos; worst 53 ms instead of 1.3 s); 12 pairs whose overlapping bars can't be matched become echo-outs or cuts. `PlanCtx::no_echo` (set by the app when the outgoing song itself came in through an echo-out, `MixState::entered_echo`) drops the echo candidates, so two echo-outs never follow each other; `bridge::gap` counts an echo-out or cut as "no beat-matched join", `bridge::always_try` exempts such pairs from the bridge chance and `bridge::choose` then asks no flow gain of the legs over the direct pair (only that they flow no worse). The echo is four `adelay` taps behind a send `volume` mixed back by `amix` in the deck's lavfi chain (live `delays`/`weights`/`volume` commands, `listen_core::settings::DECK_ECHO_*`). `amix` and `volume` step in one sample, so the send and the dry signal are switched by `afade` gates whose `start_time` is set to the planned file time 1.5 s ahead (`deck_echo_commands`): each switch is a 5 ms ramp at exactly that sample, as in `listen_mix::echo`, the same effect on samples for the preview and the Android mixer.
- `loudness` (contract C5) also holds the blend gain: `plan` stores `BlendLevels` in each overlapping plan (both decks' local loudness every 0.5 s across the blend, from `Features::lufs` anchored on the per-bar energy curve and power-averaged over 3 s, plus the 4 s edge levels before and after), and `blend_trim(plan, t, g_out, g_in, deck_gain_out_db, deck_gain_in_db)` is the trim both engines and `render` apply to both decks: the summed loudness follows the line between the edge levels, weighted by how much the decks overlap, clamped to −6..+3 dB. It replaced the fixed 0.85 trim. A boost may take a deck above unity: the desktop sends mpv volumes up to its default `volume-max` of 130 % for it (`listen_player::MAX_VOLUME`), so the app plays what `render` and `verify` model. Handovers get none.
- `render` makes an offline preview of a plan, available through `listen --check-mix --render`.
- `verify` is render-and-verify: `plan_verified` takes the planner's options in order (`plan::alternatives`: the chosen plan, then the other candidates and handovers by score, then the plain fade) and plays each one through on the analysis curves every 0.1 s. It uses both decks' local loudness (`loudness::LocalLevel`), the vocal curve with lyrics merged in, the gain envelope, the equal-loudness trim, rough loudness and vocal losses of the EQ moves, and the echo repeats. It arms the first plan that passes the gates: a hole or bump over 1.5 LU (against both the songs' own levelled loudness and the edge line; a modelled hole counts 0.6 LU deeper, `HOLE_MARGIN_LU`, the curves' error against rendered blends), more than 1 s of audible vocals on both decks, or a mid-line exit (lyric cues only). The desktop plans before it fetches the incoming file, so the check runs on curves rather than audio. `audio_levels` and `audio_dip_bump` measure a rendered preview the same way (BS.1770 K-weighting over left and right, 3 s short-term) for `--check-mix --render`. The reference for each deck is that deck alone on the preview's own timeline (`render::render_decks`): a tempo ramp before the blend moves the outgoing up to a second away from its file, and measuring against the file turned level steps at the mix point into holes the mix doesn't have. On 427 real history pairs the planner's first choice fails in 12 pairs (a hole within the margin in 11, a cut line in 1; each arms another blend) and the check takes 0.08 ms on average.
- `lyric` merges a `VocalMap` (from `listen_lyrics::vocal_map`: sung stretches, line ends, chorus occurrences of synced lyrics) into `bars.vocal` and sets `bars.cue` flags (`LINE_END`, `ENTRY`, `CHORUS`, `CHORUS_END`). It drops lyrics that are anti-correlated with the audio vocal curve or fit clearly better shifted by up to 6 bars. The planner then adds cue-outs after a sung phrase or at a breakdown in the last 40 % (cut cost 0.02/s), cue-ins 1, 2 or 4 bars before a verse or chorus entry (skip cost 0.01/s), +0.5/+0.3 when a blend ends on the incoming chorus/entry, and −0.5 for stopping the outgoing mid-line. Without lyrics the plans are unchanged. The desktop looks lyrics up for the playing and next song (`app/src/app/mix/vocal.rs`) and re-plans when they arrive before the preload.

### 3. Two mpv decks

`app/src/app/mix/` and `decks.rs` run a plan on two mpv players:

1. About 20 seconds before the mix point the incoming song is loaded paused on the idle deck, with its start position set.
2. It is unpaused at the planned moment. A learned latency compensates for the start-up delay.
3. **Phase lock:** during the silent pre-roll bar both decks' `audio-pts` are read back to back, and the incoming deck's speed is nudged by about 0.5 percent (up to 2 percent while it is still silent) until the beats line up. The loop keeps running through the blend to catch underruns. Seeks are not used to correct the phase.
4. Volume, bass, high-pass and low-pass changes are sent slightly early (about 240 ms with the 0.1 s audio buffer) and as ramps of at least one beat, so the jitter of mpv's filter commands is inaudible. Tempo matching uses the rubberband filter, with scaletempo2 as a fallback.
5. Halfway through the blend the incoming deck becomes the active deck.

Anything unexpected (a late deck, a skip, a seek, a pause) cancels the blend and falls back to mpv's gapless playback.

## Measured accuracy

In the prototype, the two decks stayed aligned with a median error below 0.1 ms and a 95th percentile below 1 ms once the lock converged. The design target was a median of 10 ms or less. The end-to-end test (`tools/dj-e2e.sh`, described in [Tests](tests.md)) checks real blends for gaps, level and doubled beats.

## Tempo matching (Android)

The Android app's heart-rate mode uses `listen_mix::tempo`: a reading (heart beats or steps per minute
from a Wear OS watch, a Bluetooth strap or the phone's step detector) is smoothed with a 30-second time
constant and turned into a tempo target clamped to 70–180 BPM, ± 8 BPM, with half and double time
counting (85 BPM fits 170). When the target has moved more than 5 BPM since the last time, the upcoming
automatic queue items are re-ranked: songs within 4 BPM stay, closest first, the others make room for
tempo-targeted Auto-DJ picks. Songs added by hand and the playing song are never touched. Music does not
measurably change heart rate during exercise; the mode picks music that fits the effort, it is not
training guidance.

## Where to read more

The full notes explain why each choice was made, with results for the analyser benchmark, key detection, the mpv deck prototype and the end-to-end run:
[github.com/leks2205/Listen/blob/main/docs/dj-transitions.md](https://github.com/leks2205/Listen/blob/main/docs/dj-transitions.md).
