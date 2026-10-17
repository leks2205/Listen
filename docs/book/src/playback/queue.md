# Player bar and queue

This page covers the player bar at the bottom of the window, the seek bar (slider or waveform), and the play queue in all its forms: side panel, drawer and the Now Playing page. It also explains the queue tools, the difference between songs you queued and songs a DJ added, how the queue is restored, saving it to your server and the sleep timer.

## The player bar

The bar has three columns. On the left are the cover and the song information, in the middle the transport buttons and the seek bar, on the right the extra buttons and the volume. When the window is 768 px wide or narrower, Listen switches to a compact bar with a smaller set of buttons.

### Left: song information

- Click the cover to open the [full-screen player](fullscreen.md). The small arrow on the cover moves the cover into the sidebar and back.
- The title is a link to the **Now Playing** page. Artists and album are links to their pages.
- The **⋮** button next to the title opens the song menu. A right-click on the song information opens the same menu.
- While a [DJ transition](../dj/transitions.md) runs, the third line reads "Mixing into …". While the [AI DJ](../dj/ai-dj.md) talks, its name and its line are shown instead of the song.
- While an [internet radio](radio.md) station plays, the station name and the current stream title are shown.

### Middle: transport buttons

From left to right:

| Button | What it does |
|---|---|
| Stop | Stops playback. |
| Play mode | Cycles Off, Shuffle, Auto-DJ and AI DJ. See [Play modes and Auto-DJ](play-modes.md). |
| Previous | Goes to the previous song. More than 10 seconds into a song, it restarts the song instead. `Alt`+click jumps to the previous album. |
| Skip backward | Jumps back by a fixed time (5 s by default). |
| Play / Pause | Starts or pauses playback. |
| Skip forward | Jumps ahead by a fixed time (10 s by default). |
| Next | Goes to the next song. `Alt`+click jumps to the next album. |
| Repeat | Cycles Off, One and All. A highlighted button means repeat is on. |
| Shuffle all songs | Plays a random selection of songs from the current server. |

On a narrow bar, Listen first hides the skip buttons, then Stop and "Shuffle all songs". A radio station keeps only Stop and Play.

The skip buttons, their durations and the button size are set in **Settings → General → Player bar** (**Show skip buttons**, **Skip backward duration**, **Skip forward duration**, **Player bar button size**).

### Seek bar and times

The seek bar sits under the buttons. Click or drag to seek; the time on the left follows the drag and the seek happens when you release. The time on the right is the song length. Click it to show the time left (for example `-2:31`) instead; the setting **Show remaining time** does the same.

Seeking while a DJ transition is running cancels that transition.

### Waveform seek bar

By default the seek bar is a plain slider. To show the loudness curve of the song instead:

1. Open **Settings → General → Player bar**.
2. Set **Playerbar slider** to **Waveform**.

The played part of the waveform is drawn in your accent colour. A click or drag seeks, as with the slider. These extra rows appear once you choose the waveform:

| Setting | Range | Default |
|---|---|---|
| **Waveform align** | Top, Center, Bottom | Center |
| **Waveform bar width** | 1 to 10 px | 2 px |
| **Waveform gap** | 0 to 10 px | 1 px |
| **Waveform radius** | 0 to 20 px | 4 px |
| **Waveform stretch** | on or off; scales the bars to the full height | off |
| **Waveform loading delay** | 0 to 10 s | 2 s |

Listen calculates the waveform from the song's audio, so it has to read the whole song once. For a song on a server this means an extra download, which is why the setting warns against the waveform on slow or metered connections. The result is cached, so a song is analysed only once. The **loading delay** postpones the calculation after a song change; raise it if you hear stutter when songs change.

Until the waveform is ready, and for songs without usable audio input, the plain slider is shown. A YouTube Music song gets a waveform only when a local copy of its audio already exists; the waveform never causes extra requests to YouTube.

### Right: buttons, rating and volume

The right-hand buttons can be reordered and hidden in **Settings → General → Player bar → Right-hand buttons**. The available buttons are:

- **Sleep timer** (see [below](#sleep-timer)).
- **Player config**, shown as **Player options**: output device, playback speed and pitch. See [Sound](sound.md).
- **Visualizer** and **Lyrics**: open the matching tab of the [full-screen player](fullscreen.md).
- **Favorite**: the heart. It is hidden when **Show favourites** is off in **Settings → General → Lists**.
- **Queue**: shows or hides the queue panel (or drawer).
- **Volume**: mute button and slider.

Some buttons appear on their own when they apply: the **Show video / Show song** switch for YouTube Music songs ([Music videos](videos.md)) and the cast button when speakers or TVs were found ([Casting](casting.md)).

**Ratings.** When the song's server supports ratings (Navidrome and other OpenSubsonic servers; never YouTube Music) and **Show ratings** is on, five stars appear above the buttons. Click a star to rate the playing song; click the same star again to clear the rating. The hotkeys "Rate 0 stars" to "Rate 5 stars" do the same.

**Volume.** Click the speaker to mute. Scroll the mouse wheel over the slider to change the volume by the **Volume wheel step** (5 by default). The slider width is **Volume slider width**. A right-click on the speaker opens the list of audio output devices. While you cast to a device, the volume control sets the device's volume.

**Open on click.** If you switch on **Player bar opens full-screen player**, a click on an empty part of the bar opens the full-screen player.

## The queue

The queue is the list of songs that play next. It can hold songs from several servers at once.

### Where the queue is shown

| Place | How to open it |
|---|---|
| Side panel | The **Queue** button in the player bar (hotkey "Toggle queue"). The default. |
| Drawer | Choose **Detached** for **Side play queue style** (below), then rest the pointer on the arrow tab at the right window edge, or press the **Queue** button to pin the drawer open. It slides in over the page and closes when the pointer leaves, unless you pinned it. |
| Now Playing page | Click the song title in the player bar, or enable **Now Playing** in the sidebar items (**Settings → General → Sidebar**). The page shows the whole queue at full width. |
| Full-screen player | The **Up Next** tab. |

Each place shows a header with the number of songs and the total time, a button naming the current play mode (a click moves to the next mode), and the queue itself.

Queue settings are in **Settings → Playback → Queue**:

| Setting | Effect |
|---|---|
| **Side play queue style** | **Attached** (a panel next to the page) or **Detached** (the drawer). |
| **Side queue layout** | **Horizontal** puts the queue in a column on the right, **vertical** puts it in a panel under the page. Drag the divider to resize. |
| **Follow current song** | Scrolls the queue to the playing song at each track change. On by default. |
| **Up next queue** | Labels songs added with Play next and Add to queue as "Up Next". On by default. |
| **Confirm queue changes** | Asks before something replaces the queue or before the queue is cleared. Off by default. |
| **Save play queue** | Keeps the queue between runs. On by default. |
| **Autosave play queue to server** and **Autosave every** | See [Saving the queue to your server](#saving-the-queue-to-your-server). |

**Settings → Playback → Player sidebar panels** controls what the right-hand sidebar holds. Besides the queue it can show a lyrics panel, a visualizer strip, or both combined in one panel, and you can set the **Panel order**. Drag the dividers between the panels to resize them. The queue panel itself can be switched off as long as another panel stays on.

### Working with the queue

- **Play a song:** double-click it, or use the play button that replaces the number when you hover a row.
- **Reorder:** drag rows up or down. Drag songs, albums or playlists from any list and drop them between the rows (or on the empty area under the last row) to insert them. You can also drop onto an empty queue to fill it.
- **Select:** `Ctrl`+click toggles a row, `Shift`+click selects a range. See [Lists, selection and menus](../using/lists.md) for the details.
- **Remove:** select rows and press `Delete` or `Backspace`. The playing song stays.
- **Search:** the field in the toolbar filters the queue by title, artist, album or genre.
- **Right-click** (or the row menu) offers **Remove from queue**, **Move to top**, **Move to next**, **Move to bottom** and **Shuffle selected**.

Songs that were analysed for the DJ features show their tempo and key after the artist ("Artist · 124 BPM · 8A").

### Queue tools

The toolbar above the queue has these buttons:

| Button | What it does |
|---|---|
| **Save queue to server** / **Restore queue from server** | Only on servers that keep a play queue. See below. |
| **Shuffle queue** | Shuffles the queue in three groups: the songs before the playing one, the Up Next block and the rest. The playing song keeps its place. |
| **Clear queue** | Empties the queue and stops playback. Asks first if **Confirm queue changes** is on. |
| **Jump to playing song** | Scrolls to the playing song. |
| **Create playlist from queue** | Starts a new playlist with every song in the queue. |
| Search field and list options | Filter the rows; the options button at the right end sets columns and similar list options. |

In a narrow queue the buttons fold into a **…** menu, from the least to the most important one.

### Queued songs and DJ songs

The queue distinguishes three kinds of songs:

1. **The rest of the queue:** the songs of the album, playlist or list you started.
2. **Up Next:** songs you added with **Play next** (they go to the top of the block) or **Add to queue** (they go to the end of the block). They play right after the current song, ahead of everything else. With **Up next queue** on, the block gets "Up Next" and "Queue" label rows; with it off, such songs carry a **Queued** tag until they play.
3. **DJ songs:** songs the [Auto-DJ](play-modes.md#auto-dj) or the [AI DJ](../dj/ai-dj.md) appended at the end. They carry an **Auto-DJ** tag. Leaving the Auto-DJ mode does not remove songs it already added; delete them like any other song.

Dragging a row into the Up Next block adds it to the block; dragging it elsewhere takes it out. A marker row "AI DJ break" shows where the AI DJ will speak.

### Player filters

**Settings → Playback → Player filters** keeps unwanted songs out of the queue. Press **Add rule**, then choose a field (title, artist, genre, year, play count, rating and others), an operator (for example **contains**, **is**, **starts with**, **matches regex**) and a value. A song that matches an active rule is left out when you play or queue it, and Listen says how many were kept out. Songs the Auto-DJ and AI DJ add are not filtered. Each rule has an on/off switch.

## Queue restore

With **Save play queue** on, Listen writes the queue and the position of the playing song to disk every few seconds and when you quit. At the next start the queue is back, the playing song is selected and the position is remembered, but nothing plays by itself. Press Play to continue where you stopped. Shuffle and repeat are restored as well.

## Saving the queue to your server

Navidrome and other OpenSubsonic servers can store one play queue per user, so that another app can continue where you stopped. The two buttons appear in the queue toolbar when the server you are browsing supports this.

- **Save queue to server** sends the songs of that server (songs from other servers are left out), the playing song and the position. A message tells you how many songs were saved.
- **Restore queue from server** replaces your queue with the saved one. Nothing starts playing; the first Play resumes at the saved position. If **Confirm queue changes** is on, Listen asks before replacing.

To save automatically, switch on **Autosave play queue to server** in **Settings → Playback → Queue** and set **Autosave every** (1 to 20 track changes; default 1).

## Sleep timer

The **Sleep timer** button (a clock icon) in the player bar opens a small panel:

- **End of song** pauses at the end of the playing song.
- **End of album** pauses once the album of the playing song is over. If you start another song yourself, the timer follows that song's album. It is not available while Shuffle is on.
- The grid starts a timer of 5, 10, 15, 30, 45, 60, 120, 180 or 240 minutes.
- **Custom** takes hours, minutes and seconds (up to 23 h 59 min 59 s).

While a timed timer runs, the remaining time is shown on the button. The countdown only runs while music plays, so pausing does not use up your time. When the timer ends, playback pauses and Listen shows a short message. **Cancel** in the panel turns it off. The sleep timer is not kept when you quit Listen.
