# YouTube Music in Listen

## Signing in with your browser (recommended)
**Add server → YouTube Music → Sign in with <browser>.** Listen finds a browser on this computer that is signed in to
YouTube and reads its youtube.com cookies (`crates/listen-ytmusic/src/browser.rs`):
- Chromium-based: Ecosia (also the Flatpak `org.ecosia.Browser`), Chrome, Chromium, Brave, Edge and Vivaldi. Listen
  reads the `Cookies` SQLite database from a copy, because the browser locks it while running.
  - `v10` values (Chromium's built-in key, e.g. Flatpaks without keyring access) are decrypted directly.
  - `v11` values need the browser's password from the Secret Service (read via `secret-tool`).
  - Chromium 130+ databases (meta version ≥ 24) prefix values with SHA-256(host), which Listen strips.
- Firefox: `cookies.sqlite` (not encrypted).

The keyring stores only *where* to read the cookies (browser profile + channel), never the cookies. They are read again
on every start, so Listen stays signed in as long as the browser does.

**Channels / brand accounts.** A Google account can have several YouTube identities: its own, plus brand-account
channels. The music library belongs to one of them. After "Sign in with …", Listen lists them with their library size
(`channels.rs`, via `account/accounts_list`), and you pick one. Requests then act as that channel with the
`X-Goog-PageId` header (`token.rs`).

Without this, a library that lives on a brand channel looks empty: requests without a page id see the account's
own identity.

Google sign-in (OAuth) is **not possible**. Since Aug 2025 YouTube Music rejects third-party OAuth tokens with
"400 Request contains an invalid argument"; see ytmusicapi #813, where the maintainer sees no way around it.

Check what Listen sees without starting the UI: `listen --check-youtube-music`. It prints channels, counts and whether
every playlist and album opens, never cookies.

## Signing in with a cookies file (fallback)
Under **Other ways to connect**: export music.youtube.com's cookies with **Get cookies.txt LOCALLY** (JSON or
cookies.txt; Cookie-Editor/EditThisCookie JSON and a raw `Cookie` header work too). Then **Browse…** to the file,
drop it onto the window, or paste it. Listen keeps only the cookies a browser would send to music.youtube.com and
needs `SAPISID`. The cookies stop working when you sign out of YouTube in that browser.

**Guest mode:** leave the field empty. Search, albums, artists, radio and playback work; there is no personal library,
likes, playlists or history. Listen uses a consent-only cookie (`SOCS`) because EU visitors are otherwise redirected
to Google's consent page.

## How it works
| Part | Implementation |
|---|---|
| Library, search, albums, artists, playlists, likes, history, lyrics | `ytmapi-rs` (InnerTube, like ytmusicapi) — `crates/listen-ytmusic/src/lib.rs` |
| Audio streams | `yt-dlp -f bestaudio -g` → direct `googlevideo.com` URL for mpv; cached until ~15 min before `expire=` — `ytdlp.rs` |
| JS runtime for yt-dlp | Deno or Node, auto-detected incl. `~/.nvm` and `~/.local/bin` (desktop apps don't get the shell PATH) |
| Radio / Auto-DJ | `get_watch_playlist_from_video_id` (the "watch next" list) |
| History | playing a track reports it (`add_history_item`) so recommendations stay current |

Requirements on Fedora: `sudo dnf install yt-dlp nodejs` (or Deno).

## ytmapi-rs notes (0.3.3)
- IDs are newtypes; convert with `YoutubeID::from_raw` / `get_raw`.
- Playlist **browse** ids need the `VL` prefix for reading (`get_playlist_tracks`), editing uses the raw id.
- "Liked Music" is the playlist `VLLM` (hidden from the library playlist list, added back by Listen).
- Simplified queries return only the first page; use `yt.stream(&Query)` to collect all pages.
- ytmapi-rs's playlist and library-song parsers fail the *whole* list when one entry lacks a field (e.g. an artist
  without a link). Listen reads song lists itself (`home::song_page`): tolerant, all continuation pages, both
  continuation styles.
- Tracks = library songs + Liked Music. Albums = saved albums + the albums of those songs.
- Durations are strings (`"3:45"`), years are strings, search results carry a single artist string without ids.
- Album tracks have no artists/thumbnails (the album's are used); artist top songs have no duration.
- There is no home-feed query; Listen sends `browse FEmusic_home` itself and parses the shelves (`home.rs`):
  Quick picks (songs), mixes/playlists and albums, plus two continuation pages.
- Artists = artists of library songs (all pages) + subscribed artists.
- Uploaded music shows as an "Uploads" list in Playlists (only when you have uploads). Whether yt-dlp plays upload ids is untested (needs a real account).
- Song/video switch (player bar, YTM songs only, off by default): a second mpv window shows the music video (counterpart id from the `next` endpoint, else the song's own video), muted, kept within 0.7 s of the audio.

## Risks
Unofficial API use: YouTube changes break it every few weeks. Update yt-dlp (`sudo dnf upgrade yt-dlp`) first when
playback fails; library errors usually need a new `ytmapi-rs`. Opt-in live checks:
`LISTEN_YTM_LIVE=1 cargo test -p listen-ytmusic --test live_ytdlp`.
