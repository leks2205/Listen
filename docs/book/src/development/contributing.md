# Rules for contributors

Short rules for anyone (person or AI agent) changing this repository. Read them before your first
change; the rest of this guide (architecture, building, tests, translations, DJ transitions) has the
details.

## Before you commit

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`. The iced simulator tests in `app/tests/` need a desktop (GPU) session.
- App unit tests live in the lib target: `cargo test -p listen --lib <name>` (`--bin listen` has none
  and passes vacuously).
- A user-visible change updates `docs/book/src/` in the same commit; new UI strings get their German
  text in `app/i18n/listen/de.json` and the same entry in the generated `app/i18n/de.json` (see
  [Translations](translations.md); `merged_tables_carry_every_listen_entry` checks the pair).

## Rules that break things far away

- **Never remove or rename a settings field.** Old `config.toml` files must load and round-trip
  (`v0_23_config_round_trips_unchanged`); keep an unused field and document it as unused.
- **The crates under `crates/` are also built by another app pinned to a commit** — every one except
  `listen-rip` and `listen-visualizer` (the desktop-only CD ripper and visualizer): don't rename or
  remove their public items; add the new one and keep the old one working. A crate that becomes
  desktop-only, or a new shared one, updates this list.
- **Tracked files and commit messages carry no personal names, e-mail addresses or private host
  names**, and no attribution trailers. Notes about who reported what go in `docs/PROGRESS.md`
  (git-ignored).
- **A stream URL never goes into a command line.** Server URLs carry the login (Subsonic `u`/`t`/`s`,
  Jellyfin `apiKey`) and every local user can read a process's arguments. Hand URLs to mpv over IPC
  (`loadfile`, as `listen-player` does) or as a playlist on stdin (`--playlist=-`, as
  `listen_analysis::decode` does); local file paths may stay in argv.
- **Tests that need the network, a real device or a running service are `#[ignore = "live: …"]`**
  (run them with `-- --ignored`). An early `return` when an environment variable is missing makes
  them pass without testing anything, and nobody sees that they never ran.
- **Snapshot tests are render checks, not comparisons.** Each run deletes the old PNG and writes a
  new one, so a snapshot test fails only when the page panics. A test whose name promises how a page
  looks (`*_fits_*`) asserts on widget bounds or text. Don't snapshot content chosen at random (e.g.
  the Recommended strip): two runs can't be compared by eye. Look at it once by hand; test the
  behaviour instead.

## Traps that already cost a bug

- `mix::Phase`: with DJ transitions on, `Planned` holds for most of every song. `phase != Idle`
  means "a plan exists"; use `Phase::blending()` / `Phase::decks_busy()` for "two decks in use".
- `Playback::playing` is mpv's "not paused" and is true while mpv idles with nothing loaded; use
  `playing_here()` for "a song plays here".
- `Player::af_command` values are lost whenever mpv rebuilds the filter graph (seek, new file,
  `set_af`); re-send them on `PlayerEvent::Restarted`.
- Analysis store keys: build them with `listen_core::analysis::record_key` / `YTM_PREFIX`; YouTube
  Music records are `ytm:<videoId>`. `ShardStore::open` takes the folder that contains `v1/`.
- Two keys for a song: `listen_core::versions::song_key` folds every version of a song into one
  (DJ dedupe, exclusions); `Listen::server_song_key` is one exact song on one server (caches).
- Every song a DJ adds from outside its own pool (fills, bridges, tempo picks) must pass the scene
  gate (`listen_mix::pool::gate_pool`, `listen_analysis::bridge_scene_gate`).
- An iced `Task` is lazy: a method that returns one (`dlna_call`, `player_task`, `start_current`, …)
  has done nothing until the task is returned from `update` or batched into what is. `let _ =`
  silences `#[must_use]` and drops the work.
- "The music stopped with songs queued" is handled in one place, `keep_going` (`app/src/app/transport.rs`);
  extend it instead of adding another special case.
