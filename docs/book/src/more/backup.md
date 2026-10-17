# Backup, logs and diagnostics

This page covers saving and restoring your settings, the log files, the diagnostics file you attach to a bug report, and the caches Listen keeps. All of it is in **Settings → Advanced**.

## Settings export and import

**Settings → Advanced → Settings backup**

- **Export settings** writes all settings to a JSON file named `listen-settings-<date>-<time>.json` in your Downloads folder. If a file with that name exists, a number is added. A message shows the full path. Keys that look like secrets (password, token, cookie, API key and similar) are left out. The optional Last.fm key for Discord covers is the exception: it is a plain setting and is exported.
- **Import settings** reads such a file. Type or paste the path (for example `~/Downloads/listen-settings.json`) and press **Import**.

Before anything changes, Listen checks every value and shows a confirmation: "N settings will replace your current ones. Servers and sign-ins are not touched." If some values could not be used, the dialog lists the first six of them, each as `setting.path: reason`, and says they keep their current value. Typical reasons are a value of the wrong type, a setting that does not exist in this version ("unknown setting"), or a secret ("secrets are not imported"). Confirm to apply the rest, or cancel to leave everything as it is.

What is covered and what is not:

| Included | Not included |
|---|---|
| Every setting in the app: appearance, playback, hotkeys, lyrics, DJ, window, integrations, CD ripping, and so on | Servers and sign-ins, passwords and tokens, the Gemini key, the MCP token, listening history, queue, playlists (those live on your servers) |

Importing works across machines and between Linux and Windows, so it is also the way to move your setup to a new computer: export, copy the file, import, then add your servers again.

If you give Listen a Feishin export here, it tells you to use **Import from Feishin** instead. See [Importing from Feishin](feishin-import.md).

> **Note:** Your servers and a few machine-specific values are kept in `config.toml`; see [Files and folders](../reference/files.md) if you want to back up the whole configuration by copying files.

## Log files

**Settings → Advanced → Logging**

Listen writes a log while it runs, so you can find out why a song did not play.

| File | Content |
|---|---|
| `listen.log` | What Listen itself does: connections, playback decisions, errors |
| `mpv.log` | What the audio player (mpv) says |
| `rip-<date>-<disc id>.log` | One per CD rip (Linux) |

The logs are in the `logs` folder of Listen's data folder: `~/.local/share/listen/logs` on Linux and `%APPDATA%\Listen\data\logs` on Windows. At each start the previous run is kept as `listen.log.1` and `mpv.log.1`, so a crash does not erase its own log. The row **Log level** shows the exact folder.

**Log level** has two choices, and the change takes effect immediately without a restart:

- **Debug** (the default) records everything Listen's own parts say. Use it when something does not work.
- **Info** records less detail and keeps `listen.log` smaller.

Cookies and stream URLs are not written to the logs, and obvious secrets are removed again when you export diagnostics.

If you start Listen from a terminal, it also prints log lines there. Set the environment variable `RUST_LOG` (for example `RUST_LOG=debug`) to change how much; see [Command line](../reference/command-line.md).

## Export diagnostics

**Settings → Advanced → Logging → Export diagnostics**

Press **Export** and Listen saves `listen-diagnostics-<date>-<time>.zip` in your Downloads folder. Attach it to a bug report; see [Troubleshooting](troubleshooting.md#how-to-report-a-bug). It contains:

| File in the zip | Content |
|---|---|
| `versions.txt` | Listen version, operating system and architecture, how Listen was installed, update channel, log level, and the first line of `mpv --version` and `yt-dlp --version` ("not found" if missing) |
| `config.toml` | Your configuration with sensitive values replaced |
| `logs/…` | The last 4 MB of each log file (`listen.log`, `listen.log.1`, `mpv.log`, `rip-*.log`, and so on) |

Before anything goes into the zip, Listen removes:

- values of keys such as password, token, secret, API key, cookie, authorization, signature, salt and credentials;
- the user name, e-mail and key fields in `config.toml`;
- the login part of URLs (`https://user:pass@host` becomes `https://***@host`);
- `Authorization: Bearer …` headers and common token formats (for example GitHub, Google API keys and Tailscale keys);
- sensitive query parameters such as `t`, `s`, `u`, `p`, `key`, `sid` and `auth`.

Server addresses, song and artist names and file paths in the logs are **not** removed, and neither is the optional Last.fm key of the Discord settings if you entered one. Open the zip and look through it before you share it, and send it only to people you trust with that information.

## Caches

**Settings → Advanced → Cache**

Caches are copies of data Listen can fetch or compute again. Clearing them is always safe: your library, playlists and settings are not touched.

| Row | What it holds | Folder in the cache directory |
|---|---|---|
| **Clear memory cache** | Covers and data kept in memory. They load again as you browse. Files on disk stay. | (memory only) |
| **Cover art cache** | Downloaded album and artist images | `covers` |
| **YouTube Music audio cache** | Songs kept so they can play gapless and replay quickly | `ytm-audio` |
| **Waveform cache** | The waveforms for the player bar slider | `waveforms` |

Each row shows how much space it uses. **Clear** asks for confirmation and then shows how many files and how much space were freed. **Clear all** clears the three disk caches at once, and **Refresh sizes** measures them again. After clearing the YouTube Music audio cache, songs you played before are fetched again when you next play them.

The cache folder is `~/.cache/listen` on Linux and `%LOCALAPPDATA%\Listen\cache` on Windows. Other folders in it (the beat-detection model, AI DJ clips, downloaded updates, CD-rip work files) are managed by Listen itself and are not listed here. The full layout is in [Files and folders](../reference/files.md).
