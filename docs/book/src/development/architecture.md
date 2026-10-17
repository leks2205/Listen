# Architecture

Listen is a Cargo workspace: one application crate (`app`, binary name `listen`) and a set of library crates under `crates/`. This page explains what each part does and how data flows through the app.

## Workspace crates

| Crate | What it does |
|---|---|
| `listen-core` | Data model (songs, albums, artists, playlists), the play queue, history, the settings structs and their defaults, the Feishin settings importer, themes, and the shared analysis types. No UI and no network code. |
| `listen-source` | The `MusicSource` trait that every backend implements, a `Capabilities` bit set (folders, ratings, radio, playlist editing, lyrics and so on), query and sort types, client-side sorting and filtering, and the parser for Navidrome smart playlists (`nsp`). |
| `listen-subsonic` | Navidrome and OpenSubsonic backend. |
| `listen-jellyfin` | Jellyfin backend, including Quick Connect and server discovery. |
| `listen-ytmusic` | YouTube Music backend. See [YouTube Music internals](youtube-music.md). |
| `listen-local` | Local folders: scanner and tag reader. |
| `listen-player` | The mpv wrapper: starts mpv with a JSON IPC socket, keeps the two-slot (current and next) gapless model, reports events and audio timestamps. |
| `listen-analysis` | Song analysis: decoding through mpv, beat and downbeat tracking, key, structure, timbre, loudness, the waveform, the shard-based result store and the analysis worker protocol. |
| `listen-mix` | DJ transition logic without DSP or mpv: scoring how well two songs follow each other, ordering DJ picks, and planning where and how to mix. |
| `listen-dj` | The AI DJ: prompts, personas, the Gemini text and speech client, request quota. |
| `listen-lyrics` | Lyrics providers, LRC parsing, ranking, translation and romanisation helpers. |
| `listen-visualizer` | Audio capture and spectrum analysis for the visualizer. |
| `listen-dlna` | UPnP/DLNA casting: discovery, SOAP control and a streaming proxy. |
| `listen-offline` | Downloaded songs. |
| `listen-rip` | CD ripping pipeline (whipper, FLAC, MusicBrainz tags). |

## The app crate

`app/src/main.rs` only calls into `app/src/lib.rs`, which parses the command line (`Args`) and starts the iced application. Most of the code is under `app/src/app/`, with one module per feature area:

| Area | Modules |
|---|---|
| Playback | `transport`, `queue_ui`, `play_mode`, `fade`, `audio`, `sleep_timer`, `decks`, `mix/`, `dj`, `dj_mix`, `video/` |
| Library pages | `home`, `album_page`, `artist_page`, `list`, `list_query`, `search`, `playlists/`, `smart_playlist`, `radio`, `item/` |
| Integrations | `integrations` (remote control, Discord, MPRIS, tray), `mcp`, `dlna`, `updater` |
| Settings | `settings_shell`, `settings_msg`, `advanced`, `appearance` |
| Misc | `analysis`, `history`, `offline`, `rip`, `milkdrop`, `viz`, `onboarding`, `palette`, `overlay`, `menu/` |

The user interface is in `app/src/ui/`: one module per page or widget group (player bar, queue, settings tabs, context menu and so on). `app/src/i18n.rs` holds the translation tables, `app/src/mpris.rs`, `smtc.rs`, `tray.rs`, `discord.rs` and `global_shortcuts.rs` hold the platform integrations, `app/src/mcp/` and `app/src/remote/` hold the two network servers, and `app/src/demo.rs` builds the demo library.

## The Elm architecture in iced

Listen follows iced's Elm-style loop. The central type is `Listen` in `app/src/app.rs`, which holds all state. Four functions on it drive the app:

- `update(&mut self, Message) -> Task<Message>` changes the state in response to a message and may return tasks (asynchronous work) whose results come back as new messages. It dispatches to `update_message`, which hands each message group to the matching `app/*.rs` module.
- `view(&self) -> Element<Message>` builds the whole widget tree from the state. It never changes anything.
- `subscription(&self) -> Subscription<Message>` lists the event streams the app listens to right now: window and keyboard events, timers, mpv events, and the background streams described below. The list depends on the state, so a stream is only active when its setting is on.
- `theme(&self)` and `title(&self)` give the current theme and window title.

`Message` is one large enum whose variants are grouped by feature (for example the `Mcp(McpMsg)` variant for everything the MCP server sends).

```text
 user input ---------------> view() --Message--+
 window / keyboard / timers -> subscription() --+
 mpv events (listen-player) -> subscription() --+--> update() --> state (Listen)
 analysis worker ----------> subscription() ----+        |            |
 MCP server calls ---------> subscription() ----+        |            v
 remote control commands --> subscription() ----+        |         view()
 finished Tasks ------------------------------------------+
                                                         v
                                   Task: network requests, mpv commands, files
```

## Music sources

All backends implement the `MusicSource` trait from `listen-source`. The app holds one `Arc<dyn MusicSource>` per configured server (`SourceHandle`) and calls the same methods for every kind of source: listing and searching albums, artists, songs and genres, playlists, lyrics, similar songs, radio, scrobbling and resolving a stream URL. Features a backend lacks are declared through `Capabilities`; the UI hides the matching controls. Adding a source means implementing the trait in a new crate and wiring it up where the app connects to servers (see `app/src/app/servers.rs`).

## Playback and the two mpv decks

`listen-player` starts an mpv process per player and talks to it over its JSON IPC. The queue lives in the app, not in mpv. mpv holds two entries, the current song and the next one, and moves to the next gaplessly; the app then drops the finished entry and appends the following song.

`app/src/app/decks.rs` wraps two such players. The active deck plays the current queue song. The idle deck is empty, or it carries the incoming song during a DJ transition. Without transitions only deck 0 runs. During a transition `app/src/app/mix/` preloads the incoming song paused on the idle deck, starts it at the planned moment, phase-locks it to the outgoing deck by nudging its speed, applies the planned volume and EQ envelopes, and makes it the active deck halfway through. Anything unexpected cancels back to gapless playback. See [DJ transitions internals](dj-transitions.md).

## Analysis worker

Analysis runs in a separate short-lived helper process, started as `listen --analysis-worker`. Decoding and the beat model use a lot of memory that the allocator does not return, so the helper exits when idle and is replaced when it grows too large. The protocol is one JSON line per request on stdin and one per answer on stdout (`crates/listen-analysis/src/worker.rs`).

On the app side, `app/src/app/analysis.rs` owns the job queue (the play queue first, then the AI DJ's candidate pool, then the library crawl), starts the helper, writes results to the shared store and exposes lookups for the mixer and the Auto-DJ. Results reach the update loop as messages from a subscription.

## MCP and remote servers

Both servers run on the tokio runtime, in the same process, and never touch the state directly.

- `app/src/mcp/` is the MCP server (Streamable HTTP, bearer token). It answers `initialize`, `ping` and `tools/list` itself. A `tools/call` becomes a `Call` with a one-shot reply channel and is sent through a channel to the app. `app/src/app/mcp.rs` turns that channel into a subscription, executes the tool inside `update` (so it sees the same state as the UI) and answers through the reply channel. Tools that ask a server run as tasks and answer when they finish.
- `app/src/remote/` is the remote control server: HTTP plus WebSocket on one port, an embedded phone page and Feishin's message format. The app publishes a snapshot of the player (`Shared`) for clients to read; commands from clients arrive through a channel that `app/src/app/integrations.rs` exposes as a subscription.

Because every outside command goes through a `Message`, there is one place where state changes. This also makes the servers testable: `app/tests/mcp.rs` and `app/tests/remote.rs` drive the real update loop without a window.

## Persistence

Settings and server lists are TOML files handled by `listen-core::config`; credentials go to the system keyring, with a private `credentials.toml` as the fallback. Analysis results live in a shard store that several computers can share. See [Files and folders](../reference/files.md).
