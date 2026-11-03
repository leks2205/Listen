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

Scrobbling reports your plays to the music server and, once connected, straight to Last.fm (see [Last.fm](#lastfm)). It is under **Settings → General → Scrobble**.

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

### Last.fm

Listen can scrobble to Last.fm itself, for every source: YouTube Music, Navidrome, Jellyfin and local folders alike. It uses the same rule as above for when a play counts, and shows the song as "now playing" on your profile while it plays.

1. You need a Last.fm API account (free, made with your normal Last.fm login at [last.fm/api/account/create](https://www.last.fm/api/account/create)). Under **Settings → General → Scrobble → Last.fm**, enter its **API key** and **Shared secret**.
2. Press **Connect to Last.fm**. Your browser opens Last.fm; allow access there. Listen notices it within a few seconds and shows **Connected as** your user name.
3. **Disconnect** forgets the sign-in. **Scrobble to Last.fm** pauses sending without signing out.

The shared secret and the sign-in are kept with your server passwords (keyring), not in the settings file.

**One name per song.** The same song is often spelled differently by different sources: "Ditto (Official Audio)" on YouTube Music, "Ditto" in your tags; "뉴진스 (NewJeans)" or "NewJeans"; "OMG - Single" or "OMG"; an artist's Korean and international name. Last.fm would count each spelling as a track of its own. Before a play is sent, Listen asks Last.fm which spelling of the artist, the title and the album most people use, and sends that one: upload labels such as "(Official Audio)" or "[MV]" go, " - Single" and " - EP" go, and an artist's other name is used when Last.fm itself ties the two together (same MusicBrainz id, or the artist's Last.fm biography names the other). The album is the one Last.fm files the track under, so the same song gets the same album from every source. The answers are kept for 30 days in `lastfm/lastfm-names.json` in Listen's data folder.

**Offline.** Plays go into `lastfm/lastfm-queue.json` first and are sent in batches of up to 50. Plays made offline or while Last.fm is down are sent later (Last.fm accepts plays up to 14 days old). A short bridge stretch of the DJ is never scrobbled, and nothing is sent in private mode.

If you scrobble to Last.fm from Listen, unlink Last.fm in Navidrome (user menu → Personal → Scrobble to Last.fm off); otherwise Navidrome songs arrive twice.

Nothing is scrobbled in **private mode**. Turn it on under **Settings → Window → Private mode** (or in the application menu). In private mode nothing is scrobbled, shown on Discord or saved to the local history, and the window title says so. It switches off again when Listen restarts.
