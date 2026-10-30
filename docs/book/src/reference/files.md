# Files and folders

This page lists where Listen keeps its files on Linux and Windows: settings, history, caches, logs, your own themes and presets, and the program's helper files. Listen stores everything in your user account; it never writes outside your home or profile except to the music folder you choose for CD ripping.

## The three base folders

Listen uses the standard per-user folders of your system, under the application name `Listen`.

| Folder | Linux | Windows |
|---|---|---|
| **Config** | `~/.config/listen` | `%APPDATA%\Listen\config` |
| **Data** | `~/.local/share/listen` | `%APPDATA%\Listen\data` |
| **Cache** | `~/.cache/listen` | `%LOCALAPPDATA%\Listen\cache` |

On Linux the folders follow `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` when you have set them. On Windows `%APPDATA%` is usually `C:\Users\<you>\AppData\Roaming` and `%LOCALAPPDATA%` is `C:\Users\<you>\AppData\Local`.

Rule of thumb: **config** is what you would want to keep, **data** is what Listen has learnt or saved for you, and **cache** can be deleted at any time.

## Config folder

| Path | Content |
|---|---|
| `config.toml` | All settings, your list of servers (name, kind, address, user name), per-server options and the setting for password storage. Contains no passwords. Written atomically, so a crash cannot leave it half-written. |
| `credentials.toml` | Passwords, tokens and keys, only when the system keyring is not used or not available ("File only" or no keyring). On Linux readable by your user only (permissions 0600). Delete a server in Listen to remove its entry. |
| `themes/` | Your own themes. See [Custom themes](../appearance/custom-themes.md). |

If `config.toml` cannot be read because it is damaged, Listen starts with default settings and logs a warning. Keep a copy, or use **Export settings** (see [Backup, logs and diagnostics](../more/backup.md)).

## Data folder

| Path | Content |
|---|---|
| `logs/` | `listen.log`, `mpv.log` (and the previous run as `.log.1`) and `rip-*.log` from CD rips. |
| `history.jsonl` | Your listening history, one line per play. Local only. |
| `queue.json` | The play queue, restored at the next start. |
| `analysis/` | Song analysis results (tempo, key, sections), unless you set another **Analysis folder** in the settings. |
| `offline/` | Songs you downloaded for offline playback from your own servers, with an index. The folder is private to your user. See [Downloads](../using/downloads.md). |
| `milkdrop/presets/` | Milkdrop presets (`.milk` files) when you have not chosen another preset folder. |
| `lyrics_overrides.json` | Which lyrics match you picked for songs. |
| `ai-dj-quota.json` | The count of AI DJ spoken breaks voiced today, and whether Google reported the speech quota as used up. |
| `rips.json` | Discs you have ripped, including those listed under **Rips to fix**. |

## Cache folder

Everything here is created again when needed.

| Path | Content | Cleared by |
|---|---|---|
| `covers/` | Downloaded album and artist images | **Settings → Advanced → Cache → Cover art cache** |
| `ytm-audio/` | Audio kept for YouTube Music songs so they can play gapless and replay quickly; limited to about 500 MB | **YouTube Music audio cache** |
| `waveforms/` | Waveforms for the player bar slider | **Waveform cache** |
| `models/` | The beat-detection model for song analysis (about 11 MB), downloaded once | not listed; delete the folder to fetch it again |
| `dj/` | Clips of the AI DJ's spoken breaks | not listed |
| `updates/` | Downloaded update files | not listed |
| `rips/` | Work files of CD rips (Linux) | not listed |

## Other places

| Path | Used for |
|---|---|
| `~/Downloads` (your system's Downloads folder) | Where exports go: settings (`listen-settings-<date>.json`), diagnostics (`listen-diagnostics-<date>.zip`) and history (JSON or CSV). |
| Your music folder (default `~/Music`, set in **Settings → Advanced → CD ripping**) | Ripped albums as `Album Artist/Album/NN Title.flac`, with a hidden `.listen-incoming` folder while copying. |
| `~/.config/whipper/whipper.conf` | whipper's own configuration, including the drive offset found by **Verify offset**. Linux only. |
| `$XDG_RUNTIME_DIR/io.github.leks2205.Listen.sock` | Local socket through which a second start reaches the running Listen (Linux). The file `io.github.leks2205.Listen.lock` plays this role on Windows. Without `XDG_RUNTIME_DIR` a private folder `listen-<user>` in the temp folder is used. |
| `$XDG_RUNTIME_DIR` | Also holds the private socket of the audio player (mpv). |
| `%LOCALAPPDATA%\Listen\bin` | Helper programs (mpv, yt-dlp, Deno) when they are installed there. Listen looks for helpers next to `listen.exe`, then on the `PATH`, then here and in winget's folders. |
| `%LOCALAPPDATA%\Programs\Listen` | The Windows installation (installer without administrator rights). |
| Your KDE colour scheme and theme-switcher files | Read, never written, when **Follow desktop theme** is on (Linux). |

## Backing up and moving

- To move your setup to another computer, use **Export settings** and **Import settings**, then add your servers again. See [Backup, logs and diagnostics](../more/backup.md).
- To back up everything Listen knows, copy the config and data folders while Listen is closed. Passwords stored in the keyring are not in these folders; they stay with your user account's keyring.
- To start fresh, close Listen and rename the three folders. Listen creates new ones at the next start.
- To remove all traces, delete the three folders and, if used, the entries for `io.github.leks2205.Listen` in your keyring.

> **Note:** The application ID `io.github.leks2205.Listen` is also the name of the desktop file, the icon and the Wayland application ID.
