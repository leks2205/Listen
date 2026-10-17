# Lists, selection and menus

Almost every page in Listen shows a list of songs, albums, artists, playlists or genres. They all share the same component, so they all look and behave the same way. This page explains the displays, columns, sorting and filters, how selection works, what the keyboard and mouse do, drag and drop, and the right-click menus.

## Displays

A list can be shown in up to three ways. When a list offers more than one, two or three small buttons appear in its page header (**Table view**, **Grid view**, **Detail view**).

| Display | What it looks like | Available for |
|---|---|---|
| **Table** | One row per item with columns | All lists |
| **Grid** | Cards with a cover and a few lines of text | Albums, artists, playlists |
| **Detail** | Every album as a block with its cover, facts and track list | Albums |

Song lists, genres, the queue and search results for tracks are always tables.

By default the Albums, Artists, Album Artists and Playlists pages start as a grid. Every other list starts as a table.

The look of a list (display, columns, sizes) is saved per kind of list, so all playlists share one look, all album grids share one look, and so on. The tracks of an album, the songs of a playlist, the queue panel and the Now Playing page each have their own.

## List options

The sliders button in a page header opens **List options**. What it contains depends on the display.

**Display** switches between table, grid and detail (when more than one is offered).

### Table options

| Option | Meaning |
|---|---|
| **Row size** | **Compact** (40 px), **Default** (64 px) or **Large** (88 px) rows. |
| **Show header** | The row of column titles. |
| **Highlight the row under the pointer** | On by default. |
| **Alternate row colors** | Zebra stripes. |
| **Horizontal borders**, **Vertical borders** | Lines between rows and columns. |
| **Fit columns to the width** | On by default: the columns stretch to fill the width. When off, they keep their width and the table scrolls sideways if needed. |
| **Columns** | See below. |

### Columns

Under **Columns** every column the list can show has a line with:

- a checkbox to show or hide it,
- up and down arrows to move it,
- **L** and **R** to pin it to the left or right edge, so it stays in view when the table scrolls sideways,
- an alignment button (left, centre, right),
- **Fit**, which makes the column as wide as its longest value.

You can also work directly in the table header: drag a column title onto another to reorder, and drag the right edge of a title to change the width.

The columns available for songs are: album group, number (`#`), image, title (three variants: name only, title and artist, or cover with title and artist), duration, album, album artist, artist, composer, date, genre, genre badges, year, release year, release date, disc, track, bit depth, bitrate, codec, sample rate, last played, comment, channels, BPM, date added, path, plays, size, favorite, rating and actions. Albums, artists, playlists and genres have shorter sets (for example, albums offer song count, release type and last played; playlists offer owner and duration). A column that no source can fill is not offered, and a column the server has no data for stays empty.

The default song table shows the number, cover with title and artist, duration, album, genre, year and a heart. The songs of an album show track number, title, duration and heart in compact rows.

The **Album group** column groups the rows of a song table by album under a header with the cover, facts, a favorite heart and a rating. When you enable it, an **Album group** section appears in the options: the cover size (**No cover**, 64 to 160 px), which facts to show (album artists, release date, release year, song count, duration, release type, genres, size), **Favorite and rating** and **Cover above the facts**.

### Grid options

| Option | Meaning |
|---|---|
| **Card size** | **Compact** (cover only, with the name on a strip at its bottom), **Default** or **Large** (fewer, bigger cards). |
| **Gap** | Space between cards, from `xs` (10 px) to `xl` (32 px). `sm` (12 px) is the default. |
| **Cards per row** | Tick it to fix the number of cards per row (1 to 20). Otherwise the number follows the window width. |
| **Lines under a card** | Which lines to show under each cover and in which order, with an alignment button each. For albums the default is name, album artist and release year. |

Hovering a card shows round buttons on the cover: play, **Play next**, **Play last**, the heart, **More options** and, for albums, a chevron that expands the album's track list in a panel under the row.

### Loading

**Infinite scroll** (the default) loads more rows as you scroll. **Pages** shows a fixed number of rows and a bar at the bottom ("1-100 of 1250", **Previous page**, "Page 1 of 13", **Next page**). **Items per page** is 100 by default and changes in steps of 25.

**Reset to defaults** puts the list back as it was.

## Sorting, searching and filtering

The Albums page, the Tracks page and the albums and tracks of a genre have a header with these controls:

- **Sort by** opens a menu of sort fields. The fields depend on the server: Navidrome offers the most, plain Subsonic servers and local folders offer a few. Tracks can be sorted by album, album artist, artist, BPM, duration, favorited, genre, name, play count, rating, recently added, recently played, release year and more.
- The arrow button toggles ascending and descending order. Each sort field starts in its natural direction (for instance, "Most played" starts descending).
- **Filters** opens **Album filters** or **Track filters**. See below.
- **Refresh** loads the list again.
- **Collections** (Albums and Tracks pages) lists your saved queries and lets you save the current one.
- **Search** filters by text after you stop typing for a moment (about 0.4 seconds). The cross button clears it.
- **Play** and **Shuffle play** play everything the current sort, search and filters match, up to 20,000 songs. Hold **Play** for half a second to play it shuffled.

Other lists (artists, genres, playlists, search results) have their own simpler controls, described on their pages. In table views the column titles do not sort; use **Sort by**.

### Filters

The filter window offers the filters the server supports, so you may not see all of these:

| Filter | Meaning |
|---|---|
| **Favorited** | **Any**, **Yes** or **No**. |
| **Rated** | **Any**, **Yes** or **No**. |
| **Compilation** | Albums only. |
| **Recently played** | Albums only. |
| **Artists** | Pick one or, with **Multiple**, several artists. |
| **Genres** | Pick one or, with **Multiple**, several genres. |
| **Year** | A single year or a **From** and **To** range. |
| **Tags** | The server's custom tags, with a list of values to pick from. |

On plain Subsonic servers only one of **Favorited**, **Artists**, **Genres** and **Year** can be used at a time; the others show "Off while another of these filters is on". **Reset** clears all filters, **Done** closes the window. The filter button lights up while a filter is active.

Listen remembers the sort, order, search and filters of the Albums and Tracks pages and of genre pages between sessions.

### Collections

To save the current sort, order, search and filters as a collection, click **Collections**, type a name in **Save these filters as...** and press **Save**. A saved collection appears in the same menu and in the sidebar. Click it to apply it. The bin button in the menu deletes it; in the sidebar the three dots offer **Rename** and **Delete**.

## Selection and clicking

Listen's selection rules differ a little from Feishin's, so they are worth knowing.

| Action | Result |
|---|---|
| Click on a row or card | Opens the item. A song opens its **album**; an album, artist, playlist or genre opens its own page. The click waits about a third of a second so that a double-click is not mistaken for it. A click also clears the selection. |
| Double-click | Songs play. Albums, artists, playlists and genres open. |
| `Ctrl`+click | Adds or removes one item from the selection. |
| `Shift`+click | Selects the range from the last item you selected to this one. |
| Right-click | Opens the [context menu](#context-menus). If the item is not selected, it becomes the only selected item first. |
| Click the play button in the `#` column | Plays that row. |

When something is selected, a bar at the bottom of the list shows "3 selected" with **Clear** and **More**. **More** opens the context menu for the whole selection.

A double-click on a song plays it and, with the default **Play button behavior** of **Play now**, queues up to 50 songs before and after it from the same list, so playback continues through the list. The queue panel itself behaves differently: a double-click there jumps to that song without replacing the queue.

In a grid the same rules apply to the whole card. If you turn off **Settings → General → Lists → Grid multi-select**, a click on the cover opens the item immediately.

### Play button behavior

**Settings → General → Lists → Play button behavior** decides what the main Play buttons, a double-click on a song and the list's play key do:

| Choice | Effect | Button label |
|---|---|---|
| **Play now** | Replaces the queue and starts playing. | **Play** |
| **Add to queue next** | Adds right after the playing song. | **Add next** |
| **Add to queue last** | Adds to the end of the queue. | **Add last** |

Shuffle play, the Auto-DJ and the AI DJ do not depend on it. The other rows of the **Lists** section are **Grid multi-select**, **Show favourites** (hearts everywhere) and **Show ratings** (star ratings, for Navidrome and OpenSubsonic).

## Keyboard

Keys act on the list you are looking at, when no text field has the focus.

| Key | Action |
|---|---|
| `Up`, `Down` | Move the selection one row. |
| `Left`, `Right` | Move one card (grids only). |
| `Home`, `End` | First or last item. |
| `Page Up`, `Page Down` | Move one screenful. |
| `Ctrl+A` | Select all, or deselect all if everything is selected. |
| `Enter` | Plays the selection following **Play button behavior**. |
| `Ctrl+G` | Opens the page of the first selected item (for example, the album). |
| `Ctrl+L` | Selects and scrolls to the playing song, or opens its album. |
| `Space` | Play or pause. |

`Enter`, `Ctrl+G` and `Ctrl+L` are hotkeys you can change in **Settings → Hotkeys**; there are also optional hotkeys for **List: play now**, **List: play next** and **List: play last**. See [Hotkeys](../integration/hotkeys.md). The context menu has its own keys: `Up` and `Down` move, `Right` opens a submenu, `Left` closes it, `Enter` runs the entry and `Esc` goes back or closes the menu.

## Hover controls

Move the pointer over a row and its `#` column turns into a play button that follows the play button behavior. Rest on it for a moment and a small bar offers **Play now**, **Play next** and **Play last**. A right-click on the button opens the same three choices. The **Actions** column (three dots), when shown, opens the context menu. Hearts and stars in the **Favorite** and **Rating** columns work with one click; clicking the same star again clears the rating.

## Drag and drop

Press on a row or card and move more than 6 px to start dragging. If the row is part of a selection, the whole selection is dragged. A small label follows the pointer. Drop it on:

- **the queue panel** (or the Now Playing page): the items are inserted where you drop them. Dragging rows within the queue reorders them. Dropping on an empty queue starts playing.
- **one of your playlists in the sidebar**: the items are added to that playlist. Playlists you cannot change do not accept drops, and neither does the sidebar while **Sidebar playlist sorting** is on (dragging then reorders the playlists).

Albums, artists and playlists that you drop are expanded into their songs first.

In a playlist's edit mode, rows are dragged by a grip instead; see [Playlists and smart playlists](playlists.md#changing-the-order).

## Context menus

Right-click any song, album, artist, playlist, genre or folder, or choose **More options** (three dots), to open its menu. The menu acts on the whole selection. Entries that the current server cannot do, or that do not apply, are left out. The footer of the menu previews what it acts on.

### Songs

| Entry | What it does |
|---|---|
| **Remove from history** | Only in **Recently played**: forgets those plays. |
| **Play** | A side menu with **Play**, **Next**, **Last**, **Play (shuffled)**, **Next (shuffled)** and **Last (shuffled)**. Clicking **Play** itself does what your play button behavior says. |
| **Start radio** | Plays songs similar to this one. Greyed out for several songs. |
| **Add to playlist...** | See [Add to a playlist](playlists.md#adding-songs-to-a-playlist). |
| **Remove from playlist** | Only inside a playlist you can change. Asks for confirmation. |
| **Favorite** | **Add to favorites** or **Remove from favorites**. On YouTube Music: **Like** and **Remove from Liked Music**. Not offered for local folders. |
| **Set rating** | No stars to five stars. Navidrome and OpenSubsonic only. |
| **Download** / **Remove download** | Offline copy. See [Downloads](downloads.md). |
| **Copy link** | YouTube Music songs: copies the song's public link. |
| **Go to** | **Go to album** and **Go to** each artist. Greyed out for several songs. |
| **Get info** | See below. |
| **Edit metadata...** | Local folder songs only. See below. |
| **Show in file manager** | Local folder songs, one at a time. |
| **Share...** | Servers that can create share links (Navidrome). |
| **Save file...** | Servers that allow downloading the original file (Navidrome, OpenSubsonic and Jellyfin). |
| **Clear selection** | When opened from a list. |

On rows of the play queue the menu starts with **Remove from queue** and has **Move items** (**Move to top**, **Move to next**, **Move to bottom**) and **Shuffle** (**Shuffle selected**, **Shuffle all**) as well. See [Player bar and queue](../playback/queue.md).

### Albums

**Play** (side menu), **Album radio** (**Play**, **Next**, **Last**), **Add to playlist...**, **Favorite** (on YouTube Music: **Save to library** and **Remove from library**), **Set rating**, **Download**, **Go to** the album artists, **Get info**, **Share...** (on YouTube Music **Copy link**), **Save file...** and **Clear selection**.

### Artists

**Play** (side menu), **Artist radio**, **Add to playlist...**, **Favorite** (on YouTube Music: **Subscribe** and **Unsubscribe**), **Set rating**, **Get info**, **Share...** or **Copy link**, **Clear selection**.

### Playlists

**Play**, **Shuffle**, **Play next**, **Add to queue**, a **Play** side menu, **Add to playlist...** (adds all its songs to another playlist), **Download**, **Open playlist**, **Edit playlist...**, **Edit smart rules...** (smart playlists), **Copy link** (YouTube Music), **Delete playlist...**, **Get info**, **Share...**, **Save file...** and **Clear selection**. Editing and deleting only appear for playlists you own on a server that allows them. See [Playlists and smart playlists](playlists.md).

### Genres and folders

A genre has **Play** (side menu), **Add to playlist...** and **Clear selection**. A folder has the same, plus **Share...** and **Save file...** where the server allows it. A folder's songs are collected recursively.

### Get info

**Get info** opens a window with every fact Listen knows about the item. For a song that includes, where available, the id, title, artists, album, disc and track, year, genres, duration, codec, bitrate, sample rate, bit depth, channels, size, BPM, favorite, rating, play count, last played, dates added and modified, ReplayGain values, whether it has lyrics, the comment, MusicBrainz ids and the file path. Albums, artists and playlists show their own facts. Empty values are left out, and the server is asked for more details while the window is open. With several items selected, arrows step through them. A copy button next to a value copies it, a link button opens a MusicBrainz page, and the path of a local file has a **Show in file manager** button.

### Edit metadata (local folders)

For songs from a local folder, **Edit metadata...** opens a tag editor. It edits **Title**, **Artist**, **Album**, **Album artist**, **Track**, **Disc**, **Year**, **Genre**, **Comment** and **Lyrics**, and the **Cover**: **Use image** (give the path of a PNG or JPEG file), **Remove** or **Keep**. You can select several songs; fields with different values show a hint and are left alone unless you type something. **Preview changes** lists exactly what will change, **Write tags** writes the files one by one, and Listen then rescans the folder. The files on disk are changed, so keep a backup if you care about the old tags.

### Share links and copy link

On Navidrome, **Share...** opens **Share item**. Set **Expiration** with an amount and a unit, or use a preset (**Server default**, **1 day**, **1 week**, **1 month**, **1 year**), and optionally add a **Description**. **Make default** stores your choice as the default for new shares (you can also set it in **Settings → General → Sharing**). When you press **Share**, the link is created on the server and copied to the clipboard; the notification has an **Open link** button.

On YouTube Music, **Copy link** copies the public `music.youtube.com` address of the song, album, artist or playlist.

### Save file

**Save file...** downloads the original, unconverted file from the server into your system **Downloads** folder (not Listen's offline store). Several songs of one album go into a folder named after the album. If a file with that name exists, Listen adds a number such as "(2)". A progress notification shows how far it is. Do not confuse it with **Download**, which stores a copy inside Listen for offline playback.
