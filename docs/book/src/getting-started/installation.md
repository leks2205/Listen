# Installation

This page explains how to install Listen on Windows, Fedora and other Linux distributions, and how to try it without a server. Download all packages from the project's **Releases** page on GitHub (the latest release is the one marked "Latest").

## Windows 10 and 11

1. Download `Listen-Setup-<version>.exe` and run it.
2. Windows SmartScreen may say the app is unrecognised, because the installer is not code-signed yet. Click **More info**, then **Run anyway**.
3. Follow the installer. You can tick an option to create a desktop icon, and the last page offers to start Listen.

The installer installs for your user only, into `%LOCALAPPDATA%\Programs\Listen`, and needs no administrator rights. A Start menu entry is always created.

> **Note:** Some antivirus programs find it unusual that Listen can read a browser's YouTube sign-in. Listen reads only the cookies of youtube.com and music.youtube.com, only when you choose a browser for YouTube Music, and sends them nowhere but YouTube. See [YouTube Music](youtube-music.md).

### Helper programs

Listen plays through three separate open-source programs:

| Program | Used for | winget ID |
|---|---|---|
| mpv | Playing audio and video (always needed) | `shinchiro.mpv` |
| yt-dlp | Finding the audio of YouTube Music songs | `yt-dlp.yt-dlp` |
| Deno | The JavaScript runtime yt-dlp needs for YouTube | `DenoLand.Deno` |

The last step of the [first-start wizard](first-start.md) installs them for you with winget, Windows' package manager. Listen opens a console window of its own that runs `winget install` for the programs that are missing. mpv asks for administrator rights once. When winget is done, press **Check again**; Listen uses the programs after a restart.

If you skipped the wizard or something is still missing, Listen shows a **Helper programs** dialog when it starts, with **Install with winget**, **Check again** and **Later**. You can also install the programs yourself:

```powershell
winget install shinchiro.mpv yt-dlp.yt-dlp DenoLand.Deno
```

To update them, for example when YouTube Music songs stop playing, open **Settings → Advanced → Updates → Helper programs** and press **Update**. **Show** opens the dialog with the current state of each program.

> **Tip:** If winget is missing, the console window points you to "App Installer" in the Microsoft Store.

## Fedora

Install the RPM with dnf:

```bash
sudo dnf install ./listen-<version>-1.fc44.x86_64.rpm
```

Pick the file that matches your Fedora release (the `fc44` part). The RPM pulls in mpv, PipeWire's `pw-record` (for the visualizer) and the graphics libraries Listen needs. It recommends yt-dlp and Node.js for YouTube Music, plus a few optional packages (for example `udisks2` for CD ripping and `cifs-utils`). If your setup skips recommended packages, install `yt-dlp` and `nodejs` (or Deno) yourself if you use YouTube Music.

The RPM installs the program under `/usr`, adds the menu entry and icon, and lets the built-in updater update it later (see [Updates](updates.md)).

## Other Linux distributions

Download `listen-<version>-x86_64.tar.gz`, unpack it and copy three files into your home folder:

```bash
tar xzf listen-<version>-x86_64.tar.gz
install -Dm755 listen-<version>-x86_64/listen ~/.local/bin/listen
install -Dm644 listen-<version>-x86_64/io.github.leks2205.Listen.desktop ~/.local/share/applications/io.github.leks2205.Listen.desktop
install -Dm644 listen-<version>-x86_64/io.github.leks2205.Listen.svg ~/.local/share/icons/hicolor/scalable/apps/io.github.leks2205.Listen.svg
```

Make sure `~/.local/bin` is on your `PATH`. The archive also contains the licence files and the README.

Install these runtime dependencies with your package manager:

| Needed for | Package (names vary by distribution) |
|---|---|
| Playback (required) | `mpv` |
| Visualizer | `pw-record` from PipeWire (`pipewire-utils` on Fedora) |
| YouTube Music | `yt-dlp` and a JavaScript runtime, either Deno or Node.js |
| Window system | Wayland or X11 client libraries (EGL, xkbcommon, libX11, libxcb and friends); most desktops have them already |
| CD ripping (optional) | `whipper`, `flac`, `cdrdao` (see [CD ripping](../more/cd-ripping.md)) |

yt-dlp needs a JavaScript runtime to talk to YouTube. Listen finds Deno or Node in the usual places, including `~/.nvm` and `~/.local/bin`, even though desktop applications do not get your shell's `PATH`.

A tarball install in `~/.local/bin` can be updated by the built-in updater, which replaces the file in place (see [Updates](updates.md)).

## Try it without a server

To look around first, start Listen in demo mode:

```bash
listen --demo
```

Demo mode uses a small built-in library with placeholder artwork, so no server or account is needed. It does not save settings or credentials, and it does not check for updates. See [Command line](../reference/command-line.md) for the other options.

## Building from source

If you prefer to build Listen yourself, see [Building from source](../development/building.md).
