# Tray, window and notifications

This page covers how Listen behaves as a desktop window: the tray icon, closing and minimising, starting hidden, running only once, remembering its size, the title bar, keeping the computer awake, song-change notifications and scrobbling. Most of it is under **Settings → Window**; scrobbling is under **Settings → General → Scrobble**.

## Tray icon

**Tray icon** (on by default) shows Listen in the system tray. On Linux this uses the StatusNotifierItem standard (KDE Plasma supports it; GNOME needs the AppIndicator extension). On Windows it is a normal notification-area icon.

Left-click the icon to show the window. Right-click for the menu:

- Play / Pause
- Next Track
- Previous Track
- Stop
- Open main window
- Quit

The tooltip shows the playing song. **Quit** from the tray ends Listen completely.

Turning **Tray icon** off hides the three options below.

## Close, minimise and start hidden

These options appear when **Tray icon** is on. All are off by default.

| Setting | Effect |
|---|---|
| **Minimize to tray** | Minimising hides the window in the tray. Click the tray icon to show it again; with this option on, a click on the icon while the window is open hides it. |
| **Close to tray** | The close button hides the window instead of quitting. Music keeps playing; use the tray menu's **Quit** to exit. |
| **Start minimized** | Listen starts with only the tray icon and no window. If no tray is available, the window opens anyway so you are never locked out. |

> **Note:** Close to tray and Minimize to tray hide the window on Linux. On Windows the tray icon and its menu work, but at the moment the close and minimise buttons behave normally there.

On Wayland, a hidden window is really closed and a fresh one is opened when you bring it back, because Wayland applications cannot hide or restore their own window. You will not notice this apart from a short flicker.

## One instance

Only one Listen runs per user. Starting it a second time (from the launcher, a file manager or the command line) does not open a second player: the running one comes to the front and the new process exits. The `--route` option of the [command line](../reference/command-line.md) is handed over the same way to open a page in the running instance.

## Window size and position

Listen remembers the size, position (where the system allows), maximised and full-screen state of the main window and restores them at the next start. It is saved shortly after you stop resizing or moving the window, and again when you close it.

If a saved position is off-screen (for example the monitor was unplugged), the window is moved back onto a display, and it is shrunk if it is bigger than the display. On Wayland, Listen cannot read or set the position, so only the size and the maximised/full-screen flags are restored. The default size is 1600 by 950.

## Title bar

**Window bar style** chooses how the top of the window looks:

| Choice | Meaning |
|---|---|
| **Windows** (default) | Listen draws its own bar with Windows-style buttons. |
| **macOS** | Listen draws its own bar with macOS-style buttons on the left. |
| **Native** | The desktop draws the normal window frame. |

The change applies immediately. **Track info in window title** (on by default) puts the playing song in the title, as "(3 / 40) Song - Artist - Listen"; a paused song starts with "Paused". With it off, the title is just "Listen". In private mode the title ends with "(Private mode)".

## Keep the computer awake

Two switches stop the computer from sleeping while music plays:

- **Prevent display sleep while playing**: keeps the screen on, and the system awake.
- **Prevent system sleep while playing**: stops the system from suspending; the screen may still turn off. This switch is on and locked while the display switch is on, because that already covers it.

Listen holds the lock only while a song is playing and releases it when you pause or stop. On Linux it uses the desktop's Inhibit portal; if the portal is missing, a warning appears in the log and nothing is held. Both are off by default.

## Song change notifications

**Settings → General → Scrobble → Song change notifications** (off by default) shows a desktop notification when the song changes, with the title, artists, album and the cover as the icon. The notification is silent. Skipping through several songs quickly shows only the one you land on, after it has played for a second. Notifications use the freedesktop notification service, so they are available on Linux.

## Scrobbling

Scrobbling reports your plays to the music server, which can pass them on to services like Last.fm. It is under **Settings → General → Scrobble**.

| Setting | Meaning | Default |
|---|---|---|
| **Scrobble** | Report plays to the server. | On |
| **Scrobble minimum mode** | When a play counts: **Percentage or seconds** (whichever comes first), **Percentage**, or **Seconds**. | Percentage or seconds |
| **Minimum scrobble percentage** | Share of the song that must be played, 25 to 90 %. Hidden in Seconds mode. | 75 % |
| **Minimum scrobble seconds** | Time that must be played, 0 to 1200 s in steps of 5. Hidden in Percentage mode. | 240 s |

Skipping ahead inside a song does not count as listening, only the time actually played counts. When a song starts, Listen also tells the server "now playing"; the scrobble itself is sent once the threshold is reached.

What happens on each source:

- **Navidrome and other OpenSubsonic servers**: the server's scrobble call (now playing, then submission). Navidrome forwards it to Last.fm or ListenBrainz if you linked an account in Navidrome.
- **Jellyfin**: a "now playing" session report, then the item is marked as played.
- **YouTube Music**: the play is added to your YouTube Music history (not for guest sessions).
- **Local folders and radio**: nothing is reported.

Nothing is scrobbled in **private mode**. Turn it on under **Settings → Window → Private mode** (or in the application menu). In private mode nothing is scrobbled, shown on Discord or saved to the local history, and the window title says so. It switches off again when Listen restarts.
