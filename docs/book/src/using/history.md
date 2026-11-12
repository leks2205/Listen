# History, statistics and mixes

Listen keeps a listening history on your computer. It feeds three pages: **Recently played**, **Statistics** (with a year review) and **Mixes**. This page explains what is recorded, how to control it, how to export or delete it, and what the pages show.

The history is a plain text file on your computer. It leaves your computer only if you switch on **Merge with other devices** (**Settings → Advanced → Listening history**), which sends your plays to your own listen-server and takes in the plays of your other devices. Everything on the three pages is calculated from it on demand. Deleting entries therefore also removes whatever was derived from them.

## What is recorded

A song is added to the history when it counts as played, using the same rule as scrobbling: when you have listened to it for a set share of its length or a set number of seconds, whichever comes first. Skipping ahead in a song does not count as listening. The AI DJ's spoken breaks are not recorded.

The rule is set under **Settings → General → Scrobble**:

| Setting | Default |
|---|---|
| **Scrobble minimum mode** | **Percentage or seconds** (the first of the two). The other choices are **Percentage** and **Seconds**. |
| **Minimum scrobble percentage** | 75 %, adjustable from 25 to 90 % |
| **Minimum scrobble seconds** | 240 s, adjustable from 0 to 1200 s |

### Skips

When you leave a song for another one before it counts as played (**Next**, **Previous**, the album skip buttons, double-clicking another song in the queue, or starting another list), the history records a **skip** instead: the song, the position you left it at, the play mode and whether you or a DJ picked it. Leaving a song does not count as a skip when it was never really started (a restored queue you have not played yet), when **Previous** only restarts the song, or when the player moves on by itself: a DJ mix into the next song, the end of a song, or a song that failed to load (also when you start another song later, after a song ended on its own). **Next** on the last song of the queue stops playback and counts as a skip, because you left the song early. The AI DJ's breaks and bridge songs are not recorded.

A skip is never a play: it does not count in the play numbers, the top lists, the mixes or **Recently played**, and it is never scrobbled. The Statistics page shows skips separately. A skip tells the DJs that a song did not fit at that moment.

The history works independently of the **Scrobble** switch: that switch only controls whether plays are reported to your server (for example to Last.fm).

Each entry stores the time, which server and song it was, whether it was played or skipped, the title, artists, album and length, and the cover address so that the list can be shown without asking the server. Newer entries also store the play mode, who picked the song (you, the Auto-DJ, the AI DJ) and which of your devices played it; skips store where in the song you left it. Songs from every source are recorded, including YouTube Music and local folders.

## Settings

Open **Settings → Advanced → Listening history**.

| Setting | What it does |
|---|---|
| **Record listening history** | On by default. Off means nothing is written. What is already there stays until you delete it. |
| **Don't record this session** | Pauses recording until you close Listen. This is the same switch as private mode, see below. |
| **Keep history for** | **1 year**, **2 years** or **Unlimited** (the default). Older entries are deleted automatically: when Listen starts, once a day while it runs, and right away when you change the setting. Choose **Unlimited** if you want year reviews of earlier years. |
| **Export** | **JSON** or **CSV**. Writes the whole history to a new file in your Downloads folder, called `listen-history-<time>.json` or `.csv` (the time is a Unix timestamp). A notification shows the path. If there is no Downloads folder, the file goes into Listen's data folder. |
| **Delete history** | **Delete all**, then confirm with **Yes, delete**. This removes every entry and with it the statistics and mixes. |

The CSV file has the columns `timestamp` (Unix seconds), `server_id`, `source`, `song_id`, `status` (`played` or `skipped`), `title`, `artists` (separated by semicolons), `album` and `duration_secs`. Text that a spreadsheet would read as a formula gets a leading apostrophe. The JSON file holds the same entries as a list.

The history itself is the file `history.jsonl` in Listen's data folder; see [Files and folders](../reference/files.md). You can back it up like any other file, and [Backup, logs and diagnostics](../more/backup.md) covers what else is worth keeping.

### Private mode

**Private mode** stops three things at once: no entry is added to the history, no plays are scrobbled and Listen shows nothing on Discord. The window title says that private mode is on. It switches off again when Listen restarts.

Turn it on or off in any of these places; they all control the same switch:

- **Menu → Private mode** in the sidebar's [app menu](window.md#the-app-menu),
- **Settings → Window → Private mode**,
- **Settings → Advanced → Listening history → Don't record this session**.

## Recently played

**Recently played** lists the last 500 plays, newest first, as an ordinary song list. Double-click a row to play from it. To forget a single play, right-click the row and choose **Remove from history**. If you have switched the history off, the page says so instead of listing songs.

## Statistics

**Statistics** summarises what you listened to. Buttons at the top choose the period: **Last 4 weeks**, **All time**, or one of the years that have entries. Listen opens on **Last 4 weeks**; picking a year shows that year, or the newest year with entries if the chosen one has none. The heading reads "Your 2026 in music" for a year and "Your listening, all time" otherwise; this is the year review.

The page shows:

- **songs played**: the number of plays in the period,
- **listening time**: the full length of each song you played past the scrobble point, added up,
- **songs skipped**: the number of skips in the period,
- **Top songs**, **Top artists** and **Top albums**: ten each, with play counts. Songs and albums also show their artist; the others show the listening time. Where a song, artist or album was also skipped in the period, the row adds "N skipped".

The ranking breaks ties by listening time and then by name. The artist chart counts every credited artist of a play, so a song with two artists counts for both.

### Last 4 weeks

The rolling view compares this week with last week. The periods are rolling and end now: **this week** is the last 7 days, **last week** the 7 days before that, and **4 weeks** the last 28 days.

- Four tiles for this week: **songs played**, **listening time**, **different songs** and **different artists**, each with the change against last week ("+25 % vs last week"). When last week had nothing, the tile says "nothing last week".
- **New artists this week**: artists with a play this week and no play at all before it, most played first, with the first song you heard from them. A skip alone does not count as having heard an artist.
- **Skipped most (4 weeks)**: the songs you skipped most often in the 28 days, with how often you skipped and played them.
- **The last 4 weeks**: plays, listening time and skips of the 28 days, then the top songs, artists and albums of that period, counted the same way as the year view.

## Mixes

**Mixes** builds playlists of up to 50 songs from your history. Open one to see its songs and play it. Each row is a normal song row with the usual [menus](lists.md#context-menus), so you can add the whole mix to a playlist from there.

| Mix | What it contains |
|---|---|
| **Daylist** | Songs you usually play around this time of day (within an hour of now). |
| **Heavy rotation** | Your most played songs of the last 30 days. |
| **Rediscover** | Songs you played at least twice before but not at all in the last 60 days. The ones you loved most (many plays, often close together) and have drifted furthest from come first. |
| **All-time favourites** | Your most played songs ever. |

A mix without enough history behind it is greyed out and says "Not enough history yet". Mixes change as you listen. They are made on your computer and need no server feature.

### Learnable songs

With a vocab-hub address set (see [Lyrics: known words](../playback/lyrics.md#word-meanings-korean)), Mixes also shows **Learnable songs**: your liked songs where you know 90 to 98 % of the Korean words, the easiest first. Enough to follow the song, with a few new words to pick up. Songs with fewer than 20 Korean words are left out. Opening the mix loads your liked songs and looks up the lyrics of those not seen yet, a few at a time (the page shows how far it is); songs you played with lyrics are counted already. Songs without Korean lyrics are looked at again after 30 days. Every song's words are kept in the cache folder (`lyrics-words.json`), so when you learn words in Lute the mix is counted again from there, without fetching lyrics again. Snoozed and "Not interested" songs stay out.

The artist page's **Top songs → Personal** view uses the same history; see [Library pages](library.md#artist-page).

## Related

- [Search and the command palette](search.md) lists the Statistics, Mixes and Recently played pages under **Go to page...**.
- [Privacy and security](../more/privacy.md) summarises what stays on your computer.

### How Listen tells a forgotten favourite

Listen models how present each song is in your memory, after the ACT-R model from cognitive psychology: every play adds to a song's "activation", and the effect of a play fades with time (by the square root of its age). Many plays close together give a high peak; months of silence bring the activation down. A **forgotten favourite** has at least five plays, none in the last 30 days, and a large gap between its peak and its activation now. The Auto-DJ's forgotten-favourite slot, the AI DJ's song pool and the Rediscover mix use this ranking. A song you skipped again and again in the last week counts as worn out and rests from automatic picks for a while. Skips never count as plays.

`listen --memory` prints the current top 20 forgotten favourites and the resting songs as JSON (`--history FILE`, `--now UNIX`, `--limit N`).
