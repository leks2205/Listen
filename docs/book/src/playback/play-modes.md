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
| **Shuffle** | The play order of the queue is shuffled. The playing song keeps playing, and songs in the Up Next block still come first, in the order you queued them. Switching away from Shuffle restores the order from before shuffling, including songs you moved; songs added meanwhile follow after it. A DJ bridge always stays right before the song it leads into. |
| **Auto-DJ** | When few songs are left, Listen appends similar songs to the queue. |
| **AI DJ** | Gemini picks sets of songs and talks between them. Needs your own Gemini API key. |

A new installation starts in **Auto-DJ**. The first-start wizard can turn it off.

> **Note:** The AI DJ is skipped in the cycle until a Gemini API key is saved. If you choose it anyway (for example through MCP), Listen shows a hint to add a key in **Settings → Playback → AI DJ**. See [AI DJ](../dj/ai-dj.md) and [Creating a Gemini API key](../dj/gemini-key.md).

Shuffle changes the order of the queue itself, so you see the new order in the queue panel. The sleep timer's **End of album** option is not available while Shuffle is on.

With the AI DJ on, clicking a single song in a list plays that song alone and makes it the new mood for the DJ's next set, instead of queueing the whole list. A song you add with **Play next** or **Add to queue** becomes the DJ's new starting point once it plays.

**The DJs stay in the scene of what is playing.** Every song the Auto-DJ or the AI DJ adds (similar songs, favourites from your history, random filler) must belong to the scene of the playing song: the same language, the same genre and an era within a decade (the 1990s fit the 1980s, the 2020s don't). The playing song leads: after you pick a 1980s ballad, the next songs are from the 1980s or 1990s even if the songs before it were recent K-pop. When too few candidates are left, songs of that scene from your history and your analysed songs fill up (ones you have played first), never songs of another scene. The AI DJ is also told the scene and each candidate's scene. Without anything known about the playing song's scene, nothing is filtered; when nothing of the scene is left at all, songs whose scene is unknown are kept so the music doesn't stop (never ones of a known other scene). The log line: `AI DJ pool: 23 kept, 17 rejected (other scene 9, other era 6, unknown 2, no language 0) [session scene ko · pop · 1980s …]`. `listen --check-ai-dj-pool <analysis copy> <embeddings dir> <history.jsonl> [--at TITLE]` replays the pool at a moment of your history.

## Repeat

The repeat button cycles **Off, One, All**:

- **Off:** the queue stops after its last song.
- **One:** the playing song repeats. Pressing Next still moves on, but stays on the last song instead of wrapping around.
- **All:** after the last song the queue starts again from the top.

The button is highlighted whenever repeat is on, and shows a "1" symbol for **One**. The hotkey is "Toggle repeat".

## Auto-DJ

The Auto-DJ keeps the music going without you filling the queue. It watches how many songs are left after the playing one. When at most one song is left, it asks the server of the playing song for songs similar to it, adds a few random songs as filler, and appends the best candidates to the end of the queue. Songs it added are tagged **Auto** in the queue. See [Player bar and queue](queue.md#queued-songs-and-dj-songs).

Details:

- Songs that already played in this session, and songs already in the queue, are not added again.
- Where a song exists in several versions (for example a studio version and a live version), one version is picked, preferring the radio edit.
- If songs were [analysed](../dj/analysis.md), the picks are ordered so that tempo, key and energy follow on from the playing song. The Auto-DJ then looks at more candidates before choosing.
- With [DJ transitions](../dj/transitions.md) on, songs the Auto-DJ picked are mixed into each other.
- **Forgotten favourites:** every eighth song the Auto-DJ adds is one of your favourites that you have not played for at least 30 days (at least five plays in your [history](../using/history.md)). Of those, the one whose tempo, key and energy fit the playing song best is taken. Its reason in the queue reads "A favourite, not played for 5 months". Without such songs in your history the slot gets an ordinary pick.
- **Worn-out songs rest:** a song you skipped three times in the last seven days (or twice, if you also played it a lot that week) is left out of the Auto-DJ, the AI DJ, radio, DJ bridges and the mixes until those skips are more than a week old. You can still play it yourself at any time.
- **Learn from my skips:** how much a tempo gap, a key clash, an energy jump, a change of sound, a hard cut or a repeated artist count when the DJ orders its picks is learnt from your own listening. Every time a song you were playing hands over to one the Auto-DJ, AI DJ, radio or Rediscover picked, Listen notes whether you skipped the new song within 30 seconds. From these handovers (on every device, the history is synced) it fits how strongly each of those terms goes with an early skip, and weighs them that way. Until 300 handovers are recorded the learnt weights are mixed with the built-in ones in proportion (100 handovers: one third learnt), and below 300 a term can only count more or less, never the opposite way; below 30 handovers the built-in weights are used unchanged. A term your history says nothing about (for example no same-artist handovers yet) keeps its built-in weight. The fit is redone every ten plays or skips and kept in `flow-weights.json` next to the history. With **Merge with other devices** on, the fit also goes through your listen-server: the newest fit of any device is used everywhere, so the computer and the phone weigh the same way (a device that has not learnt anything yet never replaces a learnt fit with the built-in weights; needs listen-server 0.10.0 or newer). Turning the setting off brings back the built-in weights exactly. `listen --flow-weights` prints the number of handovers, the cross-validated AUC of the learnt weights next to that of the built-in ones, and each weight.
- What "similar" means depends on the server: Navidrome and Jellyfin use their similar-songs lists, YouTube Music uses its "watch next" list.

### Settings

Open **Settings → Playback → Auto-DJ**:

| Setting | Default | Meaning |
|---|---|---|
| **Enabled** | on | Switches the Auto-DJ on or off. Same as choosing the mode. |
| **Songs to add** | 5 (1 to 20) | How many songs are added at a time. |
| **Forgotten favourites** | on | One song in eight is a favourite you haven't played for a while (see above). |
| **Learn from my skips** | on | The flow scorer's weights learnt from the songs you skip early after a handover (see above). |

Three more options exist that have no control in the settings window. They live in the configuration file under `[settings.auto_dj]` (see [Files and folders](../reference/files.md)), can be changed through the MCP server's `set_settings` tool, and are filled in when you import your settings from Feishin:

| Key | Default | Meaning |
|---|---|---|
| `timing` | `1` | The Auto-DJ refills when this many songs or fewer are left after the playing one. |
| `allow_duplicates` | `false` | Allows songs that are already in the queue to be added again. |
| `strategy` | `"similar"` | `"similar"` asks for similar songs first; `"random"` only adds random songs. |

Edit the file only while Listen is closed.

## Focus

**Focus** is for working or reading while music plays: songs with lyrics distract most there. The **Focus** button sits next to the play-mode button at the top of the queue (and on the Now Playing page). It shows in Auto-DJ and AI DJ mode, and always while Focus is on. A green button means Focus is on; Listen remembers it.

With Focus on:

- The Auto-DJ and the AI DJ prefer songs with little lead vocal and calm energy. Listen knows how much of a song is sung from the [analysis](../dj/analysis.md) (the vocal level of every bar). Besides the similar songs, the Auto-DJ also looks at the least sung analysed songs in your [history](../using/history.md), so each fill finds quiet songs even when the similar ones are all sung. Songs without an analysis come last.
- A sung song the Auto-DJ picked is swapped for its instrumental version when its source has one (for example "Love (Inst.)" on YouTube Music, or an instrumental track on a Navidrome album). Listen searches for "title artist inst." for up to three songs per fill; when that finds no instrumental it searches once more for plain "title artist" (some Navidrome searches miss "(Instrumental)" tracks with the extra word). The swapped song keeps its place in the queue, and its reason reads "Good for focus".
- The AI DJ does not talk: no voiced breaks, not even planned ones. It still chooses and mixes the songs.
- No forgotten-favourite slot (a forgotten favourite is rarely a quiet one).
- Turning Focus on removes upcoming DJ songs that are sung, except the next one (it may already be mixing in). Songs you queued yourself stay.

Songs you start yourself always play as they are. Turn Focus off and the next fill is an ordinary one again. The MCP server's `set_focus` tool switches it, and `get_now_playing` reports it as `focus`.

`listen --focus` checks Focus on your own data: it splits your history's analysed songs into Auto-DJ-sized candidate pools and prints, as JSON, the mean sung share of eight ordinary five-song fills and of eight Focus fills, and their ratio (`--history FILE`, `--store DIR`, `--fills N`, `--per-fill N`, `--pool N`, `--seed N`). Instrumental swaps come on top of that number.

## Heart-rate tempo

The heart button next to the play-mode button plays songs whose tempo fits your heart rate, from a Bluetooth heart-rate sensor. In Off or Shuffle it switches to Auto-DJ. See [Heart-rate tempo](heart-rate.md).
