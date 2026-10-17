# Tests

This page describes the test suite, the tests that need extra programs or a live service, the end-to-end DJ test, and how to produce screenshots.

## Running the tests

```bash
cargo test --workspace            # everything
cargo test --workspace --lib      # unit tests only (what CI runs)
cargo test -p listen --test mcp   # one integration test file of the app
cargo test -p listen-mix          # one crate
```

Unit tests live next to the code in `#[cfg(test)]` modules. The `app/tests/` folder holds integration tests that boot the real `Listen` state with the demo library and `ephemeral` set (nothing is written to disk) and drive it through `update(Message)`. They cover the queue, play modes, Auto-DJ, playlists, settings, hotkeys, the MCP and remote servers, the updater, layouts at different window sizes, and more. Crates have their own `tests/` folders, for example the fake-server tests in `listen-subsonic`, `listen-dlna` and `listen-jellyfin`.

CI (`.github/workflows/ci.yml`) runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --lib`.

## Snapshot tests

`app/tests/snapshots.rs` renders pages headlessly with `iced_test` and writes PNG files to `target/snapshots/` for visual review. The test also compares against a previous image where one exists. Run it with output visible:

```bash
cargo test -p listen --test snapshots -- --nocapture
```

Open the PNG files after a UI change to see what the pages look like. `app/tests/live_pages.rs` does the same with data from a real server (see below).

## Tests that need mpv

`crates/listen-player/tests/mpv_playback.rs` and `deck.rs` drive a real mpv with `--ao=null`, so they make no sound. They look for `mpv` on `PATH` and skip themselves when it is missing. Set `LISTEN_MPV=/path/to/mpv` to use a specific binary. `LISTEN_MPV_ARGS="--ao=null"` also silences mpv for the app's own tests.

## Live tests

Live tests talk to a real server or service. They skip silently unless their environment variables are set, so `cargo test --workspace` stays offline.

| Variables | Test | Needs |
|---|---|---|
| `LISTEN_ND_URL`, `LISTEN_ND_USER`, `LISTEN_ND_PASS` | `listen-subsonic/tests/live_navidrome.rs`, `app/tests/live_pages.rs` | A Navidrome or OpenSubsonic server with at least one album |
| `LISTEN_JF_URL`, `LISTEN_JF_USER`, `LISTEN_JF_PASS` | `listen-jellyfin/tests/live_jellyfin.rs` | A Jellyfin server with at least one album |
| `LISTEN_LYRICS_LIVE=1` | `listen-lyrics/tests/live_lrclib.rs` | Internet access (looks up a well-known song on lrclib.net) |
| `LISTEN_VIZ_LIVE=1` | `listen-visualizer/tests/live_capture.rs` | Linux with PipeWire and mpv |
| `LISTEN_YTM_LIVE=1` | `listen-ytmusic/tests/live_ytdlp.rs` | Internet, `yt-dlp` and a JavaScript runtime. `LISTEN_YTM_COOKIE` can supply a cookie header; the default is a guest cookie. |
| `LISTEN_TEST_URL=<stream URL>` | `listen-ytmusic/src/audio.rs`, run with `-- --ignored download_url` | A stream URL you resolved yourself |

Example:

```bash
LISTEN_ND_URL=https://music.example.com LISTEN_ND_USER=alex LISTEN_ND_PASS=secret \
    cargo test -p listen-subsonic --test live_navidrome
```

Never commit credentials. Use a test account on a test server.

Other development variables: `LISTEN_ANALYSIS=1` turns song analysis on in an ephemeral session, `LISTEN_AUTOPLAY_ALBUM=<title>` makes `--autoplay` start that album, `LISTEN_RIP_KEEP=<dir>` keeps the album produced by the ripping pipeline test, and `LISTEN_VIZ_DEBUG=1` prints visualizer diagnostics.

## The DJ end-to-end test

`tools/dj-e2e.sh [minutes]` tests DJ transitions in the real app. It generates synthetic songs with known beat grids, starts a local Navidrome on a fresh database, creates a private PipeWire graph with a null sink, runs Listen headless under Xvfb with the Auto-DJ and mixing on, and then summarises the mixer log and a recording of the sink (silent gaps, two decks at full level, doubled onsets). Everything is isolated in a work folder, and no real configuration or credentials are touched.

Needs: `navidrome` (path in `NAVIDROME`), PipeWire with `pw-record`, `xvfb-run`, `dbus-run-session` and a release build (`LISTEN_BIN`, built if missing). Useful variables:

| Variable | Effect |
|---|---|
| `DJ_E2E_DIR` | Work folder (default `/tmp/dj-e2e`). |
| `DJ_E2E_KEEP=1` | Keep the run folder with logs and the recording. |
| `DJ_E2E_RECORD=0` | Do not record the sink. |
| `DJ_E2E_ACTIONS=1` | Press next, pause, seek and next-while-preloading over MPRIS during the blends to test the cancel paths. |
| `DJ_E2E_MODELS` | Cache folder for the beat model between runs. |

## Screenshots

Demo mode gives repeatable content for screenshots:

```bash
tools/screenshot.sh out.png 1600x950 --demo --route albums
tools/screenshot.sh onboarding.png 1000x700 --ephemeral --onboarding
```

`tools/screenshot.sh OUT.png WxH [listen args]` runs Listen on a virtual X screen (`xvfb-run`), because some compositors ignore requested window sizes, and passes `--screenshot` and `--size` to the app. It uses `target/release/listen` unless `LISTEN_BIN` is set. Useful options to combine: `--theme <ID>`, `--queue`, `--video`, `--screenshot-delay <ms>` for pages that need time to load, and `LISTEN_LANG=<code>` for translated screenshots. `--onboarding` shows the first-start wizard even in a demo or ephemeral run.
