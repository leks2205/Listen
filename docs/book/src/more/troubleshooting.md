# Troubleshooting

Answers to the problems people run into most. Each entry says what to check and, where it helps, which log to look at. The location of the logs is in [Where are the logs?](#where-are-the-logs) and the steps for a bug report are at the end.

## YouTube Music songs stop playing

YouTube Music support is unofficial. It relies on the separately installed program **yt-dlp** to find the audio stream, and YouTube changes its site often enough that an old yt-dlp stops working. The most common cure is to update the helper programs:

- **Windows:** open **Settings → Advanced → Updates → Helper programs** and press **Update**. This updates mpv, yt-dlp and Deno with winget. **Show** lists what is installed.
- **Linux:** update yt-dlp with the way you installed it, for example `sudo dnf upgrade yt-dlp`, or `pipx upgrade yt-dlp`, or `yt-dlp -U` for a standalone copy. Check with `yt-dlp --version`.

Then try the song again. If it still fails:

- yt-dlp needs a JavaScript runtime for YouTube. Install **Deno** or **Node.js** (Linux: `sudo dnf install nodejs`). The message "yt-dlp … needs a JavaScript runtime" means it is missing.
- Run `listen --check-youtube-music` in a terminal. It signs in the way the app does, prints what Listen sees of your library (counts only) and tells you which step fails. See [Command line](../reference/command-line.md).
- Look at `listen.log` (see below) for lines about yt-dlp.
- If the yt-dlp program lives in an unusual place, set the environment variable `LISTEN_YTDLP` to its full path.

YouTube may also restrict accounts it sees accessed by unofficial clients, or change things so that Listen has to be updated. See [YouTube Music](../getting-started/youtube-music.md) for what to expect.

## YouTube Music sign-in says the browser is locked

On Windows a running browser keeps its cookie file locked. Close the browser completely (check the system tray and Task Manager for background processes) and pick it again in Listen. Listen reads the cookies when you select the browser, so you can start it again right afterwards.

## Chrome, Edge or Brave on Windows cannot be used

Recent versions of Chrome, Edge and Brave on Windows protect their cookies with app-bound encryption, which only the browser itself can read. Listen then says "this browser protects its cookies with app-bound encryption (v20)". Use Firefox instead, or choose **Other ways to connect → cookies file** and load a cookie export of `music.youtube.com` made with a browser extension. Listen's login page also accepts such a file dropped onto it.

## No sound, or "mpv" is missing

Listen plays through **mpv**.

- If you see "could not start mpv; is the `mpv` package installed?" or "The audio player (mpv) stopped unexpectedly", install mpv. Linux: `sudo dnf install mpv` (the Fedora package of Listen already depends on it). Windows: the helper programs dialog installs it, or run `winget install shinchiro.mpv`.
- If mpv is installed somewhere unusual, enter its path in **Settings → Playback → mpv → Executable path** and press **Reload**. The environment variable `LISTEN_MPV` does the same for one run.
- Check the output device in **Settings → Playback** (the audio device list) and try **System default**.
- **Extra parameters** you or an imported Feishin file added can break mpv. Clear them and press **Reload mpv**.
- Look at `mpv.log` next to `listen.log`.

## Windows SmartScreen or antivirus warns about Listen

The installer is not code-signed yet, so SmartScreen says the app is unrecognised. Click **More info → Run anyway**. Some antivirus programs also flag Listen because it can read a browser's YouTube sign-in; it only reads the cookies of `youtube.com` and `music.youtube.com`, only when you pick a browser, and sends them only to YouTube. If your antivirus quarantines the program, restore it and add an exception. You can check the download against the `SHA256SUMS` file on the project's releases page.

## Keyring unavailable

**Settings → General → Servers → Password storage** says where your passwords are kept. If no Secret Service keyring is running (typical on minimal desktops and some window managers), Listen falls back to a private `credentials.toml` in its config folder and logs "keyring unavailable". Everything keeps working. To avoid the warning or to decide yourself, install and start a keyring (KDE Wallet with the Secret Service interface, GNOME Keyring or KeePassXC), or choose **File only**. Switching moves your stored passwords. If a server keeps asking you to sign in again after a restart, the keyring probably refused to give the password back; choose **File only** to rule that out.

## Global shortcuts and media keys on Linux

- Media keys normally work through MPRIS. KDE Plasma, GNOME, Cinnamon and Budgie send them to Listen themselves; **Global media hotkeys** is only needed on other desktops.
- Global hotkeys use the desktop's shortcut portal (KDE Plasma and GNOME). The desktop has the last word: it may ask you to confirm the keys, may pick other keys than the ones you suggested, and shows them in its own shortcut settings. If it refuses, the **Hotkeys** page says "The desktop refused system-wide shortcuts" with the reason. Make sure the portal for your desktop (for example `xdg-desktop-portal-kde`) is installed and running, then restart Listen.
- On **Wayland**, apps cannot read or set their own window position, so Listen does not restore it and starts where the compositor puts it. Hiding to the tray closes the window and opens a new one when you bring it back. If you need X11 behaviour, start Listen with `WINIT_UNIX_BACKEND=x11`.
- In the tray, if your desktop has no tray (some GNOME setups need an extension), Listen starts with a window instead of hidden.

See [Hotkeys](../integration/hotkeys.md) and [Media keys and system controls](../integration/media-controls.md).

## The visualizer shows nothing

On Linux the visualizer listens to Listen's own audio through PipeWire (`pw-record` and `pw-dump`). Check that:

- PipeWire and its tools are installed (Fedora: `pipewire-utils`) and that `pw-record --version` works. With plain PulseAudio or ALSA there is nothing to capture.
- Something is playing. The bars follow Listen's output only, not other programs.
- For the Milkdrop style, the projectM library is installed; without it only the bar styles are available. If Milkdrop shows a black picture, set `LISTEN_MILKDROP_EGL` to `device`, `surfaceless` or `default` to force one way of creating the graphics context.

For a detailed trace set `LISTEN_VIZ_DEBUG=1` and read `listen.log`. See [Visualizer and Milkdrop](../playback/visualizer.md).

## The AI DJ stopped talking or says the quota is used up

The AI DJ uses your own Gemini key, and the free tier allows only a limited number of spoken breaks per day. When Google reports the quota as used up, the DJ keeps choosing songs and shows its lines as text, and tries speech again within the hour. The AI DJ settings show "N of M spoken breaks left today". The counter resets once a day, at midnight Pacific Time.

To get more speech, let the DJ talk less often (**Talk after**, **Songs per set**), lower **Spoken breaks per day**, or use a key from a project with billing enabled. If the DJ is unavailable altogether, check that the key is saved (press **Save** again, Listen checks it), that you are online, and that the key has not been deleted in Google AI Studio. `listen --check-ai-dj` runs two test rounds from a terminal. See [AI DJ](../dj/ai-dj.md) and [Creating a Gemini API key](../dj/gemini-key.md).

## Song analysis is slow or uses too much CPU

Analysis (tempo, key and sections for the DJ features) runs in the background at low priority. A large library takes hours the first time, and the results are kept, so it only happens once. To speed it up or slow it down, open the analysis settings in **Settings → Playback**:

- **Whole library** off: only the queue is analysed.
- **Also on battery** off (the default): analysis pauses when your laptop is unplugged.
- The number of analysis threads is not in the settings window. It is `threads` under `[settings.analysis]` in `config.toml`: `0` (the default) uses about a quarter of your processor cores, any other number sets the count, up to the number of cores. Restart Listen after changing it.
- **Analysis folder**: results are stored in Listen's data folder unless you pick a (synced) folder. A folder on a slow network share slows things down.

The first run also downloads a small model file (about 11 MB). Without internet access Listen uses a simpler built-in tracker. You can also run `listen --analyze-library --jobs 4` in a terminal to analyse the Navidrome library outside the app. See [Song analysis](../dj/analysis.md).

## CD ripping does not start

Run `listen --check-cd`. It names the missing tools (whipper, flac, metaflac, cd-paranoia, cdrdao), a drive that is not found, an unknown read offset, a music folder that is not mounted or not writable, and an unreachable Navidrome server. See [CD ripping](cd-ripping.md).

## Lyrics, covers or a server do not load

- Self-signed certificate on your server: edit the server and turn on **Ignore certificate errors**, which applies to that server only.
- Wrong time or a proxy can also break secure connections.
- Press **Clear all** under **Settings → Advanced → Cache** to drop broken cached images.
- Look in `listen.log` for the failing address.

## Listen is "already running"

Listen allows one copy per user. A second start brings the first window forward and exits. If the first one is hidden in the tray, use the tray icon. If you see no window and no tray icon, end the `listen` process in your task manager or with `pkill listen` and start again.

## Where are the logs?

In the `logs` folder of Listen's data folder:

| System | Folder |
|---|---|
| Linux | `~/.local/share/listen/logs/` |
| Windows | `%APPDATA%\Listen\data\logs\` |

**Settings → Advanced → Logging** shows the exact path. `listen.log` and `mpv.log` are rewritten at each start; the previous run is kept as `listen.log.1` and `mpv.log.1`. More in [Backup, logs and diagnostics](backup.md).

## How to report a bug

1. Reproduce the problem once with **Settings → Advanced → Logging → Log level** set to **Debug** (the default).
2. Right after it happened, open **Settings → Advanced → Logging** and press **Export diagnostics**. Do this before restarting Listen if you can, so the log of the failing run is the current one (a restart moves it to `listen.log.1`, which is also included).
3. Open the zip and read through it. Passwords, cookies and tokens are removed, but server addresses and song names are not.
4. Open an issue on the project's GitHub page. Say what you did, what you expected and what happened, your Listen version (**Settings → About**), your operating system and desktop, and attach the zip.
5. For YouTube Music problems also include the output of `listen --check-youtube-music` and `yt-dlp --version`.
