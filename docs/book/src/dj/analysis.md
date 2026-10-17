# Song analysis

Listen can listen to your songs once and remember what it heard: where the beats and bars are, how fast the song is, which key it is in, how loud and energetic it is and how it is built. The [DJ transitions](transitions.md) and the [AI DJ](ai-dj.md) use these measurements. This page explains what is measured, where the results are kept and how to control the background work.

Analysis is on by default for Navidrome (OpenSubsonic) songs. You can ignore it completely if you do not use the DJ features: it never changes your files and it only runs in the background.

## What is measured

For every analysed song Listen stores:

- **Beats and downbeats.** The position of every beat and which beat starts a bar. This is what makes beat-matched blends possible.
- **Tempo.** Beats per minute, with a confidence value. If your server provides a BPM tag, Listen uses it only as a hint to decide between half and double tempo.
- **Key.** The musical key, shown in the Camelot notation used by DJs (for example `8A` or `11B`). Keys that are neighbours on the wheel sound good together.
- **Sections.** Intro, body, breakdown, build, drop and outro, counted in bars, plus phrase length (4, 8 or 16 bars).
- **Energy.** Loudness, bass level, brightness and how likely a lead vocal is, one value per bar. From these Listen derives an overall energy, danceability and brightness. It also finds how many bars at the start and end of a song have no lead vocal, because those are the best places to blend.
- **Loudness.** Integrated loudness and loudness range, and the first and last moment where the song is actually audible.
- **Timbre.** A short vector that describes the sound of the song, used to judge how similar two songs are.

Songs without a steady beat (ambient, live recordings, strong tempo drift) get no beat grid. They are still analysed, but they are joined with plain fades.

Songs longer than 30 minutes are skipped.

## Which songs are analysed

| Source | Analysed |
|---|---|
| Navidrome / OpenSubsonic | Yes, queue first and then the whole library |
| YouTube Music | Only if you switch it on, see [below](#youtube-music-songs) |
| Jellyfin, local folders, internet radio | No |

Songs from sources that are not analysed are never beat-mixed. They play as usual.

## The beat model

To find beats and downbeats Listen uses a small neural network called Beat This! (MIT licence, from the Johannes Kepler University Linz). The two model files are about 11 MB in total and are downloaded once, the first time analysis runs. They are checked against a fixed checksum and then stored in the `models` folder inside Listen's cache folder (see [Files and folders](../reference/files.md)). Nothing else is downloaded and no song data is sent anywhere.

If the download fails (for example because you are offline), Listen falls back to a simpler built-in beat tracker and tries to download the model again the next time it starts. The built-in tracker is less accurate, especially above about 140 BPM.

The status line (see below) shows which one is in use: **Beats: Beat This!** or **Beats: built-in tracker**.

## How the work is scheduled

Analysis runs in a separate helper process with low priority, so it should not disturb playback. The helper stops by itself after 30 seconds without work and is started again when needed.

Songs are analysed in this order:

1. The song that is playing and the next one.
2. The rest of the queue (the next 12 songs).
3. YouTube Music only: when Auto-DJ or the AI DJ is on, songs similar to the current one, so the next pick can be ordered by flow (Navidrome songs are covered by step 4).
4. The whole Navidrome library, a little at a time in the background. This is the "crawl". Listen lists your library first and then works through it.

Queue work always goes first, so a song you are about to hear is analysed before a song you might hear next month. Library work pauses while a DJ transition is running, so the mix gets all the processor time it needs.

A 3.5 minute song takes roughly 2 to 3 seconds on a current desktop processor. A library of 10,000 songs therefore needs several hours in the background. Songs are analysed only once: the results are kept, and if two computers share the same folder they split the work (see [Sharing between computers](#sharing-the-results-between-computers)).

If a later version of Listen improves the analysis, old results are used until the song has been analysed again, and the new analysis runs at low priority.

### Battery

On a laptop that is running on battery, library analysis pauses. Work for the queue (and for YouTube Music DJ candidates) still runs, because a transition may depend on it. To also analyse the library on battery, turn on **Also on battery**.

> **Note:** Detecting battery power works on Linux. On Windows the library crawl is not paused on battery.

### Threads

Beat detection uses several processor threads. By default Listen uses a quarter of your cores (at least one). To change that, set `threads` in the `[analysis]` section of the settings file (see [Files and folders](../reference/files.md)). `0` means automatic; any other number is limited to the number of cores you have. The change takes effect after a restart. There is no switch for it in the settings window.

## Settings

All switches are in **Settings → Playback → DJ transitions & song analysis**.

| Setting | Default | What it does |
|---|---|---|
| **Analyse songs** | On | Master switch. When off, nothing is analysed and the DJs work without measurements. |
| **YouTube Music songs** | Off | Also analyse YouTube Music songs. Needs an analysis folder. |
| **Whole library** | On | Work through the whole Navidrome library in the background. The queue always comes first. When off, only the songs in the queue are analysed (and, for YouTube Music, the DJ candidates). |
| **Also on battery** | Off | Keep analysing the library when the laptop is unplugged. |
| **Analysis folder** | Listen's data folder | Where the results are stored. Can be a synced folder shared between computers. |

Below the first switches there is a status line, described next.

### The status line

The grey text under **YouTube Music songs** tells you what is going on. It can contain:

- `N songs analysed`, and in brackets how many each computer contributed when a folder is shared.
- `library N songs`, the size of the library found so far, with `(listing...)` while the library is still being listed.
- `N waiting`, the number of songs in line.
- A reason for waiting in brackets: `paused during a DJ transition`, `library analysis is off` or `paused on battery`.
- `Now:` the song being analysed.
- `Beats:` the beat source and `this machine:` the name that identifies this computer in the shared folder.
- An error, for example when the analysis folder cannot be written.
- If you have a YouTube Music account: `YouTube Music: analysed on demand (queue and DJ candidates)` or `YouTube Music: not analysed`.

It shows **Off.** when **Analyse songs** is off and **Starting...** for a moment after start.

## The analysis folder

By default the results are kept in a folder called `analysis` inside Listen's data folder (see [Files and folders](../reference/files.md)). To use another folder, type its path into **Analysis folder** and press **Save**, or press **Choose...** and pick it. Leaving the field empty returns to the default. Changing the folder restarts the analysis; results in the old folder are not moved, so copy them over first if you want to keep them.

Inside the folder Listen creates `v1/<computer name>/000001.jsonl`, `000002.jsonl` and so on. These are plain text files with one song per line, including the artist and title so you can recognise them. They contain only measurements, never audio.

### Sharing the results between computers

Analysing a big library takes time, so it is worth doing only once. If you point **Analysis folder** on every computer at the same synced folder (for example a folder in Nextcloud, Syncthing or any other sync tool), then:

- every computer reads the results of all the others;
- every computer writes only into its own sub-folder, so two computers never write to the same file and sync conflicts are avoided;
- songs that another computer has already analysed are not analysed again;
- the status line shows how many songs each computer contributed.

Results are matched by song and file, not by server address, so it also works when the computers reach the same server under different addresses. If a song's file is replaced on the server (for example re-encoded), it is analysed again.

> **Tip:** Let the sync tool finish its first sync before you start Listen on the second computer, otherwise both computers analyse the same songs in the meantime.

## YouTube Music songs

> **Note:** YouTube Music support in Listen is unofficial and not affiliated with Google or YouTube. See [YouTube Music](../getting-started/youtube-music.md). Listen does not download YouTube Music songs for offline use.

YouTube Music songs can be beat-mixed too, but they need a measurement like any other song. This is off by default. To turn it on:

1. Choose an analysis folder first (see above). The switch stays off while the folder is empty.
2. Turn on **YouTube Music songs**.

When a YouTube Music song is wanted, Listen fetches a temporary copy of the audio, measures it, and then keeps only the measurements. The audio itself is not saved as a library.

YouTube Music songs are analysed only on demand: the playing song, the queue and a limited number of candidates for the DJ (at most 20 per playing song). The whole YouTube Music library is never crawled, and requests to YouTube are limited on purpose. If a request is turned down, Listen tries again later. You use this at your own risk, like the rest of the YouTube Music support.

## Turning analysis off

Turn off **Analyse songs**. Existing results stay in the folder and are used again if you turn it back on. Without analysis the Auto-DJ and AI DJ still work, but songs are never beat-matched; they follow each other with the normal gapless playback.

You can also read progress from scripts: the [MCP tool](../reference/mcp-tools.md) `get_analysis_status` returns the same information as the status line.
