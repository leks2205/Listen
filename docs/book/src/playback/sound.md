# Sound

Listen plays through [mpv](https://mpv.io), so most sound settings map directly to mpv features. This page covers the output device, gapless playback, volume levelling, ReplayGain, the equalizer and compressor, speed and pitch, fades, streaming quality and transcoding, extra mpv options, and the server jukebox.

Most of these settings are in **Settings → Playback**, in the sections **Playback**, **Audio output**, **Volume levelling**, **ReplayGain**, **mpv**, **Equalizer**, **Headphone correction**, **Compressor** and **Transcoding**. Output device, speed and pitch are also in the **Player options** popover (the sliders icon in the player bar).

> **Note:** Equalizer, headphone correction, compressor, volume levelling, ReplayGain, speed, pitch and fades are applied by mpv on this computer. They have no effect while you [cast to a device](casting.md) or use the server [jukebox](#jukebox-mode).

## Output device

To choose where sound comes out:

- **Settings → Playback → Audio output → Audio device**, then **Refresh** if the device is not listed; or
- right-click the speaker icon in the player bar; or
- use **Audio device** in the **Player options** popover.

The first entry, **System default**, follows the operating system's default device. The other entries are the devices mpv reports. If the list cannot be read, the error is shown under the row.

More output options in the same section:

| Setting | Meaning |
|---|---|
| **Sample rate** | Output rate in Hz, for example `48000`. Empty (or below 8000) uses the device default. Press `Enter` or **Apply**. |
| **Exclusive mode** | Takes the sound device for Listen alone (mpv `audio-exclusive`; WASAPI on Windows and some ALSA setups). Other programs are silent meanwhile. Off by default. |
| **Audio format** | Sample format sent to the sound card: **auto**, `s16`, `s32`, `float` or `u8`. |
| **Preserve pitch**, **Microtonal pitch controls** | See [Speed and pitch](#speed-and-pitch). |

## Gapless playback

**Settings → Playback → Playback → Gapless playback** sets mpv's `gapless-audio`:

- **weak** (default): gapless, except when the audio format changes between two songs.
- **yes**: always gapless.
- **no**: a short gap between songs.

Listen prepares the next song while the current one plays, so albums and playlists play without a break. Crossfading between songs is not available as a setting; the [DJ transitions](../dj/transitions.md) blend songs that the Auto-DJ or AI DJ picked, and albums stay gapless. Gapless means no silence: each song still fades in and out over **Fade songs in and out** (1.5 s by default, see [Track fades](#track-fades)), also between album tracks; set it to 0 for fully seamless albums.

## Track fades

Two settings in the **Playback** section smooth the start and end of songs:

- **Fade songs in and out:** each song fades in at its start and out at its end over this time. Range 0 to 10 s in steps of 0.5 s; default 1.5 s; 0 turns it off. The AI DJ lowers and raises the music at this speed too (at least 1 s).
- **Fade on pause:** fades out briefly when you pause and back in when you resume. On by default.

## Volume levelling

Volume levelling plays every song equally loud, so a quiet ballad after a loud dance track needs no hand on the volume. It is on by default. Choose the level in **Settings → Playback → Volume levelling → Level**:

| Level | Loudness |
|---|---|
| **Off** | Songs play as they are (ReplayGain applies, if set). |
| **Quiet** | −19 LUFS: room for very dynamic music. |
| **Normal** (default) | −14 LUFS, the level streaming services use. |
| **Loud** | −11 LUFS. |

How loud a song is comes from the first of these that knows it:

1. the [analysis](../dj/analysis.md) of the song (its measured loudness),
2. YouTube's own loudness value, for YouTube Music songs that are not analysed yet (Listen asks YouTube once per song),
3. the song's ReplayGain tags,
4. nothing: the song plays unchanged.

Loud songs are turned down as far as needed; quiet songs are turned up by at most 6 dB, and a limiter at the end of the sound chain keeps boosted peaks below −1 dB, so nothing clips. Changing the level while a song plays glides to the new loudness instead of jumping. **Get info** on a song shows the gain it gets and where its loudness came from (row **Volume levelling**); the log names it too (`levelling: <title> −4.2 dB from analysis`).

While volume levelling is on, it replaces ReplayGain (it reads the ReplayGain tags itself), so the ReplayGain settings below only matter with **Off**.

## ReplayGain

ReplayGain evens out the loudness of songs that carry ReplayGain tags. In **Settings → Playback → ReplayGain**:

| Setting | Meaning |
|---|---|
| **Mode** | **Off** (default), **Track** (each song to the same loudness) or **Album** (keeps the loudness differences inside an album). |
| **Preamp** | Gain added on top of the ReplayGain value, -15 to +15 dB. |
| **Prevent clipping** | Lowers the gain if the peak would exceed full scale. On by default. |
| **Fallback gain** | Gain in dB for songs without ReplayGain tags, -15 to +15 dB. |

## Equalizer

The equalizer has twelve bands at 31.5, 63, 125, 250, 500 Hz and 1, 2, 3, 4, 6.3, 10 and 16 kHz, each from -12 to +12 dB in steps of 0.5 dB, plus a **Pre** (preamp) slider with the same range.

To use it:

1. Open **Settings → Playback → Equalizer** and switch **Equalizer** on.
2. Drag the sliders, or pick a preset under **Presets**.

The built-in presets are Acoustic, Bass Boost, Bass Cut, Classical, Electronic, Flat, Hip-Hop, Jazz, Loudness, Pop, Rock, Treble Boost, Treble Cut, V-Shape and Vocal Boost. Choosing a preset sets the bands and resets the preamp to 0.

To save your own preset, set the bands, type a name into **Save current as…** and press **Save**. A saved preset appears in the list after the built-in ones, can be deleted with **Delete**, and takes precedence over a built-in preset of the same name. **Reset** sets all bands and the preamp back to 0.

## Headphone correction

Headphone correction bends your headphones' sound toward a neutral target, using the measured profiles of [AutoEq](https://github.com/jaakkopasanen/AutoEq) (MIT licence). About 6000 headphone models are built into Listen, one profile per model (measurements by oratory1990 first, then crinacle, then the other sources), so nothing is downloaded. The correction runs first in the sound chain, before the equalizer, so the equalizer stays your own taste on top of a neutral sound.

A profile belongs to an **output**, not to Listen as a whole:

1. Play through your headphones (connect them, or pick them as the **Audio device**).
2. Open **Settings → Playback → Headphone correction**. **This output** names the output that plays now.
3. Type the model into **Find your headphones** (for example `WH-1000XM5` or `HD 600`; hyphens and case don't matter) and pick it. When the output's name already says which headphones it is (Bluetooth headphones usually do), Listen suggests the profile before you type.

From then on the profile is applied whenever that output plays, and taken off when another one does. With the audio device on **System default**, Listen follows the system's default output (on Linux: `pactl get-default-sink`, checked twice a second), so connecting Bluetooth headphones or plugging in a wired pair switches the profile within a second. While a DJ transition plays (from loading the next song until the blend has ended) the switch waits and then uses the output that plays at that moment; a transition that is only planned doesn't hold it up.

On Android the same section is in **Settings**, above the **Equalizer**. The phone binds a profile to the output media plays on (Bluetooth, wired, USB or the speaker, by the device's name), so the same headphones over Bluetooth and over a cable are two outputs, and a switch applies at once, also during a transition.

You can also paste the text of an AutoEq or Equalizer APO `ParametricEQ.txt` into the search box and choose **Use pasted profile**. Listen reads the preamp (several `Preamp:` lines add up, as in Equalizer APO) and the peak and shelf filters (`PK`, `PEQ`, `LS`, `LSC`, `HS`, `HSC`, with `Q`, `BW Oct` or a slope). Lines it can't apply, such as other filter types, a peak without Q or bandwidth, or channel and device sections, are left out, and the box says how many. **Other outputs** lists the profiles bound to outputs that don't play now; **Remove** takes one away. **Headphone correction** (on by default) turns all of them off without forgetting them.

Every profile keeps its preamp (limited to -30 to 0 dB), lowered further when its bands boost more than it leaves room for. Listen finds the true peak of the correction, also of a very narrow band, so no frequency from 20 Hz to 20 kHz comes out louder than it went in and the correction itself never clips; volume levelling's limiter comes after it as usual.

## Compressor

The compressor evens out loud and quiet passages. It uses ffmpeg's `acompressor` filter inside mpv. It runs after the equalizer.

Switch **Compressor** on in **Settings → Playback → Compressor**, then pick a preset (Broadcast, Default, Gentle, Heavy, Light, Limiter, Loud Master, Moderate) or set the values:

| Setting | Range | What it does |
|---|---|---|
| **Threshold** | -60 to 0 dB | The level above which compression starts. |
| **Ratio** | 1 to 20 (n:1) | How strongly loud parts are reduced. |
| **Attack** | 0.1 to 2000 ms | How fast the compressor reacts. |
| **Release** | 1 to 9000 ms | How fast it lets go again. |
| **Makeup gain** | 0 to 30 dB | Gain added after compressing. |
| **Knee** | 1 to 8 dB | Width of the soft knee around the threshold. |

Your own compressor presets work like the equalizer presets: type a name, press **Save**.

## Speed and pitch

Open the **Player options** popover (sliders icon in the player bar). **Playback speed** runs from 0.5x to 2x. Press **1x** to reset. For songs that have been [analysed](../dj/analysis.md), the readout also shows the resulting tempo, for example `1.25 x / 150.0 BPM`.

By default **Preserve pitch** is on: the music gets faster or slower and keeps its pitch. Switch it off (in the popover or in **Settings → Playback → Audio output**) to let speed and pitch move together, like a record. Then a row of pitch buttons appears:

- **-1st** and **+1st** change the pitch by one semitone.
- With **Microtonal pitch controls** on (**Settings → Playback → Audio output**), **-10ct** and **+10ct** change it by ten cents.

The readout between the buttons shows the speed and the pitch shift, for example `1.12x +1.96st`. Pitch steps are speed steps: a semitone is a speed change by the twelfth root of two, within the 0.5x to 2x limit.

> **Note:** The speed controls are disabled while DJ transitions are possible, because the transitions change the songs' speed to match their tempos. That is the case when **DJ transitions** is on and the Auto-DJ or the AI DJ is on, which is the default setup. To change the speed, switch DJ transitions off in **Settings → Playback → DJ transitions & song analysis**, or leave the Auto-DJ and AI DJ modes (for example choose **Off**). Speed and pitch have no effect while casting.

## Streaming quality and transcoding

**Streaming quality** (**Settings → Playback → Playback**) limits the bit rate to save bandwidth:

| Choice | Limit |
|---|---|
| **Original (best)** | The file as stored. Default. |
| **High (320 kbit/s)** | Up to 320 kbit/s. |
| **Normal (192 kbit/s)** | Up to 192 kbit/s. |
| **Low (96 kbit/s)** | Up to 96 kbit/s. |

On Navidrome the server converts the song to MP3 up to the chosen rate; YouTube Music picks a smaller audio format. Local files, downloaded songs and the copies used for DJ analysis stay original.

If you want control over the format, use the **Transcoding** section. It appears when you have at least one server that is not YouTube Music. Switch **Transcode** on to ask Navidrome or Jellyfin to convert songs while streaming. It replaces **Streaming quality** for those servers; YouTube Music keeps that setting. New values apply to songs that start from then on.

| Setting | Meaning |
|---|---|
| **Format** | `mp3`, `opus`, `flac` and so on. Empty lets the server choose (Jellyfin: MP3). |
| **Maximum bit rate** | In kbit/s; 0 or empty lets the server pick. |
| **Maximum sample rate** | In Hz, Jellyfin only; empty means no limit. |

## mpv: executable and extra options

The **mpv** section of **Settings → Playback** is for special cases:

- **Executable path:** the mpv program to run. Empty uses `mpv` from your `PATH`. On Windows the setup wizard and **Settings → Advanced → Helper programs** manage mpv for you.
- **Extra parameters:** extra mpv options, one per line, for example `--audio-channels=stereo`. A missing leading `--` is added for you. See the [mpv manual](https://mpv.io/manual/stable/#audio) for options.
- **Reload mpv:** stops playback and starts mpv again with the path and parameters above. Changes to path and parameters need this step. The **Player options** popover has a **Reload** button too.

Wrong parameters can stop playback from working. Clear the box and reload to go back to the defaults.

## Jukebox mode

Some Subsonic-style servers can play music through their own speakers, controlled by a client. If the server answers Listen's jukebox request, an **Audio player** row appears at the top of **Settings → Playback → Playback**:

- **mpv (this computer)** is the default.
- **Jukebox (the server plays)** hands the queue, starting at the playing song, to the server. Listen then only controls it: play, pause, stop, seek and volume are sent to the server, and the position is polled every two seconds.

The server administrator has to switch the jukebox on first. Listen sends up to 100 songs of the playing song's server at a time. Equalizer, compressor, ReplayGain, speed and the DJ features do not apply to the jukebox. If the server turns out to have no jukebox, Listen plays with mpv again and tells you.
