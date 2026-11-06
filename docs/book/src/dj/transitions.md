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
| **Echo-out** | The last beat of a phrase of the outgoing song is repeated four times, each repeat half as loud and thinner, while the next song starts on the downbeat right after. The two songs never play their own sound at the same time, so the tempos need not match and the voices don't clash. Used when both edges are sung, or the tempos are too far apart for a blend, and the outgoing song ends a phrase loudly or with a sung line. A bridge song is tried first (see [Bridge songs](#bridge-songs)), and a song that came in with an echo-out never leaves with one: its way out is a cut, a blend or a fade. |
| **Cut** | A hard switch on a phrase downbeat (with a few milliseconds of fade so it doesn't click), each song at its own tempo. Used in the same cases as an echo-out when the bar before is quiet. |
| **Fade** | A plain crossfade, used when one song has no steady beat, or while the AI DJ talks over the join. |
| **Gapless** | No mixing at all. |

## Lyrics show where the singing is

The song analysis guesses from the sound where a singer is, and it often hears a voice in a synth lead or a chant. Synced lyrics are much more exact: each line has a start time, and the gap to the next line shows where the singing stops. So while a song plays and while it is next, Listen looks up its synced lyrics the same way as for the lyrics view (the server's lyrics first, then the [lyric providers](../playback/lyrics.md) if **Fetch lyrics** is on) and uses them for the transition:

- **Where the singing is.** Bars without a sung line count as instrumental, even if the sound analysis thought otherwise. That finds the short vocal gaps that pop and K-pop songs have, such as rap breaks, dance breaks and post-chorus hooks, so more pairs get a real blend and fewer a filter-out.
- **Where a line ends.** In the last 40 % of a song, a mix may start right after a sung line when a gap follows, not only in the outro. A mix that would stop the outgoing song in the middle of a line is avoided.
- **Where the chorus is.** The block of lines that repeats most is the chorus. A blend that ends right where the next song's singing or chorus starts is preferred, and the next song may skip part of its intro for it (at most within its first 40 % or 90 seconds).

Cutting a song short or skipping an intro costs the plan points for every second, so Listen only does it when the join is clearly better. **Transitions may skip parts of songs** (Settings → DJ transitions) limits this: **Free** (the default) allows it as described, **Short** allows at most 16 seconds on either side, and **Never** keeps every mix in the outgoing song's outro and starts the next song at its first beat. Lyrics are not used when they do not fit the song: when they run past its end, or when the sound analysis hears singing mostly where the lyrics are silent, or the lyrics fit much better a few bars earlier or later (an offset, or the lyrics of another version). Plain (unsynced) lyrics are not used. A transition is planned at once with what is known; if the lyrics arrive later but well before the mix, it is planned again.

While both songs play, Listen keeps the overall loudness steady: the sum of the two follows a straight line from the level of the outgoing song just before the mix to the level of the next song just after it. It knows how loud each part of a song is from the song analysis, so a blend over a quiet outro is lifted a little (at most 3 dB) and two loud choruses together are turned down (at most 6 dB). With **Volume levelling** on, each song's own gain is taken into account. Echo-outs and cuts have no overlap and are left as they are.

Before a transition is armed, Listen plays it through once on the song analysis (no audio is needed, and it takes well under a millisecond). It rejects the plan if the mix would fall more than 1.5 LU below the quieter song or rise more than 1.5 LU above the louder one. Because the analysis is a little rougher than the audio, a predicted drop already fails from 0.9 LU. It also rejects it if both singers would be clearly heard together for more than a second, or if the outgoing song would stop in the middle of a sung line (this last check is only made when synced lyrics show where the lines end). A rejected plan gives way to the next best option, and the plain fade comes last. If no option passes, the one with the smallest problem plays. Rejected plans are written to the log with the reason.

For a beat-matched blend the two songs must run at the same speed. Listen speeds one or both songs up or down by a few percent without changing the pitch. With **Tempo ramps** on, the speed glides smoothly during the blend and returns to the natural speed of the new song afterwards. With it off, the tempo is matched for the whole blend at once, which can be heard on large differences.

Songs whose tempo is double or half of the other (for example 70 and 140 BPM) are matched at that ratio instead of being sped up a lot.

The speeds are taken from the bars that actually overlap, not from each song's average tempo. Many songs drift a little (a slower intro, a live drummer), and a speed set from the average would let the beats slide apart by tens of milliseconds during a long blend. When the incoming song speeds up or slows down inside the blend, its speed glides along with it so the beats stay together, also when one of the songs slows down and picks up again within the blend (the speed then follows a smooth curve, adjusted bar by bar). A mix point whose beats still wouldn't line up is passed over for a better one, or the songs join with an echo-out or a cut.

> **Note:** Tempo changes use the rubberband library inside mpv. If your mpv was built without it, Listen uses a different method, which works but can make the beat wobble slightly while a song is not at its natural speed.

## Bridge songs

Some pairs a DJ picked do not join well: the tempos are too far apart to beat-match, the keys clash, or the two songs simply sound very different. A club DJ solves this with a *bridge track*: a record that sits between the two, keeps the feel of the first and leads towards the second, so each step stays small.

With **Bridge songs** on, Listen does the same with a song that has already been analysed:

1. When the next song is far from the current one (no beat-matched join, a tempo gap of more than 4 %, clashing keys, or a large difference in sound), Listen looks for a bridge among your analysed songs: first the ones the DJs were recently offered, then your listening history, then every other analysed YouTube Music song. Only a few songs have a mostly instrumental stretch at a tempo between the two, so the whole collection is searched. Songs in the queue and songs played this session are not used.
2. The bridge must be closer to both songs than they are to each other, and both joins, into the bridge and out of it, must be beat-matched blends. When the two songs could only be joined with an echo-out, a cut or a fade, a bridge that is no further from either song than they are from each other is enough: a beat-matched way through a third song is preferred to those.
3. Only a short part of the bridge plays: 24 bars (about 45 seconds at 125 BPM) that start on a phrase and have no singing where they are blended, usually an instrumental groove, a break or a long intro. Listen blends into it with a short blend of up to 8 bars, glides it back to its own tempo, lets it play alone for a few bars, and blends out of it into the next song.

The bridge shows in the queue with the tag **Bridge**. It does not count as played: it is not scrobbled and not added to the listening history. If its mix cannot be prepared, the bridge is removed and the two songs join as before; if nothing mixes out of it, the next song starts when its part ends.

**How often to bridge** sets the share of far-apart pairs that get a bridge, so it stays an occasional move rather than a pattern. It applies only to pairs that could still be blended directly; a pair that would otherwise get an echo-out, a cut or a fade tries a bridge every time.

## Order by flow

When a DJ picks several songs, **Order by flow** arranges them so that each song follows from the one before: similar tempo, neighbouring keys and a sensible energy curve, and without repeating the same artist too soon. The Auto-DJ and the AI DJ both use this. Songs without analysis are placed by the server's own similarity order.

## Order for smooth mixing

The same idea works on a whole playlist or the queue. **Play in smooth order** (right-click a playlist), **Order for smooth mixing** (right-click a queue row, or the button in a playlist's edit mode) put all analysed songs into an order where tempo, key, energy and sound change as little as possible from one song to the next, and the same artist does not play twice in a row. The first song stays first (in the queue: the playing song). Songs without analysis keep their order and go to the end. A playlist on the server only changes when you press **Save and replace** in edit mode.

The order is found in a few milliseconds even for a few hundred songs: Listen starts with the closest next song each time and then keeps reversing stretches and moving single songs while that makes the whole run smoother.

## Settings

Everything is in **Settings → Playback → DJ transitions & song analysis**.

| Setting | Default | Range | What it does |
|---|---|---|---|
| **DJ transitions** | On | on/off | Master switch for mixing. |
| **Order by flow** | On | on/off | Pick songs whose tempo, key and energy follow on from the current one. |
| **Largest tempo change** | 6 % | 2 to 8 % | The biggest speed difference Listen accepts for a beat-matched blend. Beyond this the songs join with an echo-out or a cut instead. Smaller values sound more natural and mix fewer songs. |
| **Long blends** | 16 bars | 8 or 16 | Length of a blend over instrumental intros and outros. At 120 BPM, 16 bars take 32 seconds and 8 bars take 16. |
| **Transition edges** | Smart | Always, Instrumental only, Smart | Whether vocals at the end of the outgoing song or the start of the incoming one may be blended over. **Instrumental only** needs instrumental edges, otherwise the songs get a short blend or a filter fade. **Always** never lets vocals stop a blend. **Smart** allows it for club and electronic genres (UK garage, house, techno, trance, drum and bass, dubstep, grime, amapiano and similar), where vocal samples over the beat are normal, and keeps the strict rule for everything else. The outgoing song's end is relaxed by its own genre, the incoming song's start by its own. Songs without a genre tag, including YouTube Music songs, count as non-club. Tempo, key and beat checks apply in every mode. |
| **Transitions may skip parts of songs** | Free | Never, Short, Free | How much a transition may leave out to land on a cleaner mix point: the end of the outgoing song after its last sung line or from a breakdown, and the next song's intro up to its verse or chorus, or up to its first phrase. **Free** allows it wherever the join is clearly better, **Short** at most 16 seconds of either, **Never** mixes only in the outro and starts the next song at its first beat. |
| **Tempo ramps** | On | on/off | Glide the tempo between songs instead of jumping. |
| **Bass swap** | On | on/off | Swap the basslines on a downbeat so only one plays at a time. |
| **Mix queued songs** | On | on/off | Also mix into songs you added with **Add to queue** while a DJ is on. |
| **Bridge songs** | On | on/off | Put a short part of an analysed song between two songs that are far apart and mix through it (see [Bridge songs](#bridge-songs)). |
| **How often to bridge** | 50 % | 10 to 100 % | Share of far-apart pairs that get a bridge when one fits. Pairs that cannot be beat-matched always try one. |

One more value has no switch in the window. Set it in the `[mixing]` section of the settings file (see [Files and folders](../reference/files.md)) or with the `set_settings` [MCP tool](../reference/mcp-tools.md):

| Key | Default | Range | What it does |
|---|---|---|---|
| `fallback_fade_secs` | 4.0 | 3 to 6 seconds | Length of the plain fade used when songs cannot be beat-matched and no echo-out or cut fits. |

## Mixing with the AI DJ

The [AI DJ](ai-dj.md) knows how each candidate song would be mixed in after the current one, and it can talk over a blend. When a spoken break is planned over the end of a song or the start of the next one, Listen allows a longer blend to leave room for the voice, and it keeps the voice shorter than the instrumental intro so the host never talks over a singer.

## Troubleshooting

- **Songs are not mixed.** Check that **Analyse songs** is on, that the songs have been analysed (see the status line), that Auto-DJ or the AI DJ is on, and that the songs come from Navidrome or YouTube Music. Consecutive tracks of an album are always gapless.
- **Many echo-outs, few beat-matched blends.** Raise **Largest tempo change**: songs whose tempos are too far apart are joined with an echo-out or a cut. Songs without a steady beat are faded.
- **A song was skipped for mixing right after you started it.** A mix needs about 5 seconds of lead time. Songs started late are played gapless.
- **A song ended earlier than usual, or the next one started after its intro.** The lyrics showed a cleaner place to mix (see [Lyrics show where the singing is](#lyrics-show-where-the-singing-is)). `listen --check-mix A B --lyrics-a A.lrc --lyrics-b B.lrc` shows how a pair is planned with given lyrics.
- **Why was this transition chosen?** The log lists every plan that was rejected before it (`rejected … loudness dip`, `vocals clash`, `cuts a sung line`). `listen --check-mix A B --render out.wav` shows the same check for a pair and measures the rendered preview's loudness hole and bump.
- **Different tempo after the mix.** That is the intended tempo ramp. If you dislike it, turn off **Tempo ramps** or lower **Largest tempo change**.

How the engine works internally is described in [DJ transitions internals](../development/dj-transitions.md).
