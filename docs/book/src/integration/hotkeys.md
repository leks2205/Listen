# Hotkeys

Listen has a hotkey editor with one row per action. Hotkeys work inside the Listen window; on Linux you can also make most playback actions work system-wide. Open **Settings → Hotkeys**.

## Recording a hotkey

1. Open **Settings → Hotkeys**.
2. Click the field in the row of the action. It shows "Press keys...".
3. Press the key combination you want. The field takes the first valid combination.
4. To cancel, press `Esc`. To remove a binding, click the cross at the end of the row.

Notes:

- `Mod` means `Ctrl`. Combinations can use `Ctrl`, `Alt`, `Shift` and the Super/Windows key plus one key.
- A field with a red border means the same combination is used by another action.
- Every action starts unbound except the defaults in the table below.

## Actions

"Global" means the action can be made system-wide on Linux (see below). Actions marked "No" only work while the Listen window has focus.

| Action | What it does | Default | Global |
|---|---|---|---|
| Browser back | Go back one page | none | No |
| Browser forward | Go forward one page | none | No |
| Favorite current song | Like the playing song | none | Yes |
| Unfavorite current song | Remove the like from the playing song | none | Yes |
| Toggle current song favorite | Like or unlike the playing song | none | Yes |
| Favorite previous song | Like the queue entry before the playing one | none | Yes |
| Unfavorite previous song | Unlike that entry | none | Yes |
| Toggle previous song favorite | Toggle the like of that entry | none | Yes |
| Global search | Open the command palette and search | `Ctrl+K` | No |
| List: navigate to page | Open the page of the selected row | `Ctrl+G` | No |
| List: play (default) | Play the selected rows the default way | `Enter` | No |
| List: play last | Add the selected rows to the end of the queue | none | No |
| List: play next | Play the selected rows after the current song | none | No |
| List: play now | Replace the queue with the selected rows and play | none | No |
| List: show playing song | Scroll the list to the playing song | `Ctrl+L` | No |
| Local search | Focus the search field in the sidebar | `Ctrl+F` | No |
| Navigate to home | Go to the Home page | none | No |
| Next track | Skip to the next song | none | Yes |
| Next album | Skip to the first song of the next album | none | Yes |
| Pause | Pause (does nothing when already paused) | none | Yes |
| Play | Resume (does nothing when already playing) | none | Yes |
| Play / pause | Toggle playback | `Space` | Yes |
| Previous track | Go to the previous song | none | Yes |
| Previous album | Go to the previous album | none | Yes |
| Rate 0 stars | Clear the rating of the playing song | none | Yes |
| Rate 1 star | Rate the playing song 1 star | none | Yes |
| Rate 2 stars | 2 stars | none | Yes |
| Rate 3 stars | 3 stars | none | Yes |
| Rate 4 stars | 4 stars | none | Yes |
| Rate 5 stars | 5 stars | none | Yes |
| Skip backward | Jump back by the skip amount | none | Yes |
| Skip forward | Jump forward by the skip amount | none | Yes |
| Stop | Stop playback | none | Yes |
| Toggle full-screen player | Open or close the [full-screen player](../playback/fullscreen.md) | none | No |
| Toggle queue | Show or hide the queue | none | No |
| Toggle repeat | Cycle repeat: off, all, one | none | Yes |
| Toggle shuffle | Switch shuffle on or off | none | Yes |
| Volume down | Lower the volume by one step | none | Yes |
| Volume mute | Mute or unmute | none | Yes |
| Volume up | Raise the volume by one step | none | Yes |
| Zoom in | Make the interface larger | none | Yes |
| Zoom out | Make the interface smaller | none | Yes |

Details:

- Skip backward and forward use the amounts from the player bar settings (5 and 10 seconds by default). The volume step is the volume wheel step from the same settings.
- Rating hotkeys do nothing for sources without ratings, such as YouTube Music.
- Hotkeys that act on a list (the "List:" actions) use the rows you selected in the list that is on screen. See [Lists, selection and menus](../using/lists.md).
- Window hotkeys are ignored while the command palette is open.

## System-wide shortcuts (Linux)

On Linux the editor has an extra checkbox at the end of each row for actions marked "Global". Tick it (after you have bound a key) and Listen registers the shortcut with the desktop's GlobalShortcuts portal, so it works from any window, including on Wayland. This is supported by KDE Plasma and GNOME, among others.

- The key you record is only a suggestion. The desktop owns the final choice and may show a confirmation dialog or pick another key. The **System-wide** section shows "Active in the desktop: ..." with what was really bound.
- A global action no longer fires from inside the Listen window, only from the portal, so it never runs twice.
- To change a key the desktop has already bound, use the desktop's shortcut settings (for example **System Settings → Shortcuts → Listen**).
- If the portal is missing or refuses, a red message at the top says so, and the actions fall back to working inside the window only.
- **Global media hotkeys** is the separate switch for the hardware media keys; see [Media keys and system controls](media-controls.md).
- Clearing a hotkey also removes its global flag.

## Windows

Hotkeys work while the Listen window has focus. There are no system-wide shortcuts for the editor's actions on Windows; use the hardware media keys, which work through the media flyout (see [Media keys and system controls](media-controls.md)), or the [tray menu](window.md).
