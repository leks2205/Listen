# Playlists and smart playlists

This page explains how to create, change and delete playlists, how to add songs to them, and how smart playlists work. Playlists live on your music server (or in your YouTube Music account); Listen does not keep playlists of its own.

## What each source can do

| Action | Navidrome / OpenSubsonic | Jellyfin | YouTube Music | Local folder |
|---|---|---|---|---|
| See playlists | yes | yes | yes | no |
| Add songs to a playlist | yes | yes | yes | no |
| Create a playlist | yes | no | yes (signed in) | no |
| Rename, change description and visibility | yes | no | yes | no |
| Remove songs, change the order | yes | no | yes | no |
| Delete a playlist | yes | no | yes | no |
| Playlist cover | Navidrome 0.61 and newer | no | no | no |
| Recommended songs | yes | yes | yes | no |
| Smart playlists | Navidrome only | no | no | no |

Buttons for things a source cannot do are not shown. On YouTube Music you need to be signed in with an account; as a guest you can browse and play but not change playlists. A playlist you do not own, and YouTube Music's built-in lists such as Liked Music, can be played but not edited. YouTube Music support is unofficial; see [YouTube Music](../getting-started/youtube-music.md).

## The Playlists page

The sidebar already lists your playlists, so the **Playlists** page is not in its list of pages by default. Show it by ticking **Playlists** in **Settings → General → Sidebar**, or open it from **Go to page...** in the [command palette](search.md#the-command-palette), or click the **Playlists** heading in the collapsed sidebar. The page shows your playlists as a grid or table. The header has:

- the title and the number of playlists,
- **Play**, which plays all listed playlists following your [play button behavior](lists.md#play-button-behavior),
- a **Search** box (matches name, owner and description),
- a sort menu with **Name**, **Song count**, **Duration**, **Owner** and **Visibility**, and an order button,
- **Refresh**,
- **Create playlist** and, on Navidrome, **Create smart playlist**,
- the display buttons and list options described in [Lists, selection and menus](lists.md).

## Creating a playlist

Any of these opens the **Create playlist** window:

- the **+** button next to **Playlists** in the sidebar (or in the collapsed rail),
- **Create playlist** on the Playlists page,
- **Create playlist...** in the [command palette](search.md#the-command-palette),
- **Add to playlist...** on a song, typing a name in **New playlist...** and pressing **Create** (this creates the playlist with the selected songs in it).

The window has a **Name**, and, when the server supports it, a **Description** and a **Public** checkbox. Press **Create**. Playlists start private unless you tick **Public**. On Navidrome a public playlist is visible to the other users of your server.

## Opening a playlist

Click a playlist in the sidebar, in the Playlists page, in the home page or in the palette. The top of the page shows:

- the cover,
- the word **PLAYLIST**, a **Public** or **Private** badge and, for smart playlists, a **SMART** badge,
- the owner, number of songs and total time,
- **Play** (or **Add next** / **Add last** per your setting), **Add next**, **Add last** and a chevron with the shuffled variants, and **More options**, which opens the playlist's menu,
- the description, with **Show more** when it is long.

The toolbar below has:

| Control | What it does |
|---|---|
| **Search** | Filters the songs by words in their title, artist, album or genre. |
| **Tracks** / **Albums** | Switches between the songs and the same songs grouped as album cards. |
| Sort menu and order button | Sorts the songs. The playlist's own order is the default. |
| Filter button | Filters by **Favorites**, **Rating**, **Year** (from and to), and by **Genres**, **Artists** and **Album artists**, each with **Match All** or **Any**. |
| **Refresh** | Loads the playlist again. |
| **Edit rules** | Smart playlists you own: opens the rule editor. |
| **Edit** | Starts the order-editing mode (see below). |
| Chevron | Collapses the large header so more rows fit. |

## Recommended songs

Below the last song of a playlist, Listen offers five **Recommended** songs that should fit it: your server's own similarity (Navidrome's and OpenSubsonic's `getSimilarSongs2`, Jellyfin's *Similar*) seeded with the playlist's first, last and middle song, or a YouTube Music radio around them. Songs the playlist already has are never offered, and the five are picked at random from everything that fits, so the strip changes as you open playlists.

- a click on a recommended song plays it and the rest of the strip,
- the **+** at its right adds it to this playlist in one click; the row disappears and a message confirms it,
- the ↻ button next to **Recommended** asks for another five.

The strip is cached per playlist for ten minutes, so scrolling up and down or coming back to the page does not ask the server again; **Refresh** in the toolbar, or the ↻ button, asks again. Local folders have no server-side similarity, so they show no strip, and neither does an empty playlist.

## Editing a playlist

### Name, description, visibility and cover

Right-click the playlist (in the sidebar, the Playlists page or its own page) and choose **Edit playlist...**. Change the **Name**, **Description** or **Public**, and press **Save**. This is also how you rename a playlist. On YouTube Music the window reminds you that the current description and visibility are not shown; only the changes you make are sent.

If your Navidrome server can store playlist covers, small **+** and bin buttons appear over the cover of your own playlists. The **+** asks for an image file (PNG, JPEG, WebP or GIF, up to 10 MB). The bin removes the uploaded cover.

### Changing the order

1. Open the playlist and press **Edit**.
2. Drag the rows by the grip in the first column to their new places. The row where you will drop it is outlined.
3. Press **Save and replace** and confirm "Replace the playlist with the new order? Are you sure?". Press **Cancel** to throw the changes away.

While you edit, search, sort and filters are switched off and the page shows the songs as tracks. **Save and replace** writes the whole playlist back to the server, so avoid editing the same playlist elsewhere at the same time. **Edit** is only offered for playlists you own on servers that can reorder (Navidrome, OpenSubsonic and YouTube Music). If you press it before a long playlist has finished loading, Listen tells you to wait.

### Removing songs

Select one or more songs in the open playlist, right-click and choose **Remove from playlist**. After you confirm ("Are you sure you want to remove this song from ...?") the songs are removed from the playlist on the server. The songs themselves stay in your library.

### Deleting a playlist

Right-click the playlist and choose **Delete playlist...**. The menu then asks "Delete ...? This can't be undone." Choose **Delete** to remove it or **Cancel** to keep it.

## Adding songs to a playlist

There are three ways.

**From a menu.** Right-click songs, an album, an artist, a genre, another playlist or a folder, and choose **Add to playlist...**. The menu turns into a picker:

- a **Search playlists...** box (it finds names even when you leave letters out),
- **New playlist...** and **Create**, to make a new playlist with these songs (only on servers that can create playlists),
- **Skip duplicates**, a tick box: songs already in the playlist are not added again. It is off by default, applies on Navidrome, OpenSubsonic and Jellyfin, and is not offered for YouTube Music.
- the list of your playlists, with the one you used last pinned at the top and marked with a star.

Click a playlist to add the songs. A notification says what was added. Albums, artists and folders are added as all their songs.

**By dragging.** Select songs, drag them to one of your playlists in the sidebar and release. See [Drag and drop](lists.md#drag-and-drop).

**On YouTube Music** the picker shows a check mark behind playlists that already contain the song, and does not let you add it to them again. Songs always go into the playlist on their own server, so a queue that mixes servers can only add to a playlist of the song's source.

## Smart playlists

A smart playlist has no fixed list of songs. It stores rules, such as "genre contains rock and rating is greater than 3, sorted by play count, limited to 50", and the server works out the songs. Smart playlists are a Navidrome feature, and Listen edits them through Navidrome's own API. They appear in the sidebar like any other playlist, with a **SMART** badge on the playlist page.

### Creating and editing

Press **Create smart playlist** on the Playlists page. To change an existing one, press **Edit rules** on its page or right-click and choose **Edit smart rules...**. The editor window has:

| Part | Meaning |
|---|---|
| **Name**, **Description**, **Public** | As for normal playlists. |
| **Refresh the songs when the playlist is opened** | The server recomputes the list whenever you open the playlist. |
| **Query editor** | The rules, described below. |
| **Sort** | One or more sort entries. Each has a field (or **Random**) and **Ascending** or **Descending**. **Add sort** adds another. |
| **Limit** | At most so many songs (**#**, empty means no limit) or so many percent of the matching songs (**%**, 0 to 100). |

A rule group has a selector, **Match all** or **Match any**, and the buttons **Add rule** and **Add rule group**. Groups can be nested inside each other. The top group also has **Reset to default** and **Clear filters**; nested groups have **Remove rule group**.

A rule is a **Field**, an **Operator** and a value:

| Field type | Operators |
|---|---|
| Text | Is, Is not, Is missing, Is present, Contains, Does not contain, Starts with, Ends with |
| Number | Is, Is not, Is missing, Is present, Contains, Does not contain, Is greater than, Is less than, Is in the range (From and To) |
| Yes / no | Is, Is not, Is missing, Is present |
| Date | Is, Is not, Is missing, Is present, Is before, Is after, Is in the last and Is not in the last (a number of days), Is in the range, and the "(date)" forms that take dates as `YYYY-MM-DD` |
| Playlist | Is in, Is not in (pick another playlist) |

The fields are the ones Navidrome knows, such as Name, Album, Artist, Album Artist, Genre, Year, Duration, BPM, Bitrate, Is Favorite, Rating, Play Count, Date Added, Date Last Played, File Path and many tag fields. If you use tags Navidrome indexes under other names, add them in **Settings → General → Query builder → Custom fields**: give each a **Label**, a type (string, number, boolean, date or playlist) and the **Tag** name used in rules. That section only exists while the server you browse supports smart playlists.

Press **Create** (or **Save**). The editor refuses an empty name, and shows the server's message if the rules are not valid. Afterwards the playlist page opens and shows the songs the rules select.

### Notes

- Smart playlists are computed by the server. Listen only edits the rules and shows the result, so the same list also appears in other Navidrome apps.
- The contents come from the rules, so change the rules rather than adding, removing or moving songs by hand.
