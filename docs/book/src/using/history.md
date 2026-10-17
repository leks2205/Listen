# History, statistics and mixes

Listen keeps a listening history on your computer. It feeds three pages: **Recently played**, **Statistics** (with a year review) and **Mixes**. This page explains what is recorded, how to control it, how to export or delete it, and what the pages show.

The history is never uploaded or shared. It is a plain text file on your computer, and everything on the three pages is calculated from it on demand. Deleting entries therefore also removes whatever was derived from them.

## What is recorded

A song is added to the history when it counts as played, using the same rule as scrobbling: when you have listened to it for a set share of its length or a set number of seconds, whichever comes first. Skipping ahead in a song does not count as listening. The AI DJ's spoken breaks are not recorded.

The rule is set under **Settings → General → Scrobble**:

| Setting | Default |
|---|---|
| **Scrobble minimum mode** | **Percentage or seconds** (the first of the two). The other choices are **Percentage** and **Seconds**. |
| **Minimum scrobble percentage** | 75 %, adjustable from 25 to 90 % |
| **Minimum scrobble seconds** | 240 s, adjustable from 0 to 1200 s |

The history works independently of the **Scrobble** switch: that switch only controls whether plays are reported to your server (for example to Last.fm).

Each entry stores the time, which server and song it was, the title, artists, album and length, and the cover address so that the list can be shown without asking the server. Songs from every source are recorded, including YouTube Music and local folders.

## Settings

Open **Settings → Advanced → History**.

| Setting | What it does |
|---|---|
| **Record listening history** | On by default. Off means nothing is written. What is already there stays until you delete it. |
| **Don't record this session** | Pauses recording until you close Listen. This is the same switch as private mode, see below. |
| **Keep history for** | **1 year**, **2 years** or **Unlimited** (the default). Older entries are deleted automatically: when Listen starts, once a day while it runs, and right away when you change the setting. Choose **Unlimited** if you want year reviews of earlier years. |
| **Export** | **JSON** or **CSV**. Writes the whole history to a new file in your Downloads folder, called `listen-history-<time>.json` or `.csv` (the time is a Unix timestamp). A notification shows the path. If there is no Downloads folder, the file goes into Listen's data folder. |
| **Delete history** | **Delete all**, then confirm with **Yes, delete**. This removes every entry and with it the statistics and mixes. |

The CSV file has the columns `timestamp` (Unix seconds), `server_id`, `source`, `song_id`, `status`, `title`, `artists` (separated by semicolons), `album` and `duration_secs`. Text that a spreadsheet would read as a formula gets a leading apostrophe. The JSON file holds the same entries as a list.

The history itself is the file `history.jsonl` in Listen's data folder; see [Files and folders](../reference/files.md). You can back it up like any other file, and [Backup, logs and diagnostics](../more/backup.md) covers what else is worth keeping.

### Private mode

**Private mode** stops three things at once: no entry is added to the history, no plays are scrobbled and Listen shows nothing on Discord. The window title says that private mode is on. It switches off again when Listen restarts.

Turn it on or off in any of these places; they all control the same switch:

- **Menu → Private mode** in the sidebar's [app menu](window.md#the-app-menu),
- **Settings → Window → Private mode**,
- **Settings → Advanced → History → Don't record this session**.

## Recently played

**Recently played** lists the last 500 plays, newest first, as an ordinary song list. Double-click a row to play from it. To forget a single play, right-click the row and choose **Remove from history**. If you have switched the history off, the page says so instead of listing songs.

## Statistics

**Statistics** summarises what you listened to. Buttons at the top choose the period: **All time**, or one of the years that have entries. Listen starts on the current year, or on the newest year with entries if this year has none. The heading reads "Your 2026 in music" for a year and "Your listening, all time" otherwise; this is the year review.

The page shows:

- **songs played**: the number of plays in the period,
- **listening time**: the full length of each song you played past the scrobble point, added up,
- **Top songs**, **Top artists** and **Top albums**: ten each, with play counts. Songs and albums also show their artist; the others show the listening time.

The ranking breaks ties by listening time and then by name. The artist chart counts every credited artist of a play, so a song with two artists counts for both.

## Mixes

**Mixes** builds playlists of up to 50 songs from your history. Open one to see its songs and play it. Each row is a normal song row with the usual [menus](lists.md#context-menus), so you can add the whole mix to a playlist from there.

| Mix | What it contains |
|---|---|
| **Daylist** | Songs you usually play around this time of day (within an hour of now). |
| **Heavy rotation** | Your most played songs of the last 30 days. |
| **Rediscover** | Songs you played at least twice before but not at all in the last 60 days. |
| **All-time favourites** | Your most played songs ever. |

A mix without enough history behind it is greyed out and says "Not enough history yet". Mixes change as you listen. They are made on your computer and need no server feature.

The artist page's **Top songs → Personal** view uses the same history; see [Library pages](library.md#artist-page).

## Related

- [Search and the command palette](search.md) lists the Statistics, Mixes and Recently played pages under **Go to page...**.
- [Privacy and security](../more/privacy.md) summarises what stays on your computer.
