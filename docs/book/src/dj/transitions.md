# DJ transitions

With DJ transitions on, Listen mixes songs into each other like a DJ instead of simply playing one after the other. It matches the beat, lines the songs up bar by bar, and moves the bass and the tempo so the change is hard to notice. This page explains what happens and what each setting does.

Transitions use the measurements described in [Song analysis](analysis.md). Without analysed songs nothing is mixed.

## When songs are mixed

A transition is made only when all of this is true:

- **DJ transitions** is on.
- Both songs come from Navidrome (OpenSubsonic) or YouTube Music (with analysis switched on for it). Jellyfin, local files and radio are never mixed.
- A DJ chose the songs: the [Auto-DJ](../playback/play-modes.md) or the [AI DJ](ai-dj.md) added the next song, or you added it with **Add to queue** while a DJ is on and **Mix queued songs** is on.
- The two songs are not consecutive tracks of the same album (same disc, track numbers one apart). Albums stay gapless.
- Both songs have been analysed.

Songs you start yourself by playing an album or playlist always play gapless, with no mixing. If a mix cannot be prepared in time, or you skip, seek or pause during the lead-up, Listen simply falls back to gapless playback.

## What a transition sounds like

Listen looks at both songs and picks the best way to join them. It prefers to start the blend on a phrase boundary, where the outgoing song is instrumental (or at least has no singer at the same time as the incoming song).

| Kind | What happens |
|---|---|
| **Long blend** | A beat-matched crossfade over 8 or 16 bars (see **Long blends**), used where both songs have instrumental parts. With **Bass swap** on, the bass of the outgoing song is cut halfway through, exactly on a downbeat, while the incoming bass comes in, so the two basslines never play together. |
| **Short blend** | A beat-matched crossfade of a few bars, placed so the vocals do not overlap. |
| **Filter-out** | The outgoing song is filtered away (its low end disappears first) while the next one starts on a downbeat. |
| **Fade** | A plain crossfade, used when the songs have no steady beat or their tempos are too far apart. |
| **Gapless** | No mixing at all. |

For a beat-matched blend the two songs must run at the same speed. Listen speeds one or both songs up or down by a few percent without changing the pitch. With **Tempo ramps** on, the speed glides smoothly during the blend and returns to the natural speed of the new song afterwards. With it off, the tempo is matched for the whole blend at once, which can be heard on large differences.

Songs whose tempo is double or half of the other (for example 70 and 140 BPM) are matched at that ratio instead of being sped up a lot.

> **Note:** Tempo changes use the rubberband library inside mpv. If your mpv was built without it, Listen uses a different method, which works but can make the beat wobble slightly while a song is not at its natural speed.

## Order by flow

When a DJ picks several songs, **Order by flow** arranges them so that each song follows from the one before: similar tempo, neighbouring keys and a sensible energy curve, and without repeating the same artist too soon. The Auto-DJ and the AI DJ both use this. Songs without analysis are placed by the server's own similarity order.

## Settings

Everything is in **Settings → Playback → DJ transitions & song analysis**.

| Setting | Default | Range | What it does |
|---|---|---|---|
| **DJ transitions** | On | on/off | Master switch for mixing. |
| **Order by flow** | On | on/off | Pick songs whose tempo, key and energy follow on from the current one. |
| **Largest tempo change** | 6 % | 2 to 8 % | The biggest speed difference Listen accepts for a beat-matched blend. Beyond this the songs fade instead. Smaller values sound more natural and mix fewer songs. |
| **Long blends** | 16 bars | 8 or 16 | Length of a blend over instrumental intros and outros. At 120 BPM, 16 bars take 32 seconds and 8 bars take 16. |
| **Tempo ramps** | On | on/off | Glide the tempo between songs instead of jumping. |
| **Bass swap** | On | on/off | Swap the basslines on a downbeat so only one plays at a time. |
| **Mix queued songs** | On | on/off | Also mix into songs you added with **Add to queue** while a DJ is on. |

One more value has no switch in the window. Set it in the `[mixing]` section of the settings file (see [Files and folders](../reference/files.md)) or with the `set_settings` [MCP tool](../reference/mcp-tools.md):

| Key | Default | Range | What it does |
|---|---|---|---|
| `fallback_fade_secs` | 4.0 | 3 to 6 seconds | Length of the plain fade used when songs cannot be beat-matched. |

## Mixing with the AI DJ

The [AI DJ](ai-dj.md) knows how each candidate song would be mixed in after the current one, and it can talk over a blend. When a spoken break is planned over the end of a song or the start of the next one, Listen allows a longer blend to leave room for the voice, and it keeps the voice shorter than the instrumental intro so the host never talks over a singer.

## Troubleshooting

- **Songs are not mixed.** Check that **Analyse songs** is on, that the songs have been analysed (see the status line), that Auto-DJ or the AI DJ is on, and that the songs come from Navidrome or YouTube Music. Consecutive tracks of an album are always gapless.
- **Many fades, few beat-matched blends.** Raise **Largest tempo change**, or accept that songs without a steady beat or with very different tempos are faded.
- **A song was skipped for mixing right after you started it.** A mix needs about 5 seconds of lead time. Songs started late are played gapless.
- **Different tempo after the mix.** That is the intended tempo ramp. If you dislike it, turn off **Tempo ramps** or lower **Largest tempo change**.

How the engine works internally is described in [DJ transitions internals](../development/dj-transitions.md).
