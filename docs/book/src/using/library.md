# Library pages

This page describes the pages you reach from the sidebar: the home feed, albums, artists, tracks, genres, folders, favorites and the pages built from your listening history. The tables, grids and menus that appear on these pages work the same everywhere and are described in [Lists, selection and menus](lists.md).

What a page shows depends on the server you are browsing. Navidrome and OpenSubsonic servers offer everything. Jellyfin, YouTube Music and local folders have no folders page, and some have no genres or radio. A page the current server cannot provide is left out of the sidebar. The table at the end of this page lists the differences.

## Home

The home page is a feed of carousels.

**Featured carousel.** At the top, a banner shows random albums from your library with a large cover, the album's genres and year, and a **Play** button (the label follows your [play button behavior](lists.md#play-button-behavior)). Use the round arrows at the sides to move on. With the pointer over the banner, `Shift` plus the mouse wheel also steps through it. In **Settings → General → Home** you can switch it off (**Home featured carousel**) or choose **Home featured carousel style**: **Single** (one album, the default) or **Multiple** (a row of two to six albums, depending on the window width).

**The server's own feed.** YouTube Music has a home feed of its own, with shelves such as quick picks and mixes made for your account. Its shelves come first. The other sources have no feed of their own.

**Sections.** Below that come the sections, in this default order:

| Section | Content |
|---|---|
| **Genres** | Coloured genre tiles, each with a play button. **View more** opens the genres page. |
| **Most Played** | Your most played albums (songs on Jellyfin). |
| **Explore from your library** | Random albums. |
| **Newly added releases** | Albums most recently added to the server. |
| **Recently played** | Albums you played most recently. |
| **Recently released** | Albums by release year, newest first. |
| **Playlists** | Your playlists. |

Each carousel shows as many cards as fit the window. Use the arrow buttons in its header, drag the row sideways, or hold `Shift` and use the mouse wheel to move along it. Listen loads 20 more cards when you get near the end. The refresh button in a section's header loads it again.

To choose which sections appear and in what order, open **Settings → General → Home → Home sections**. Tick a section to show it and use the up and down arrows to move it.

If the page says "Your library looks empty.", the server returned nothing for any section.

## Albums

**Albums** lists every album on the server. It starts as a grid of covers and loads more as you scroll.

The header holds the page title and the number of albums (shown as "250+ albums" while more are still loading), a **Play** button and a **Shuffle play** button that play everything the current search and filters match (up to 20,000 songs), and these controls:

- **Sort by** and a button for ascending or descending order. The options depend on the server. Navidrome offers, for example, Album artist, Artist, Duration, Most played, Name, Random, Rating, Recently added, Recently played, Song count, Favorited and Release year.
- **Filters**, the funnel button, with filters such as Favorited, Rated, Compilation, Recently played, Artists, Genres, Year and custom tags. Which ones exist depends on the server.
- **Refresh**.
- **Collections**, the bookmark button, to apply or save a query.
- A **Search** box.
- The buttons for table, grid and detail view and the list options. See [Displays](lists.md#displays).

Listen remembers the sort, order and filters of the Albums and Tracks pages between sessions.

## Album page

Click an album anywhere to open its page.

The header shows the cover, the release type (Album, EP, Single, Compilation, ...) and the version if there is one, the title, a line with release date, number of tracks, running time, an "Explicit" mark, size on disk and play count, and the album artists as links to their pages.

Below that are the buttons:

- **Play** (or **Add next** / **Add last**, following your play button behavior), then **Add next** and **Add last**. The chevron next to them opens **Shuffle**, **Add next shuffled** and **Add last shuffled**. Holding the main **Play** button for half a second plays the shuffled version.
- **Start radio**, if the server can find similar songs.
- Five stars to rate the album (Navidrome and OpenSubsonic servers).
- A heart to add the album to your favorites. On YouTube Music this saves the album to your library.
- **More options**, the three dots, which opens the album's [context menu](lists.md#context-menus).

A toolbar has a **Search** box that filters the tracks, a sort menu for the tracks (the album's own order by default, or title, artist, duration, play count, rating, year and more), an order button and the list options. When an album has several discs and you are not searching or sorting, the tracks are grouped under **Disc 1**, **Disc 2** and so on, with the disc title when the server knows it. Tick a disc header to select all its tracks.

A long album comment is shown under the toolbar and can be expanded with **Show more**.

At the bottom of the page you find, where the data exists:

- the genres, as links to the genre pages,
- tags (release type, release country and status, **Explicit**), the record label, moods and groupings,
- **External links**, see [External links](#external-links),
- a copyright line with the label,
- **More from** carousels: one for the first album artist and one each for up to two of the album's genres. The genre carousels have a refresh button that picks other random albums.

## Artists and album artists

Listen separates two lists, as Navidrome and Feishin do:

- **Album Artists**: artists that appear as the album artist of at least one album. This is usually the list you want. YouTube Music has no album artists.
- **Artists**: everyone credited on a track, including guests and composers. On YouTube Music this page shows the artists you subscribe to.

Both pages are grids of portraits by default, with the number of albums and songs. They have no sort or search box; use the [command palette](search.md) to find an artist by name. Click an artist to open the page.

## Artist page

The header shows the portrait, **ARTIST**, the name and a line such as "4 albums, 52 tracks, 3 hr 20 min". The buttons are **Play**, **Shuffle**, **Add next**, **Add last**, a radio button (when the server supports it), a heart, five stars (Navidrome only) and three dots, which reveal **Refresh** and **Copy name**.

Below the header there are shortcuts: **VIEW DISCOGRAPHY**, **VIEW ALL TRACKS** and **ARTIST RADIO**; the artist's genres as buttons; and **EXTERNAL LINKS**. Then the sections follow in the order you chose:

| Section | Content |
|---|---|
| **About ...** (Biography) | The server's biography, collapsed to a few lines with **Show more**. |
| **Recent releases** (discography) | The artist's albums grouped by release type, such as Album, EP, Single, Live and Compilation. Choose how to sort (Release date, Name, Song count, Duration, Play count, Rating), reverse the order, search, and under the sliders button choose **Group by** **Primary types** or **All types**. Each group has play buttons; a group shows 100 cards, then **View more**. |
| **Videos** | YouTube Music only: the artist's music videos. |
| **Related artists** | Similar artists, with arrows. |
| **Top songs** | Ten songs with a search box and play buttons. **Community** shows the server's list; **Personal** ranks the artist's songs by how often you played them, from your [listening history](history.md). The page says "Nothing played yet." when there are none. **View all** lists all of them. |
| **Favorite songs** | Ten songs you favorited. On servers with ratings you can switch between **Favorite** and **Rating**. **View all** lists all. |

In **Settings → General** two sections control the page:

- **Artist page**: **Artist page configuration** (tick sections and reorder them), **Artist release type configuration** (which release types to show and in which order the groups appear) and **Artist radio count** (how many songs an artist radio queues; 20 by default).
- **External links**: see below.

### External links

Artist pages and album pages can show small buttons that open other websites in your browser. In **Settings → General → External links** you can switch the whole feature off (**External links**) or choose each site:

| Site | Where it leads |
|---|---|
| **Last.fm** | The artist or album on Last.fm. If the server supplied a Last.fm address, that one is used, otherwise a search by name. |
| **MusicBrainz** | The artist or album on MusicBrainz. Only shown where the server knows the MusicBrainz id. |
| **ListenBrainz** | The artist (by MusicBrainz id when known, otherwise a search) or a search for the album. |
| **Qobuz** | A search for the artist or album on Qobuz. |
| **Spotify** | A search on Spotify's web player. With **Open in the Spotify app** on, the link opens the desktop app instead. |

Listen only builds the address. Nothing is sent to these sites until you click a button.

## Tracks

**Tracks** lists every song on the server as a table. Like the Albums page it has **Play**, **Shuffle play**, sort, filters, refresh, collections and search, and it loads more rows as you scroll. The default columns are the number, a cover with title and artist, duration, album, genre, year and a heart. You can change them in the list options (see [Columns](lists.md#columns)).

## Genres

**Genres** lists the genres with their album and song counts. It has a **Search genres** box, a sort menu (**Name**, **Album count**, **Song count**) and an order button. Click a genre to open it.

A genre page shows either the albums or the tracks of that genre. Switch with the **Albums** and **Tracks** buttons; the choice is remembered. Both lists have the same sort, filter and search controls as the Albums and Tracks pages, without the genre filter.

## Folders

Servers that organise music in folders (Navidrome and OpenSubsonic) have a **Folders** page. The left side is a tree: **All folders** at the top, and folders that open with a chevron and load their sub-folders on demand. The right side shows a trail of the folders you are in, the sub-folders as a strip with a play button each, and the tracks directly in the open folder as a list. The buttons above the list are **Play**, **Play next**, **Play last**, **Shuffle** and **Refresh**. A right-click on a folder opens a menu that plays or queues everything inside it, including sub-folders, or adds it to a playlist.

For folders on your own computer, see [Connecting your music](../getting-started/sources.md); a local folder source does not have a folder browser.

## Favorites

**Favorites** collects what you marked with a heart. On servers that can favorite albums and artists it has three tabs: **Tracks**, **Albums** and **Artists**. On YouTube Music the tabs are named **Liked Music**, **Saved albums** and **Subscriptions**. A **Play** button plays the tab's songs. If nothing has a heart yet the page says "No favorites yet." (or "Nothing liked yet." on YouTube Music).

Show or hide hearts everywhere with **Settings → General → Lists → Show favourites**.

## Other pages

| Page | What it is |
|---|---|
| **Now playing** | The queue as a full page. See [Player bar and queue](../playback/queue.md). Hidden in the sidebar by default. |
| **Search** | The search page. See [Search and the command palette](search.md). Hidden in the sidebar by default. |
| **Playlists** | All your playlists. See [Playlists and smart playlists](playlists.md). |
| **Recently played** | The last 500 songs you listened to, newest first. Right-click a row and choose **Remove from history** to forget one play. Empty when the history is switched off. |
| **Statistics** | Top songs, artists and albums by year or of all time. |
| **Mixes** | Playlists made from your listening history. |
| **Radio** | Internet radio stations. See [Internet radio](../playback/radio.md). |
| **Downloads** | Songs stored for offline play. See [Downloads](downloads.md). |
| **Settings** | See [Settings](../reference/settings.md). |

Recently played, Statistics and Mixes are described in [History, statistics and mixes](history.md).

## What each source offers

| Page or feature | Navidrome / OpenSubsonic | Jellyfin | YouTube Music | Local folder |
|---|---|---|---|---|
| Home sections and featured carousel | yes | yes | yes | yes |
| The server's own home shelves | no | no | yes | no |
| Albums, Tracks, Artists | yes | yes | your library (empty as a guest) | yes |
| Album artists | yes | yes | no | yes |
| Genres | yes | yes | no | yes |
| Folders | yes | no | no | no |
| Favorites (songs) | yes | yes | yes (Liked Music) | no |
| Favorites (albums, artists) | yes | yes | yes | no |
| Star ratings | yes | no | no | no |
| Radio stations | yes | no | no | no |
| Offline downloads | yes | no | no | no |

YouTube Music support is unofficial. See [YouTube Music](../getting-started/youtube-music.md) for what that means and how to sign in.
