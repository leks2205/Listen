# Lyrics

Listen shows the lyrics of the playing song, highlights the line being sung and, when the lyrics carry word timings, the word. It can search other providers by hand, translate lines, show Japanese readings and export lyrics to a file.

## Where to see lyrics

- The **Lyrics** button in the player bar opens the [full-screen player](fullscreen.md) on its **Lyrics** tab.
- A lyrics panel in the right sidebar: switch on **Lyrics panel** in **Settings → Playback → Player sidebar panels**. It can sit under the queue, or share one panel with the visualizer (**Combine lyrics and visualizer**).

Lyrics that follow the song ("synchronized") scroll along and light up the current line. Plain lyrics without timing are shown as static text. If no lyrics are found, the view says "No lyrics found for this song."

## Where lyrics come from

When a song starts, Listen looks for lyrics in this order:

1. A lyrics version you picked by hand for this song earlier (see [Search](#searching-for-other-lyrics)).
2. The song's own server: Navidrome and other OpenSubsonic servers, and YouTube Music, can provide lyrics themselves.
3. The online providers you enabled.

With **Prefer local lyrics** on (the default), the server's or YouTube Music's synchronized lyrics are used without asking any provider. Plain server lyrics give way to synchronized lyrics from a provider. With it off, a provider result wins whenever one is found.

The providers are asked one after the other in your order. The first that has synchronized lyrics for the song wins. If none has, the best plain-text match of the earliest provider is used. A result is only trusted if title and artist are close enough to the playing song.

| Provider | Notes |
|---|---|
| **NetEase** | Often has synchronized lyrics, sometimes word by word, and its own translations. Enabled by default, asked first. |
| **lrclib.net** | A community database of synchronized and plain lyrics. Enabled by default, asked second. |
| **Genius** | Plain lyrics only. Off by default. |
| **SimpMusic** | A community lyrics service. Off by default. |

Each lookup sends the song's title, artist, album and length to the providers you enabled. Nothing is sent when **Fetch lyrics** is off.

To change this, open **Settings → Playback → Lyrics**:

- **Fetch lyrics** turns the lookup on or off. It covers the server's lyrics as well as the providers. With it off, Listen finds no lyrics, apart from versions you picked by hand.
- **Lyric providers** lists the four providers. Use the switch to enable one and the arrows to change the order.
- **Prefer local lyrics**, as described above.

The **Show the provider** and **Show the match** switches in the display settings add a line naming where the lyrics came from (and which song the provider matched).

## Using the lyrics view

Under the lyrics are two rows of buttons:

| Button | What it does |
|---|---|
| **EXPORT** | Saves the lyrics to a file (see [below](#exporting-lyrics)). Shown when lyrics are loaded. |
| Version list | Appears when more than one version was found (for example from the server and from a provider). Pick one; each entry names its provider and whether it is synced or plain. |
| **TRANSLATION** | Shows or hides translations under each line. Appears when the lyrics carry a translation or when you set up an online translation service. |
| **DISPLAY** | Opens the display settings. |
| **SEARCH** | Opens the lyrics search. |
| **−** / number / **+** | Moves the lyrics against the music in steps of 50 ms (tooltips "Lyrics earlier" and "Lyrics later"). You can also type a value. |
| **CLEAR** / **REFRESH** | **CLEAR** removes the lyrics of this song, including a version you picked. **REFRESH** looks for them again. |

### Offset

The number between **−** and **+** is the shift in milliseconds for this song. The default for all songs is **Lyric offset** in **Settings → Playback → Lyrics** (-2000 to 2000 ms, default 0). A per-song change lasts for the current session.

### Searching for other lyrics

If the automatic lookup picked the wrong song or no lyrics:

1. Press **SEARCH**. The title and artist of the playing song are filled in.
2. Change them if needed and press `Enter` or **Search**. Every enabled provider is asked at once.
3. Select a result to preview it. Results are sorted with synchronized ones first, and marked synchronized or unsynchronized.
4. Press **Apply** to use it. Listen remembers your choice for this song, also after a restart.

**Export** in this dialog saves the previewed result.

### Display settings

**DISPLAY** (and the same rows at the end of **Settings → Playback → Lyrics**) sets:

| Setting | Range | Default |
|---|---|---|
| **Alignment** | Left, Center, Right | Center |
| **Lyric size (synchronized)**, **(unsynchronized)** | 8 to 72 px | 24 px |
| **Lyric gap (synchronized)**, **(unsynchronized)** | 0 to 50 px | 24 px |
| **Left padding**, **Right padding** | 0 to 50 % of the width | 0 |
| **Follow the current lyric** | on or off | on |
| **Follow position** | -50 (top) to 50 (bottom) | 0 (centred) |
| **Line lead time** | 0 to 3000 ms | 800 ms |
| **Opacity of other lines** | 0 to 100 % | 20 % |
| **Size of other lines** | 50 to 100 % | 95 % |
| **Show the match**, **Show the provider** | on or off | off, on |

**Line lead time** makes a line light up that many milliseconds before its time stamp, so you can read ahead. Lines in right-to-left scripts such as Arabic and Hebrew are aligned to the right.

## Karaoke highlighting

When lyrics have word timings (enhanced LRC files, or NetEase's word-by-word format), the words of the current line light up as they are sung. There is no setting for this; it happens whenever the lyrics contain word timings and are synchronized.

## Japanese: romaji and furigana

Two optional extras help with Japanese lyrics:

- **Romaji** shows the line in Latin letters under it. Kana are converted directly; kanji are read through the built-in dictionary when your build has one.
- **Furigana** shows the reading of the kanji above them. It needs the Japanese dictionary that normal builds include. If your build lacks it, the setting says so and stays off.

Both are off by default. Chinese text without kana is left alone.

## Translation

There are two kinds of translation:

1. **NetEase translations.** Switch on **NetEase translations** to fetch NetEase's own translation along with the lyrics and show it under each line.
2. **Online translation with your own key.** Listen can send the lyrics to Microsoft Azure Translator or Google Cloud Translation. Set it up in **Settings → Playback → Lyrics**:
   1. Pick **Translation provider**: **Microsoft Azure** or **Google Cloud**.
   2. Paste your API key into **Translation API key**. You need your own account with that provider; Listen has no key of its own.
   3. Choose **Translation language** (31 languages, English by default).
   4. Press **TRANSLATION** in the lyrics view to translate the playing song. Turn on **Automatic translation** to translate every song's lyrics as they load.

The whole text of the lyrics goes to the provider you chose in one request, with your key, and nothing else. The key is stored in Listen's configuration file, so treat that file as private (see [Privacy and security](../more/privacy.md)). The provider may charge for usage under your account.

## Exporting lyrics

**EXPORT** opens a preview of the file Listen would write. For synchronized lyrics you can keep **Synchronized (.lrc)** on (with an **Offset (ms)** written into the file) or switch it off to get plain text. The file is saved to your Downloads folder as `Artist - Title.lrc` or `.txt`, and the dialog shows the exact name. Word timings and translations are kept in the `.lrc` file.
