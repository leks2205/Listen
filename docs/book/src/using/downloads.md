# Downloads

Downloads store copies of songs inside Listen so that you can play them without a network connection or when your server is down. This page explains which sources support it, how to download and remove songs, where the files are kept and what limits apply.

## What can be downloaded

Offline downloads work for songs from **Navidrome and other OpenSubsonic servers**. Jellyfin servers and local folders do not offer downloads (a local folder is already on your disk). **YouTube Music songs cannot be downloaded**: Listen does not download YouTube Music songs for offline use, and it does not offer the command for them. Listen's YouTube Music support is unofficial; see [YouTube Music](../getting-started/youtube-music.md).

> **Note:** Downloads are for your own server's music. They are not a way to export your library to other programs. To save a normal file that you can use elsewhere, see **Save file...** below.

## Downloading

Right-click a song, an album, a playlist or a selection (or use the three dots) and choose **Download**. Listen puts the songs in a queue and fetches them one at a time. While it works:

- a notification says "Downloading 12 songs...",
- the **Downloads** page shows "Downloading “Title” (3 of 12)..." in blue,
- when the batch is finished you get "Downloaded 12 for offline play." Songs that failed are counted too, and listed on the Downloads page with the reason.

Songs you already have are skipped ("Already downloaded."). If the server is not connected or has no download support, Listen says so.

Listen downloads the file as the server stores it, not a transcoded version.

## The Downloads page

**Downloads** is always in the sidebar, below your pages. It lists everything that is stored, sorted by artist, album and track number, and shows how many songs there are and how much space they use, for example "12 songs · 480 MB".

| Control | What it does |
|---|---|
| A song row | Click to play the list from that song. The rows work like other song rows, including the right-click menu. |
| Bin button on a row | **Remove download** for that song. |
| **Remove all** | Deletes every download. |
| **Dismiss errors** | Clears the list of failed downloads shown under the title. |

To remove downloads from the context menu, select songs that are all downloaded and choose **Remove download**; the same entry replaces **Download** in that case.

Removing a song while it is being fetched cancels it. Removing a server from Listen also deletes all downloads from that server.

## Playing offline

Whenever a song has a downloaded copy, Listen plays the copy instead of streaming it, even when the server is reachable. This also saves traffic and keeps playback working on a bad connection.

If Listen starts and cannot reach any server, the connection screen offers **Play downloaded songs (offline)**, which opens the Downloads page. That page works without a connection, and so does the player. Browsing the library, search, lyrics lookup from the internet and similar features need a server or internet connection as usual.

## Where the files are

Downloads live in a folder named `offline` inside Listen's data folder (see [Files and folders](../reference/files.md)), with the audio in `offline/files` and an index file that remembers the song details. The folder is private: on Linux only your user can read it. A download counts only when it is complete; unfinished files (`.part`) are cleaned up when Listen starts. The file names are made of the server and song ids, so the folder is not meant to be browsed or used by other programs.

## Limits

- Downloads are fetched **one at a time**, in the order you queued them.
- Listen sets no size limit of its own; the limit is the free space on the disk that holds the data folder.
- An empty or failed file is discarded and shown in the list of errors.
- Downloads are tied to the server they came from. If you remove the server from Listen, its downloads go too.

## Download versus Save file

The context menu has two similar entries on Navidrome, OpenSubsonic and Jellyfin:

| Entry | Result |
|---|---|
| **Download** | A private copy inside Listen for offline playback (Navidrome and OpenSubsonic only). |
| **Save file...** | The original file saved into your system **Downloads** folder, where you can use it with any program. Several songs of an album go into a folder named after the album. |

See [Lists, selection and menus](lists.md#save-file) for details.
