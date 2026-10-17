# Privacy and security

Listen has no server of its own. This page lists what leaves your computer, where it goes and why, how secrets are stored, and what you can turn off. The short version: no telemetry, no analytics and no account with the project. Listen talks only to the servers and services you set up, plus the ones listed below.

## What goes where

| Destination | When | What is sent |
|---|---|---|
| **Your music servers** (Navidrome, OpenSubsonic, Jellyfin) | Whenever you use them | Your login, browsing and search requests, stream requests, favourites and ratings, playlist changes, and play reports ("scrobbles") so the server knows what you listened to. What the server does with them (for example forwarding to Last.fm) is up to the server. |
| **YouTube Music** | Only if you add a YouTube Music source | Requests to YouTube Music for your library, search and playback, made with the session cookies of the browser you picked. The helper program yt-dlp contacts YouTube to find the audio stream. Cookies are read from your browser at each start and go only to YouTube. See [YouTube Music](../getting-started/youtube-music.md). |
| **Lyrics providers** | When lyrics are shown and lyrics fetching is on (default) | Song title and artist, for some providers also album and length, to the providers you enabled in the lyrics settings: lrclib.net and NetEase by default, Genius and SimpMusic if you add them. Lyrics stored in your files or on your server are used first if you ask for that. |
| **Translation services** | Only if you switch on lyrics translation and enter your own key | The lyrics text, to Microsoft Azure Translator or Google Cloud Translation, whichever you choose. |
| **Google Gemini** | Only with the AI DJ turned on and your own key | Song titles and artists the DJ picks from, the DJ's spoken lines and your "Details for the DJ". Outside the EU, UK and Switzerland, Google may use free-tier requests to improve its products; see [AI DJ](../dj/ai-dj.md). |
| **MusicBrainz, Cover Art Archive, iTunes** | Only when you rip a CD (Linux) | The disc's table of contents and ID to MusicBrainz; the release to the Cover Art Archive; if that has no cover, a search by album and artist to the iTunes Search API. See [CD ripping](cd-ripping.md). |
| **Last.fm** | Only if you enter a Last.fm API key in the Discord settings | Artist and album, to look up a cover for your Discord status. |
| **Discord** | Only with **Discord Rich Presence** on (Settings → Window) | Song title, artist and album, play state and time, shown on your Discord profile, through the Discord app running on your computer. A server cover is sent only if you enable that option and your server is on a public address. See [Discord status](../integration/discord.md). |
| **GitHub** | Update checks (default: at start), and when you press **Check now** or install an update | A request for the project's public releases. No account and no personal data besides what any web request reveals (your IP address and the Listen version in the user agent). Downloads are verified against the release's `SHA256SUMS`. Listen also downloads the beat-detection model for [song analysis](../dj/analysis.md) once, about 11 MB, from GitHub. |
| **Artist link buttons** | When you click one | Opens the browser at Last.fm, MusicBrainz, ListenBrainz, Qobuz or Spotify. Listen itself sends nothing. |

Anything not in this table does not leave your computer. Music from your servers goes straight to your computer, not through anyone else.

You can turn off lyrics fetching, update checks, Discord, the AI DJ, song analysis and CD ripping individually in Settings.

## What stays on your computer

- **Listening history.** Songs, server and time are saved in a file on your computer and never uploaded or shared. Switch **Record listening history** off or use **Don't record this session** in **Settings → Advanced → Listening history**. You can export or delete the history there. See [History, statistics and mixes](../using/history.md).
- **Song analysis results** (tempo, key, sections) are stored locally, or in a folder you choose and sync yourself.
- **Settings, queue and caches** are plain files in Listen's folders; see [Files and folders](../reference/files.md).

## Passwords, tokens and keys

Passwords, tokens and API keys are not stored in `config.toml`. They go to:

1. The **system keyring**: Secret Service (for example KDE Wallet or GNOME Keyring) on Linux, Windows Credential Manager on Windows.
2. A **credentials file**, `credentials.toml` in the config folder, when no keyring is available or it refuses a value. On Linux only your user can read it (permissions 0600); on Windows it lives in your user profile.

**Settings → General → Servers → Password storage** shows which is in use and lets you choose **System keyring** or **File only**. Switching moves the stored secrets. The file stores secrets in plain text, protected only by the permissions of your account, so prefer the keyring when you have one. If the keyring does not work, see [Troubleshooting](troubleshooting.md#keyring-unavailable).

The Gemini key, the MCP token and the remote-control password are stored the same way. The optional Last.fm key and the lyrics translation key are plain settings in `config.toml`; keep that in mind before you share the file.

## Logs and diagnostics

Log files stay on your computer. Cookies and stream URLs are not written to them. When you export diagnostics, Listen also removes passwords, tokens, cookies, keys and the login part of URLs. Server addresses and song names in the logs are not removed. Nothing is uploaded; you decide whom to send the zip to. Details: [Backup, logs and diagnostics](backup.md).

## Letting other programs and devices in

Listen has two small servers, both off by default. Turn on only what you use.

| Feature | Address | Protection |
|---|---|---|
| **Remote control** (phone page and Feishin-compatible remote) | Port 4333, by default on every network interface | Optional user name and password. **With both empty, anyone on your network can control Listen.** Set a password or change the address to `127.0.0.1`. |
| **MCP server** (AI assistants such as Claude Code or Claude Desktop) | `http://127.0.0.1:4334/mcp`, this computer only unless you switch on **Allow other computers** | A secret token that every request must carry. Anything holding the token can control Listen fully, including settings, your playlists on your servers and your Gemini quota. **New token** disconnects all clients. |

Neither server returns passwords, cookies, API keys or the token itself. See [Remote control](../integration/remote.md) and [MCP server](../integration/mcp.md). Neither should be exposed to the internet.

## Single instance and local sockets

Listen runs once per user. A second start talks to the first through a local socket (Linux: in `$XDG_RUNTIME_DIR`) or lock file (Windows) and exits. The audio player mpv is controlled through a private local socket. Neither is reachable from the network.

## Reading browser cookies (YouTube Music)

To sign in to YouTube Music, Listen reads the cookies of `youtube.com` and `music.youtube.com` from the browser profile you pick, and only then. It stores which profile and channel to use, not the cookies. Because this looks unusual to some antivirus programs, they may warn about Listen on Windows; see [Troubleshooting](troubleshooting.md#windows-smartscreen-or-antivirus-warns-about-listen).

YouTube Music support is unofficial and not affiliated with Google or YouTube. You use your own account at your own risk.

## Reporting a security problem

If you find a security issue, report it privately to the maintainers through the project's GitHub page rather than in a public issue.
