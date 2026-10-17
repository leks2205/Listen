# Full-screen player and Now Playing

The full-screen player shows the cover large with the song details, and one panel next to it: the queue, similar songs, lyrics or the visualizer. This page also covers its settings, the idle auto-open and the Now Playing page.

## Opening and closing

Open the full-screen player in any of these ways:

- Click the cover in the player bar.
- Press the **Lyrics** or **Visualizer** button in the player bar. They open the full-screen player on that tab.
- Use the hotkey **Toggle full-screen player** (see [Hotkeys](../integration/hotkeys.md)).
- Switch on **Player bar opens full-screen player** (**Settings → General → Player bar**) and click an empty part of the bar.
- Switch on the song/video switch for a YouTube Music song ([Music videos](videos.md)); the video is shown here.

Close it with the **Minimize** button (the arrow at the top left), the same hotkey, or `Esc`.

When nothing is playing, the window says "Nothing is playing."

## What you see

The top-left corner has two buttons: **Minimize** and **Configure** (the gear, see [below](#settings-of-the-full-screen-player)). The top-right corner has four tab buttons:

| Tab | Content |
|---|---|
| **Up Next** | The play queue, with the same toolbar as the side panel. See [Player bar and queue](queue.md). |
| **Related** | Songs similar to the playing one (up to 50), when the song's server can provide them. Right-click or use the menu on a row as usual. |
| **Lyrics** | The [lyrics](lyrics.md) with their buttons. |
| **Visualizer** | The [visualizer](visualizer.md) (spectrum, or Milkdrop where available). |

Click a tab to open its panel; click it again to close the panel, and the cover moves to the middle. One panel is open at a time.

On the left are the cover (or the music video) and, under it, the title, the artists, the album and badges with technical details such as the codec. The title and the artists are links. A long title scrolls like a marquee unless you chose wrapped titles.

The background takes its colour from the cover and changes with each song.

The visualizer tab has an **Expand visualizer** button at its top right that fills the whole window with the visualizer, with the song name in a corner. Use **Shrink visualizer** or `Esc` to go back.

In a narrow window (768 px or less) Listen uses a phone-style layout instead: a header, the cover, the details, a progress bar and the transport buttons, and a bottom row with shuffle, repeat, queue and lyrics. The queue and the lyrics replace the cover while they are open.

## Settings of the full-screen player

There are two places:

1. **Settings → General → Full-screen player** has **Full-screen auto-open timeout** and **Configure…**. The same dialog opens from the gear inside the player.
2. **Configure** opens the dialog **Full-screen player**. Changes apply live behind the dialog; press **Done** to close it.

| Setting | Range | Default | Meaning |
|---|---|---|---|
| **Dynamic background** | on or off | on | Colours the background from the cover. Off uses the theme background. |
| **Use the cover as background** | on or off | off | Shows the blurred cover instead of a flat colour. |
| **Image blur** | 0 to 6 rem | 6 | Blur of that image. |
| **Opacity** | 0 to 100 % | 25 | How strongly the background is darkened. |
| **Use the cover's aspect ratio** | on or off | off | Shows non-square covers whole, instead of cropping them to a square. |
| **Cover size** | 55 to 100 % | 75 | Cover width as a share of its column. The cover also never gets taller than the room above the song details. |
| **Alignment** | Left, Center, Right | Center | Where the cover and the text sit in their column. |
| **Title** | **Scrolling** or **Wrapped** | Scrolling | One line that scrolls when too long, or a title that wraps. |
| **Title lines** | 1 to 10 | 1 | Lines for a wrapped title. Shown only for **Wrapped**. |
| **Metadata badges** | checkboxes | Codec, Release year | The badges under the title. |

The available badges are **Codec**, **Bit rate**, **Bit depth**, **Sample rate**, **BPM**, **Year**, **Release year**, **Date**, **Release date**, **Release type**, **Genres**, **Disc number** and **Track number**. A badge is skipped when the song has no data for it.

## Auto-open

To open the full-screen player on its own, for example when the computer acts as a music display, set **Full-screen auto-open timeout** to a number of minutes (1 to 120). After that many minutes without a mouse move, click, scroll or key press, while a song is playing, the player opens. Press `Esc` to return. The default, 0, switches the feature off.

## Now Playing page

The **Now Playing** page is the queue as a full-width page, with the same toolbar and tools as the side panel. It also shows the number of songs, the total time and the play-mode button. Open it by clicking the song title in the player bar. To add it to the sidebar, enable **Now Playing** in the list of items in **Settings → General → Sidebar**; it is hidden by default.

While the page is open, the side panel and drawer are not shown, since the page already shows the queue.
