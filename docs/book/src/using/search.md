# Search and the command palette

Listen has two ways to search: the command palette, a quick window you open from anywhere, and the search page, a full page with tabs. The palette can also jump to any page, switch servers and start a new playlist.

Both search the server you are currently browsing. With a local folder source they search that folder. If you want to search another server, switch to it first.

## The command palette

Open the palette with `Ctrl+K`, with the **Search** box at the top of the sidebar, or with **Open command palette** in the [app menu](window.md#the-app-menu). Press `Ctrl+K` again to close it. You can change the shortcut under **Settings → Hotkeys** (the action is called **Global search**); see [Hotkeys](../integration/hotkeys.md).

Start typing. After a short pause (about 0.4 seconds) Listen searches the server and shows up to five results per type under these headings:

| Heading | Chosen result does this |
|---|---|
| **Albums** | Opens the album page. |
| **Artists** | Opens the artist page. |
| **Tracks** | Plays the song right away. |
| **Playlists** | Opens the playlist. |

Each heading shows how many results it has. Click the heading to fold or unfold it. **View all** opens the search page on that tab with your text, so you can see more than five.

Under the results is a **Search** group with these commands, which you can also reach with an empty box:

- **Search for "your text"** opens the search page with the same text.
- **Create playlist...** opens the new playlist dialog. Only offered when the server can create playlists.
- **Go to page...** opens a second page of the palette. The entries are **Home**, **Search**, **Settings**, **Favorites**, **Recently played**, **Statistics**, **Mixes**, **Radio**, **Downloads** under **Pages**, and **Albums**, **Tracks**, **Album artists**, **Artists**, **Genres**, **Folders**, **Playlists** under **Library**. Pages the current server does not have are left out. Typing narrows the list.
- **Server commands...** lists your servers under **Select server** (choose one to switch to it) and **Manage servers...**, which opens **Settings → General**.

### Keys

| Key | Action |
|---|---|
| `Up`, `Down` | Move the highlight. |
| `Enter` | Run the highlighted entry. |
| `Esc` | Go back one page of the palette; on the first page it closes the palette. |

The footer shows a breadcrumb of the pages you opened (click a part to go back) and the keys. The cross in the text box clears your text.

## The search page

The search page is hidden in the sidebar by default. Show it with **Settings → General → Sidebar** (tick **Search**), or open it from the palette: **Go to page... → Search**, **Search for "..."** or **View all**.

Type in the box at the top. Listen searches while you type, after a short pause, and `Enter` searches at once. The results are in tabs:

- **Tracks**,
- **Albums**,
- **Artists**,
- **Playlists**, which appears when the server found some.

Once you have typed something, each tab shows its number of results, for example **Albums (12)**. Listen asks the server for up to 100 results of each type. The box says "Type to search your library." while it is empty, and the list says "No results." when nothing matched.

Each tab is an ordinary list: you can switch between table and grid with the buttons at the right (albums and artists), change columns in the list options, select, drag and right-click exactly as described in [Lists, selection and menus](lists.md). `Ctrl+F` puts the cursor back into the search box (**Local search** in the hotkey list).

## What gets searched

What the search matches depends on the server:

- **Navidrome, OpenSubsonic and Jellyfin** search their library for song titles, albums, artists and, where supported, playlists.
- **YouTube Music** searches YouTube Music's catalogue, not only your own library. Results from it are played through the unofficial YouTube Music support described in [YouTube Music](../getting-started/youtube-music.md).
- **Local folders** search the files Listen found in the folder.

Filtering the lists on the Albums and Tracks pages is separate and works on your library only; see [Sorting, searching and filtering](lists.md#sorting-searching-and-filtering).
