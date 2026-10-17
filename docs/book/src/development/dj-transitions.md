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
- `plan` decides which pairs may be mixed at all (`mix_eligible`) and builds a `TransitionPlan`: the kind of transition (a beat-matched blend or a plain fade), where the outgoing song stops and the incoming one starts, envelopes for volume, bass and filters, and speed ramps. Songs more than a few percent apart in tempo get a fade instead of a beat match.
- `render` makes an offline preview of a plan, available through `listen --check-mix --render`.

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

## Where to read more

The full notes explain why each choice was made, with results for the analyser benchmark, key detection, the mpv deck prototype and the end-to-end run:
[github.com/leks2205/Listen/blob/main/docs/dj-transitions.md](https://github.com/leks2205/Listen/blob/main/docs/dj-transitions.md).
