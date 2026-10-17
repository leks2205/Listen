# Media keys and system controls

Listen tells your desktop what is playing and accepts commands back, so media keys, lock-screen style widgets and panel applets work without Listen having focus. On Linux this uses MPRIS; on Windows it uses the System Media Transport Controls (the media flyout).

## Linux: MPRIS

Listen registers itself on the D-Bus session bus as an MPRIS media player named **Listen**. Anything that speaks MPRIS can show and control it: the KDE Plasma and GNOME media widgets, panel applets, `playerctl`, KDE Connect, and so on.

What the desktop sees:

- Title, artists, album, album artist, track number, length and the cover.
- Genre, disc number, BPM, comment, year, last played, play count and rating when the song has them.
- The current lyrics (synchronised lyrics as LRC text) in the `xesam:asText` field.
- Playback state, position, volume, shuffle and repeat (none, track, playlist).

What the desktop can do: play, pause, play/pause, stop, next, previous, seek (relative and absolute), set the volume, switch shuffle and repeat, and raise the Listen window. Listen cannot be quit over MPRIS, and it does not expose a track list.

The player appears as soon as Listen starts. If the session bus is not available, Listen logs a warning and carries on without it.

## Windows: media flyout

On Windows, Listen publishes to the System Media Transport Controls. That gives you:

- The media flyout (volume popup) and the lock screen with title, artist, album and cover.
- Hardware media keys and keyboard media keys, and the media buttons on headsets and Bluetooth devices.
- Play, pause, next, previous, stop, seeking and the position slider. The fast-forward and rewind buttons jump 10 seconds; the position slider sets an exact position.

Listen keeps a hidden helper window for this, so the controls keep working while the main window is hidden in the tray.

## Media keys

| Platform | How the keys reach Listen |
|---|---|
| Windows | Through the media flyout (SMTC). Nothing to set up. |
| Linux with KDE Plasma, GNOME, Cinnamon or Budgie | The desktop sends the keys to the running MPRIS player. Nothing to set up. |
| Other Linux desktops (for example sway or Hyprland) | Listen asks the desktop portal to grab the keys itself. See below. |

### Global media keys through the portal (Linux)

**Settings → Hotkeys → System-wide → Global media hotkeys** (on by default) lets Listen register these keys with the desktop's GlobalShortcuts portal:

| Shortcut | Preferred key | Action |
|---|---|---|
| Media key: play / pause | `XF86AudioPlay` | Play / pause |
| Media key: next track | `XF86AudioNext` | Next track |
| Media key: previous track | `XF86AudioPrev` | Previous track |
| Media key: stop | `XF86AudioStop` | Stop |

Listen only registers these on desktops that do not already route the keys over MPRIS. On KDE Plasma, GNOME, Cinnamon and Budgie the setting has no effect, because binding the keys twice would make every press fire two times. Turn the setting off if you want only MPRIS to control Listen.

The portal treats the keys above as suggestions. The desktop has the final say, and may ask you to confirm or may bind a different key. The Hotkeys tab shows what the desktop actually assigned under "Active in the desktop". To change a key the desktop already bound, open the desktop's own settings (for example **System Settings → Shortcuts → Listen**).

If your desktop has no GlobalShortcuts portal, the Hotkeys tab shows "The desktop refused system-wide shortcuts" with the reason. MPRIS keeps working.

See [Hotkeys](hotkeys.md) for the other system-wide shortcuts.

## Troubleshooting

- Media keys do nothing on Linux: check that another player is not grabbing them, and that your desktop supports MPRIS or the GlobalShortcuts portal.
- On Windows the flyout shows an old song: it updates when the song changes; pausing and resuming refreshes it.
- A key fires twice on a non-listed desktop: turn **Global media hotkeys** off and let MPRIS handle it.
