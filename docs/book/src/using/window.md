# The window

This page describes how Listen's main window is laid out: the sidebar, the title bar, the page area, the player bar and the side panels, and how the layout changes when the window gets narrow.

## Overview

In the normal layout the window has these parts:

| Part | Where | What it is |
|---|---|---|
| Title bar | Top, 30 px | Listen's own window bar (optional, see [Title bar](#title-bar)) |
| Sidebar | Left | Navigation, servers, collections and playlists |
| Page | Middle | The page you are on: home, albums, a playlist, settings, ... |
| Side panel | Right | Queue, lyrics and visualizer (optional) |
| Player bar | Bottom | Playback controls, always visible |

Listen remembers the window size, its position and whether it was maximized. On Wayland a program cannot place its own window, so only the size and the maximized state come back there. The window starts at 1600 x 950 px the first time, and it cannot be made smaller than 480 x 120 px. See [Tray, window and notifications](../integration/window.md) for tray behaviour, single-instance start and sleep prevention.

## The sidebar

The expanded sidebar has an action bar at the top and a scrollable list below it.

### Action bar

| Control | What it does |
|---|---|
| **Search** box | Opens the [command palette](search.md#the-command-palette) (`Ctrl+K`). It is a button, not a text field. |
| Collapse button | Folds the sidebar into the narrow icon rail. |
| **Menu** | Opens the [app menu](#the-app-menu). |
| Back / Forward arrows | Step through the pages you visited, like a browser. They are grey when there is nothing to go back or forward to. |

### Sections

From top to bottom the list holds:

1. **Servers**: every server you added, with its type and **Add server**. Click a server to browse it; playback of the current queue continues. A server that is not connected shows "not connected". The small remove button next to a server asks for confirmation. This section is not shown in the demo library (`listen --demo`).
2. **My Library**: the pages you chose (see [Choosing and ordering the pages](#choosing-and-ordering-the-pages)), followed by **Downloads** and, on Linux with a CD drive attached, **Rip CD** (see [CD ripping](../more/cd-ripping.md)).
3. **Collections**: saved sort and filter combinations of the Albums and Tracks pages. The section only appears when you have saved one. See [Collections](#collections).
4. **Playlists**: your playlists, with a **+** button to create one.
5. **Shared Playlists**: playlists other people made that you can see. Only shown when there are some.

Each of the last four sections has a header with a chevron; click it to fold or unfold the section. Listen forgets which sections you folded when it restarts.

### Choosing and ordering the pages

Open **Settings → General → Sidebar**. **Sidebar configuration** lists every page. Tick a page to show it, and drag the grip on its left to move it.

| Page | Shown by default |
|---|---|
| Now playing | no |
| Search | no |
| Home | yes |
| Favorites | yes |
| Albums | yes |
| Tracks | yes |
| Album artists | yes |
| Artists | yes |
| Genres | yes |
| Folders | yes |
| Playlists | no (the playlists are listed in their own section) |
| Collections | yes |
| Recently played | yes |
| Statistics | yes |
| Mixes | yes |
| Radio | yes |
| Settings | no (it is in the app menu) |

A page that the current server cannot offer is hidden even when ticked. For example, YouTube Music has no **Folders** or **Album artists**, and only Navidrome and OpenSubsonic servers have **Folders** and **Radio**. **Downloads** is always shown.

### Playlists in the sidebar

These rows are under **Settings → General → Sidebar**:

| Setting | Meaning |
|---|---|
| **Sidebar playlist list** | Show or hide the playlist sections. On by default. |
| **Playlist filter regex** | Playlists whose name matches this regular expression (case-insensitive) are hidden. A pattern that is not valid hides nothing and shows a warning. Example: `^daily mix.*` |
| **Sidebar playlist sorting** | Lets you drag playlists into your own order. Off by default; the server's order is used. While it is on, a click on a playlist still opens it, but dragging moves it. |
| **Sidebar playlist mode** | **Expanded** (cover, name and song count; the default) or **Compact** (name only). |
| **Enable folders** | Turns playlist names such as `Workout/Run` into folders. Off by default. |
| **Folder separator** | The text that separates folder levels in a name. Default `/`. |
| **Folder view** | **Tree view** (nested, collapsible folders; the default), **Single folder** (top-level folders only, each named by its whole path) or **Navigation view** (one level at a time with a back arrow). |
| **Tree indent** | Pixels each level is indented, 0 to 64, default 16. Tree view only. |
| **Tree line color** | Colour of the connecting lines as `#rrggbb`. Empty uses the theme's border colour. Tree view only. |
| **Sidebar (collapsed) navigation** | Shows the back and forward arrows in the collapsed rail. On by default. |

With folders on, a header button expands or collapses all folders at once.

You can drop songs on one of your own playlists in the sidebar to add them; see [Drag and drop](lists.md#drag-and-drop).

### Collections

A collection is a saved query: the sort, its direction, the search text and the filters of the Albums or Tracks page. To save one, set up the page the way you want it, click the bookmark button (**Collections**) in the page header, type a name under **Save these filters as...** and press **Save**.

In the sidebar, click a collection to open the matching page with that query applied. The three dots next to a collection offer **Rename** and **Delete**. See [Lists, selection and menus](lists.md#sorting-searching-and-filtering) for the controls themselves.

### The collapsed rail

When you collapse the sidebar it becomes a rail 84 px wide. It shows:

- optionally the back and forward arrows,
- a **Menu** button that opens the app menu,
- one icon with a small label for each page you enabled, plus **Downloads**, **Rip CD** (when a disc is present), **Search** and **Settings**, which the rail always shows,
- a **Collections** button that opens a menu of your collections (only if you have some),
- your playlists as small covers, under a **Playlists** heading. The chevron there hides or shows the covers and **+** creates a playlist.

You can collapse and expand the sidebar in three ways: the collapse button in the action bar, **Collapse sidebar** / **Expand sidebar** in the app menu, or by dragging the sidebar's edge (see [Resizing](#resizing)).

If you expanded the cover of the playing song into the sidebar (the arrow that appears on the cover in the player bar), the cover shows large at the bottom of the expanded sidebar, and floats next to the rail when it is collapsed. Click it to open the full-screen player. **Settings → General → Player bar → Expand album art button** turns this arrow off.

## The app menu

The **Menu** button in the sidebar opens a small menu. Its entries are:

| Entry | What it does |
|---|---|
| **Select server**, your servers, **Add server**, **Manage servers** | Switch the server you browse, add one, or jump to **Settings → General → Servers**. Not shown in the demo library. |
| **Open command palette** | Same as `Ctrl+K`. |
| **Go back**, **Go forward** | Only when the sidebar is collapsed and the layout is not compact. |
| **Collapse sidebar** / **Expand sidebar** | Not shown in the compact layout. |
| **Private mode** | Nothing is scrobbled, shown on Discord or saved in the listening history until you turn it off or restart Listen. See [History, statistics and mixes](history.md#private-mode). |
| **Settings** | Opens the settings page. |
| **Version x.y.z** | Opens the release notes in your browser. |
| **Quit** | Closes Listen completely (even when "close to tray" is on). |
| Two layout buttons | Put the queue panel in a column on the right or in a panel under the page. Shown while the queue panel is on and the layout is not compact. |

## Title bar

By default Listen draws its own 30 px window bar. Choose its look in **Settings → Window → Window bar style**:

- **Windows**: the app icon and the playing song on the left, minimize, maximize and close on the right. This is the default.
- **macOS**: round red, yellow and green buttons on the left and the title centred.
- **Native**: your desktop draws the frame and Listen shows no bar of its own.

You can drag the custom bar to move the window, double-click it to maximize or restore, and right-click it for the desktop's window menu. With a custom bar the window can be resized by dragging any edge (5 px wide) or corner (10 px). **Track info in window title** (same section) controls whether the playing song is shown in the bar and in the window title. In private mode the title says so.

## The page area

The page area shows whichever page you opened. Pages are described in [Library pages](library.md).

### Right-hand panels

The queue panel on the right (or under the page) can show three panels: the queue, the lyrics of the playing song and a visualizer strip. Open **Settings → Playback → Player sidebar panels**:

| Setting | Meaning |
|---|---|
| **Queue panel** | Show the play queue in the right sidebar. |
| **Lyrics panel** | Show the lyrics of the playing song. |
| **Visualizer panel** | Show a spectrum strip. It only captures Listen's own audio. |
| **Combine lyrics and visualizer** | Put both in one panel, with the visualizer under the lyrics. |
| **Panel order** | The order of the panels from top to bottom. |

Two settings in **Settings → Playback → Queue** change how the whole panel sits in the window:

- **Side play queue style**: **Attached** (a panel next to the page) or **Detached** (a drawer that slides in when you rest the pointer on the right edge).
- **Side queue layout**: **Horizontal** (a column on the right) or **Vertical** (a panel under the page).

The panel is hidden on the Now Playing page, because that page shows the queue itself, and in the full-screen player. The queue is described in [Player bar and queue](../playback/queue.md).

### Resizing

Drag the thin divider to resize:

| Divider | Range |
|---|---|
| Between sidebar and page | The sidebar is 260 to 400 px wide. Drag it further left, below 160 px, and it folds into the rail. Drag it back out and it opens again. |
| Between page and right panel | 300 to 900 px wide (600 px by default). |
| Above a panel under the page (vertical layout) | The height of the panel. |
| Between two stacked panels | How much room each panel gets. |

Your sizes are saved.

## Compact layout and the mini player

There is no separate mini-player window. Instead, Listen switches to a compact layout whenever the window is 768 px wide or narrower, so shrinking the window gives you a small player. There is no button for this; it follows the window width, and the layout switches back above 768 px.

In the compact layout:

- The sidebar and the side panels are gone. The page fills the window above a 90 px player bar.
- A floating **Menu** button at the bottom left of the page opens the sidebar as a 320 px drawer over the page. Click outside the drawer or press `Esc` to close it. The button is hidden when the window is less than 192 px high.
- The player bar shows the cover, the title with a three-dots song menu, and previous, play and next. The speaker button opens a vertical volume slider.
- The full-screen player switches to a phone-style layout with shuffle, repeat, queue, lyrics and video buttons.

At the smallest size, 480 x 120 px, little more than the player bar fits, which makes a compact strip for keeping music controls beside other windows.

The settings **Player bar button size**, the skip buttons and the volume options are in **Settings → General → Player bar**.

## Where to go next

- [Library pages](library.md) for what each page shows.
- [Lists, selection and menus](lists.md) for tables, grids and right-click menus.
- [Themes and colours](../appearance/themes.md) for fonts, zoom and colours.
- [Hotkeys](../integration/hotkeys.md) for keyboard shortcuts.
