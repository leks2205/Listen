# Heart-rate tempo

**Heart-rate tempo** plays songs whose tempo fits your heart rate, for a run, a ride or a workout at home. It needs Bluetooth on the computer and a Bluetooth heart-rate sensor that uses the standard Heart Rate Service: most chest straps do, and so do watches that can broadcast their heart rate. Listen does not stretch songs to the beat. It only chooses songs that already fit.

Music does not measurably change your heart rate during exercise. The mode picks music that fits your effort. It is not training guidance.

## Switching it on

The heart button sits next to the play-mode button in the [player bar](queue.md), and next to the repeat button in the narrow [full-screen player](fullscreen.md). You can switch it at any time. The song that plays keeps playing. The button is highlighted while the mode is on, and once readings come in it shows the target tempo ("142"). Its tooltip shows the status:

- **Heart-rate tempo off**
- **Looking for a heart-rate source…**: Listen is scanning for a sensor.
- **No heart-rate source found**: no sensor is in range, or Bluetooth is off or missing. Listen keeps looking every few seconds, and everything else works as usual.
- **Heart-rate tempo on**: readings come in. The tooltip also shows the target and the sensor's name.

The **Toggle heart-rate tempo** [hotkey](../integration/hotkeys.md), the switch in **Settings → Playback → Heart-rate tempo** and the MCP server's `set_heart_rate` tool switch it too.

The tempo needs a DJ to choose the songs. In **Off** or **Shuffle**, switching the mode on also switches the [play mode](play-modes.md) to **Auto-DJ**. In **AI DJ** the AI DJ keeps going. Switching the mode off leaves the play mode as it is.

## The sensor

When the mode is on, Listen scans for a Bluetooth device that offers the Heart Rate Service. It connects to the first one it finds and subscribes to its heart-rate readings. The sensor is not paired with the computer. Listen remembers the sensor and looks for it first next time. Other sensors are only taken when it doesn't show up within a few seconds. **Settings → Playback → Heart-rate tempo → Forget sensor** clears the remembered one. If the link drops (out of range, strap taken off), Listen connects again on its own.

Wear the strap before you switch the mode on. Most straps only send while they have skin contact.

On Linux, Listen talks to BlueZ (the system's Bluetooth service) over D-Bus. On Windows it uses the system Bluetooth stack. The computer needs Bluetooth Low Energy (Bluetooth 4.0 or later). Step cadence (the phone app's mode without a sensor) is not available on the computer.

## How songs are chosen

Readings are smoothed over about 30 seconds, so one odd reading doesn't change the music. The target is your heart rate in beats per minute, clamped to 70–180, ± 8 BPM. Half and double time count: at 150 a 75 BPM song fits, and at 70 a 140 BPM song fits. The fit is judged at the faster pulse, so at 70 the songs that fit are 62–78, 132–148 and 31–39 BPM. A 130 BPM song does not fit 70: it is 10 BPM off 140. Tempos come from the [song analysis](../dj/analysis.md). Songs without one count as unknown, unless their tags give a BPM.

- **Re-ranking.** When the target has moved more than 5 BPM since the last time, the upcoming songs the DJs picked are sorted again. Songs within 4 BPM stay, closest tempo first, and the rest are taken out. The song right after the current one also stays, because it may already be loaded or mixing in. Songs you added yourself are never touched.
- **Fills.** Every Auto-DJ fill and every AI DJ set aims at the target. The fetched candidates that fit come first. When too few fit, Listen tops them up from the analysis store: songs from your [history](../using/history.md) and analysed YouTube Music songs that fit, closest to the current song's sound. Songs that are queued, played this session, snoozed, hidden ("Not interested") or resting as worn out never come back this way. Only when even the store has too few does the fill pad with the nearest songs that don't fit, so the music never stops. The log says so: `heart-rate tempo: only N songs fit 142, padded with M nearest`.
- **AI DJ.** The AI DJ is told that heart-rate mode is on and the tempo is a hard requirement. Every candidate is marked "tempo fits the heart rate" or "tempo does NOT fit the heart rate, avoid".
- **Why this song.** Auto-DJ picks that fit read "Matches your tempo: 142 BPM" (see [Why this song?](queue.md)).
- The forgotten-favourite slot is skipped while the mode is on, because an old favourite at another tempo would break the run.

Switch the mode off and the next fills are ordinary again. Tempo picks already in the queue stay.

## Testing without a strap

`LISTEN_HR_FAKE=70:180,150:300,90:300 listen` replaces the sensor with a scripted source: steps of `bpm:seconds`, one reading per second, repeated. Demo mode (`listen --demo`) plays this rest, run and cool-down curve by default. The MCP tool `set_heart_rate` takes `on` and `sample_bpm`, which feeds one reading as if it came from the sensor. `get_now_playing` reports the mode as `heart_rate`: `on`, `status`, the last `bpm`, the `smoothed` rate, the `target`, the `source`, the remembered `sensor`, the number of `reranks` and what the `last_fill` took.
