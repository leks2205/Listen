# Play modes and Auto-DJ

Listen has one button that decides what happens when the queue runs low: stop, shuffle, add similar songs (Auto-DJ) or let the AI DJ take over. Repeat is a separate button. This page explains both and the Auto-DJ settings.

## The play mode button

The play-mode button sits in the player bar next to Stop. The same button appears in the header of the queue and on the Now Playing page, labelled with the current mode. Each click moves to the next mode:

**Off, Shuffle, Auto-DJ, AI DJ, and back to Off.**

The tooltip shows the current mode and the one a click leads to ("Play mode: Shuffle → next: Auto-DJ"). The "Toggle shuffle" hotkey, the media-key integration and the remote-control page use the same cycle. The MCP server's `set_play_mode` tool can pick a mode directly. A highlighted button means any mode other than Off is on.

The modes exclude each other. If several settings were on together (for example from an older configuration), the order of priority is AI DJ, then Shuffle, then Auto-DJ.

| Mode | What happens |
|---|---|
| **Off** | The queue plays in order and stops at its end. |
| **Shuffle** | The play order of the queue is shuffled. The playing song keeps playing, and songs in the Up Next block still come first, in the order you queued them. Switching away from Shuffle restores the original order. |
| **Auto-DJ** | When few songs are left, Listen appends similar songs to the queue. |
| **AI DJ** | Gemini picks sets of songs and talks between them. Needs your own Gemini API key. |

A new installation starts in **Auto-DJ**. The first-start wizard can turn it off.

> **Note:** The AI DJ is skipped in the cycle until a Gemini API key is saved. If you choose it anyway (for example through MCP), Listen shows a hint to add a key in **Settings → Playback → AI DJ**. See [AI DJ](../dj/ai-dj.md) and [Creating a Gemini API key](../dj/gemini-key.md).

Shuffle changes the order of the queue itself, so you see the new order in the queue panel. The sleep timer's **End of album** option is not available while Shuffle is on.

With the AI DJ on, clicking a single song in a list plays that song alone and makes it the new mood for the DJ's next set, instead of queueing the whole list.

## Repeat

The repeat button cycles **Off, One, All**:

- **Off:** the queue stops after its last song.
- **One:** the playing song repeats. Pressing Next still moves on, but stays on the last song instead of wrapping around.
- **All:** after the last song the queue starts again from the top.

The button is highlighted whenever repeat is on, and shows a "1" symbol for **One**. The hotkey is "Toggle repeat".

## Auto-DJ

The Auto-DJ keeps the music going without you filling the queue. It watches how many songs are left after the playing one. When at most one song is left, it asks the server of the playing song for songs similar to it, adds a few random songs as filler, and appends the best candidates to the end of the queue. Songs it added are tagged **Auto-DJ** in the queue. See [Player bar and queue](queue.md#queued-songs-and-dj-songs).

Details:

- Songs that already played in this session, and songs already in the queue, are not added again.
- Where a song exists in several versions (for example a studio version and a live version), one version is picked, preferring the radio edit.
- If songs were [analysed](../dj/analysis.md), the picks are ordered so that tempo, key and energy follow on from the playing song. The Auto-DJ then looks at more candidates before choosing.
- With [DJ transitions](../dj/transitions.md) on, songs the Auto-DJ picked are mixed into each other.
- What "similar" means depends on the server: Navidrome and Jellyfin use their similar-songs lists, YouTube Music uses its "watch next" list.

### Settings

Open **Settings → Playback → Auto-DJ**:

| Setting | Default | Meaning |
|---|---|---|
| **Enabled** | on | Switches the Auto-DJ on or off. Same as choosing the mode. |
| **Songs to add** | 5 (1 to 20) | How many songs are added at a time. |

Three more options exist that have no control in the settings window. They live in the configuration file under `[settings.auto_dj]` (see [Files and folders](../reference/files.md)), can be changed through the MCP server's `set_settings` tool, and are filled in when you import your settings from Feishin:

| Key | Default | Meaning |
|---|---|---|
| `timing` | `1` | The Auto-DJ refills when this many songs or fewer are left after the playing one. |
| `allow_duplicates` | `false` | Allows songs that are already in the queue to be added again. |
| `strategy` | `"similar"` | `"similar"` asks for similar songs first; `"random"` only adds random songs. |

Edit the file only while Listen is closed.
