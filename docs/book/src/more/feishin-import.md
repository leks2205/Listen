# Importing from Feishin

Listen's interface is modelled on [Feishin](https://github.com/jeffvli/feishin), and it can read a Feishin settings export so you do not have to set everything up again. This page explains where to find the option, what is taken over and what is not.

## How to import

1. Get a Feishin settings file: a JSON export of Feishin's settings.
2. In Listen open **Settings → Advanced** and scroll to **Import from Feishin**.
3. Type or paste the path to the file in **Feishin settings file**, for example `~/Downloads/feishin-settings.json`. A leading `~` is your home folder.
4. Press **Import** (or Enter).

> **Tip:** Instead of typing the path you can drag the file into the Listen window while Settings is open. It is imported right away.

Listen shows "Feishin settings imported." and applies the values at once. If the file cannot be read or is not a Feishin file, an error message tells you why, and nothing changes.

The import works the same on Linux and Windows, because it only reads the file you point to. Listen does not look for Feishin's own configuration folder by itself, so you always give the path.

> **Note:** The import changes the settings in place and does not ask for confirmation. If you want a way back, first make a copy with **Export settings** (see [Backup, logs and diagnostics](backup.md)).

## What is imported

Values that are missing from the file, or have a type Listen does not accept, keep their current value in Listen. Only what the file contains is applied.

| Area | Taken over |
|---|---|
| Appearance | Theme, follow system theme, dark and light theme, accent colour, "use the theme's accent", font, zoom, album and artist background blur |
| Sidebar | Item order and visibility, playlist list, playlist folders and separator, sidebar image, queue in the sidebar, panel order |
| Home | The feature carousel and which home sections are shown |
| Artist pages | Section order and visibility, release types, radio size, external links (Last.fm, MusicBrainz, ListenBrainz, Qobuz, Spotify) |
| Player bar | Info items, skip buttons and their seconds, volume wheel step, open drawer, volume width, button size, the waveform slider and its options, full-screen auto-open time |
| Playback | Gapless mode, ReplayGain mode, preamp, fallback and clipping, audio device, extra mpv parameters, fade on pause, equalizer and its preamp, preserve pitch, sample rate, exclusive mode, compressor, transcoding, scrobble thresholds |
| Auto-DJ | Enabled, item count, timing, duplicates, strategy |
| Lyrics | Fetching, following, alignment, sources, delay, local lyrics first, provider and match labels, romaji, furigana, translation settings, font sizes, gaps, padding and opacity |
| Visualizer | The analyzer options and custom presets (built-in preset names are skipped) |
| Hotkeys | Key bindings (which also decide what is "global"), global media keys |
| Window | Tray, minimize and exit to tray, start minimized, window bar style and track info |
| Discord | Enabled, application ID, "listening" mode, paused state, state icon, server image, display type, link type |
| Scrobbling | Minimum mode, notify on song change |
| Remote | Enabled, port, user name |
| Query builder | Your custom smart-playlist tags |

## What is not imported

- **Servers and sign-ins.** Add your servers in Listen as usual (see [Connecting your music](../getting-started/sources.md)).
- **Passwords.** The remote-control password in the file is ignored.
- **Hotkeys you did not set in Feishin.** The bindings in the file replace your current ones; actions the file does not list keep Listen's defaults.
- **Settings that only exist in one app.** Anything Feishin has and Listen does not, and the other way round, is left alone.

> **Note:** If the file contains a lyrics translation API key, it is imported with the other lyrics settings and kept in Listen's `config.toml`. Delete it in Settings if you do not want it there.

## Listen's own export is different

**Settings → Advanced → Settings backup** writes and reads Listen's own format. If you give such a file to **Import settings** and it turns out to be a Feishin file, Listen tells you to use **Import from Feishin** instead. The other way round, the Feishin import expects Feishin's layout and does not read Listen's export.

See also [Themes and colours](../appearance/themes.md) for the Feishin themes that come with Listen.
