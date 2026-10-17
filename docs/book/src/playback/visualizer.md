# Visualizer and Milkdrop

Listen draws a live picture of the music. The built-in visualizer is a spectrum analyzer with the same options as Feishin's (audioMotion). On Linux you can switch to Milkdrop, which draws animated presets through libprojectM.

## Where it appears

- **Full-screen player:** the **Visualizer** tab. The **Expand visualizer** button there fills the whole window; `Esc` or **Shrink visualizer** goes back. See [Full-screen player](fullscreen.md).
- **Right sidebar:** a strip under the queue. It is on by default. Turn it on or off with **Visualizer panel** in **Settings → Playback → Player sidebar panels** (or **Visualizer in queue** in the **Player options** popover). You can also combine it with the lyrics panel and change the panel order there.
- The player bar's **Visualizer** button opens the full-screen player on the visualizer tab.

While the visualizer is not on screen, Listen does not capture or analyse audio.

## What it listens to

The visualizer analyses the audio Listen itself plays. On Linux it records Listen's own playback streams through PipeWire (`pw-record` and `pw-dump` must be installed, which they are with a standard PipeWire setup). Other programs and the AI DJ's voice are not included, and the volume slider does not change the picture. On Windows it uses a loopback of the default output device, so it shows whatever that device plays, which includes other programs.

If the capture fails, the panel shows the reason instead of a picture. The visualizer shows nothing while you [cast](casting.md) or use the server jukebox, because Listen's own audio is silent then.

## Spectrum options

Open **Settings → Playback**, find the **Visualizer** section and press **Configure…** next to **Visualizer settings**. The dialog **Visualizer settings** has the options below. Their names and ranges follow Feishin and audioMotion, and the dialog changes the picture live.

**Presets.** **Select Preset** picks a saved set of options. Four presets (Preset 1 to 4) come with Listen. To keep your own, type a name under **Preset Name** and press **Save as Preset**; **Delete** removes one of yours. **Copy Configuration** puts the current options on the clipboard as JSON and **Paste Configuration** applies options from the clipboard, so you can share setups with other Listen or Feishin users. **Reset to default** at the bottom restores the defaults (your saved presets stay).

**General**

| Option | Meaning |
|---|---|
| **Mode** | `[0]` discrete frequencies, `[1]` to `[8]` octave bands from 1/24th octave (240 bands) to a full octave (10 bands), or `[10]` line / area graph (default). |
| **Line Width**, **Fill Alpha** | For the graph mode. |
| **Bar Space** | Space between bars, for the bar modes. |
| **Channel Layout** | **Single**, **Dual-Combined**, **Dual-Horizontal** or **Dual-Vertical**. |
| **Max FPS** | Frame rate, 1 to 60; 0 uses 30. |
| **Opacity** | 0 to 1. |

**Colors:** **Color Mode** (**Gradient**, **Bar-Index**, **Bar-Level**), **Gradient** (built-in gradients and your own), and for dual layouts **Gradient Left**, **Gradient Right** and **Split Gradient**.

**FFT:** **FFT Size** (1024 to 32768; default 16384) and **Smoothing** (0 to 0.95).

**Frequency range and scaling:** **Minimum Frequency** (20 to 50 Hz), **Maximum Frequency** (8000 to 22050 Hz) and **Frequency Scale** (**Log**, **Linear**, **Bark**, **Mel**).

**Sensitivity:** **Weighting Filter** (None, A, B, C, D, Z), **Minimum Decibels** (-120 to -60), **Maximum Decibels** (-40 to 0), **Linear Amplitude** with **Linear Boost**, and **Volume** (input gain, 0 to 4).

**Peak behavior:** **Show Peaks** (bar modes) or **Peak Line** (graph mode), **Fade Peaks**, **Gravity**, **Peak Fade Time (ms)** and **Peak Hold Time (ms)**.

**Radial spectrum:** **Radial**, and with it **Radial Invert**, **Spin Speed** (-3 to 3 rpm) and **Radius**.

**Reflex:** **Reflex Ratio**, **Reflex Alpha**, **Reflex Brightness**, **Reflex Fit** and **Mirror** (Left, Off, Right).

**Miscellaneous:** for bar modes **LED Bars** (with **True LEDs**), **Alpha Bars**, **Lumi Bars**, **Outline Bars**, **Round Bars**, and **Low Resolution** (mode 0) or **ANSI Bands**; and for all modes **Show Scale X**, **Note Labels**, **Show Scale Y** and **Show FPS**.

**Custom gradients.** Under **Custom Gradients**, press **Add Custom Gradient**, name it, choose **Vertical** or **Horizontal**, and add colours as `#rrggbb` values with **Add Color**. **Copy** and **Paste Gradient** move a gradient through the clipboard. Your gradients then appear in the gradient choices.

## Milkdrop (Linux)

Milkdrop is the classic music-visualizer format. Listen draws `.milk` presets with [projectM](https://github.com/projectM-visualizer/projectm). It is available on Linux only. Listen does not include projectM or any presets, so you need two things:

1. **libprojectM version 4** installed on your computer. Listen loads it when it starts; it is not needed to build or run Listen otherwise. Listen looks for `libprojectM-4.so.4` in the system library paths, in `/usr/lib64`, and in `/usr/local/lib64` and `/usr/local/lib` (where a build from source installs it).
2. **Milkdrop presets** (`.milk` files) in a folder. By default that is `presets` inside the `milkdrop` folder of Listen's data folder (on Linux `~/.local/share/listen/milkdrop/presets`). Subfolders up to eight levels deep are read, so you can unpack a preset pack there as it comes. A different folder can be set in the settings.

To use it:

1. Open **Settings → Playback → Milkdrop**. If libprojectM is missing, the row says "Milkdrop needs libprojectM 4" and the button stays disabled.
2. Set **Visualizer type** to **Milkdrop**. The queue strip and the full-screen visualizer now show Milkdrop.
3. Choose your folder in **Preset folder** (**Browse…**) and press **Rescan** after adding files. The row shows how many presets were found.

If the folder has no `.milk` files, the picture area says so and names the folder.

| Setting | Default | Meaning |
|---|---|---|
| **Preset** | | Search by name and pick a preset to show now. |
| **Next preset** | | Switches now. The picture also has a next button. |
| **Cycle presets** | on | Switches to another preset on a timer. |
| **Cycle time** | 30 s (1 to 300) | How long a preset stays. |
| **Blend time** | 2.5 s (0 to 10) | How long a preset takes to fade into the next. |
| **Random next preset** | on | Pick the next preset at random; off follows the list order. |
| **Include all presets** | on | Cycle through every preset; off limits it to the selected ones. |
| **Selected presets** | none | **Add / remove current** puts the preset on screen into your selection; **Clear** empties it. |
| **Ignored presets** | none | **Ignore current** never shows the preset on screen again; **Clear** undoes that. |
| **Max FPS** | 0 (= 30) | Frames per second; values are limited to 5 to 60. |
| **Opacity** | 100 % | How strongly the picture shows. |

Milkdrop renders on a hidden OpenGL context at 960 by 540 pixels and scales the picture to the available space. It draws from the same audio capture as the spectrum.
