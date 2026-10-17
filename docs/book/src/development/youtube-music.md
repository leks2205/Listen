# YouTube Music internals

This page describes how the YouTube Music source (`crates/listen-ytmusic`) works. For the user side, see [YouTube Music](../getting-started/youtube-music.md).

> **Note:** Listen's YouTube Music support is unofficial and not affiliated with Google or YouTube. It uses undocumented interfaces that can change without notice and may break at any time. Users sign in with their own account at their own risk. Listen does not download YouTube Music songs for offline use; songs are streamed only. `yt-dlp` is a separate program that the user installs.

## Building blocks

| Part | Implementation |
|---|---|
| Library, search, albums, artists, playlists, likes, history, lyrics | The `ytmapi-rs` crate (version 0.3.3, features `rustls` and `simplified-queries`), which speaks YouTube Music's InnerTube interface in the same way as the `ytmusicapi` project. Entry point: `crates/listen-ytmusic/src/lib.rs`. |
| Audio streams | `yt-dlp` resolves a song to a direct stream URL that is handed to mpv (`ytdlp.rs`). URLs are cached until about 15 minutes before their `expire=` time. |
| JavaScript runtime | yt-dlp needs Deno or Node.js to work with YouTube. Listen looks for `deno` and `node` on `PATH` and in the usual per-user locations (for example `~/.nvm` and `~/.local/bin`), because desktop launchers often do not pass the shell's `PATH`. |
| Radio and Auto-DJ | The "watch next" list (`get_watch_playlist_from_video_id`). |
| History | Playing a track reports it with `add_history_item`, so recommendations stay current. |

Helper program paths can be overridden with `LISTEN_YTDLP` for development, and are set in **Settings -> Advanced -> Helper programs** for users.

## Sign-in

Google's own sign-in flow for third-party apps (OAuth) is not usable with YouTube Music, so Listen works with the session of the user's browser or with an exported cookie file.

### Reading a browser's session

`browser.rs` finds browsers on the computer that are signed in to YouTube and reads their youtube.com cookies. Only the location (browser profile and channel) is stored in the keyring, never the cookies; they are read again on every start.

| Browser family | How the cookies are read |
|---|---|
| Chromium-based: Chrome, Chromium, Brave, Edge, Vivaldi, Ecosia (also as Flatpaks) | The `Cookies` SQLite database is copied first, because the browser locks the original while it runs. Values marked `v10` use Chromium's built-in key. Values marked `v11` need the browser's password from the Secret Service (read with `secret-tool`). Databases from Chromium 130 on (meta version 24 or higher) prefix each value with a SHA-256 of the host, which Listen strips. |
| Firefox | `cookies.sqlite`, which is not encrypted. |

### Cookie file and guest mode

As a fallback, Listen accepts an exported cookies file (JSON or `cookies.txt` format, Cookie-Editor style JSON, or a raw `Cookie` header). It keeps only the cookies a browser would send to music.youtube.com and requires `SAPISID`. Without any cookies Listen runs in guest mode with a consent-only cookie (`SOCS`): search, albums, artists, radio and playback work; there is no personal library.

### Channels and brand accounts

A Google account can have several YouTube identities: its own and brand-account channels. The music library belongs to one of them. After sign-in, `channels.rs` lists the identities with their library size (using `account/accounts_list`) and the user picks one. Requests then carry the `X-Goog-PageId` header for that channel (`token.rs`). Without it, a library on a brand channel looks empty.

## Library mapping and ytmapi-rs notes

- IDs are newtypes; convert with `YoutubeID::from_raw` and `get_raw`.
- Playlist browse ids need the `VL` prefix for reading (`get_playlist_tracks`); editing uses the raw id.
- "Liked Music" is the playlist `VLLM`. It is hidden from the library playlist list, so Listen adds it back.
- Simplified queries return only the first page. `yt.stream(&Query)` collects all pages.
- The ytmapi-rs playlist and library-song parsers fail the whole list when one entry lacks a field. Listen reads song lists itself (`home::song_page`): tolerant of missing fields, all continuation pages, both continuation styles.
- Tracks are library songs plus Liked Music. Albums are saved albums plus the albums of those songs. Artists are the artists of library songs plus subscribed artists.
- Durations and years arrive as strings. Search results carry one artist string without ids. Album tracks have no artists or thumbnails of their own (the album's are used). Artist top songs have no duration.
- There is no home-feed query in the crate. Listen requests `browse FEmusic_home` itself and parses the shelves (`home.rs`): quick picks, mixes and playlists, albums, plus two continuation pages.
- Uploaded music appears as an "Uploads" list in Playlists, only when the account has uploads.
- The song/video switch (player bar, YouTube Music songs only, off by default) starts a second muted mpv window with the music video, kept within 0.7 s of the audio. The counterpart video id comes from the `next` endpoint, else the song's own video.

## Request budget

YouTube limits how fast one client may resolve streams; going over leads to "confirm you're not a bot" answers. `budget.rs` keeps Listen's background work well below that:

| Constant | Value | Meaning |
|---|---|---|
| `BACKGROUND_MIN_GAP` | 10 s | Minimum distance between background requests. Any request, urgent or not, pushes the next background slot back. |
| `BACKGROUND_PER_HOUR` | 150 | Background requests only start while fewer than this many requests of any kind happened in the last hour. |
| `BACKGROUND_MAX_WAIT` | 15 s | How long a background request waits for a slot before giving up. The caller tries later. |

Requests are either urgent (`Urgency::Now`: playback, the current and next queue song, retries) or background (analysis of the AI DJ's candidate pool). Urgent requests always pass but are counted. Background work gets the slots that are left. YouTube Music songs are therefore analysed only on demand, never in a full-library crawl.

## When it breaks

YouTube changes its interfaces often. When playback fails, update `yt-dlp` first. When library pages fail, a newer `ytmapi-rs` is usually needed. Two helpers:

- `listen --check-youtube-music` signs in through the browser session and prints channels, counts and whether every playlist and album opens. It prints counts only, never cookies.
- `LISTEN_YTM_LIVE=1 cargo test -p listen-ytmusic --test live_ytdlp` runs an opt-in live check. See [Tests](tests.md).
