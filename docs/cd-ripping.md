# CD ripping (Linux)

Insert an audio CD and Listen rips it, much like MusicBee's auto-rip: a Fast rip (one burst read with
`cd-paranoia`, checked against AccurateRip) or a Secure rip with [whipper](https://github.com/whipper-team/whipper)
(Test & Copy, AccurateRip), always FLAC level 8,
MusicBrainz tags, cover art and ReplayGain, filed as `Album Artist/Album/NN Title.flac` into your music
folder. When it is done the disc is ejected and you get a notification.

## Setup

```bash
sudo dnf install whipper flac cdrdao        # the RPM pulls these in already
listen --check-cd                           # tools, drive, read offset, music folder
```

Pick the destination in **Settings → Advanced → CD ripping**. It can be a network share (for example the
folder Navidrome scans); Listen waits for it if it isn't mounted and never deletes or overwrites anything there.

## Rip modes

**Settings → Advanced → CD ripping → Rip mode**, Fast by default; each rip's card has a Secure / Fast
switch that works until the first track is being read.

| | Fast | Secure |
|---|---|---|
| Reads | each track once, `cd-paranoia -Z` (burst), drive at full speed (`eject -x`) | `cdrdao read-toc`, then every track twice with paranoia, compared |
| 15:53 CD (LG GU70N, CD-R) | ≈ 4.2 min: 953 s of audio at 3.79x | ≈ 19 min: 202 s table scan + two 1.84x reads |
| Verification | AccurateRip v1/v2 (Listen computes the checksums and fetches `dBAR-….bin`) | the second read + AccurateRip (whipper) |
| CD-Text | READ TOC format 5 from the drive | whipper's cdrdao `.toc` |

Measured on track 1 of that disc (167 s): paranoia read 90.6 s, whipper's verify read another 90.6 s,
burst 51.6 s, burst at full speed 44.0 s; the burst output was byte-identical to the paranoia output.

What Fast gives up: no second read. A read error the drive doesn't report is only caught when AccurateRip
knows the disc (it then shows as a mismatch; the files are kept). On a disc AccurateRip doesn't know, a Fast
rip is unchecked. A track cd-paranoia can't read in burst mode is read once more with paranoia before it
counts as unreadable.

## What it does

| Step | Listen |
|---|---|
| Detect | UDisks2; also while Listen sits in the tray |
| Rip | Fast: `cd-paranoia` burst read + AccurateRip; Secure: whipper secure rip. Drive offset from the AccurateRip drive database (or **Verify offset**) |
| Identify | MusicBrainz disc ID → fuzzy TOC match whose every track fits within 3 s → gnudb (only with an e-mail address set; colliding entries only when iTunes/Deezer confirm exactly one) → CD-Text → "Unknown Album (…)"; everything after MusicBrainz is listed under "Rips to fix" |
| Several matching releases | a picker; the best match is taken after 30 s |
| Cover | Apple Music (iTunes Search API, full size), Cover Art Archive (original image; release → release group) and Deezer (1000 px), asked at the same time; gnudb and CD-Text discs get Apple Music and Deezer by artist and album. Each image is measured from its bytes; under 500 px, not square within 1:1.15, under 10 KB or not JPEG/PNG is rejected. Highest resolution wins; within 10 % the order is Apple → Cover Art Archive → Deezer. Over 3000 px or 8 MB is scaled to 3000 px (JPEG). Embedded + `cover.jpg`/`cover.png` |
| Files | `Album Artist/Album/NN Title.flac`, multi-disc `D-NN Title.flac`, ReplayGain track + album |
| Publish | copied into a hidden `.listen-incoming` folder, then renamed into place in one step |
| Navidrome | asked to scan when you are signed in as an admin; otherwise its watcher picks the album up |

## gnudb

A disc MusicBrainz doesn't know is looked up on [gnudb](https://gnudb.org) (the FreeDB successor), but only
after you enter an address in **Settings → Advanced → CD ripping → gnudb e-mail address**. gnudb asks every
program for a contact address and Listen sends it with every lookup (`hello=<user> <host>
ListenMusicPlayer <version>`); Listen ships no address of its own, so with the field empty gnudb is never asked.

Listen sends `cddb query` with the disc's CDDB ID, track offsets and length (lead-out / 75), then `cddb read`
with the category and ID the query answered (which can differ from the computed one). gnudb refuses some
lengths outright (`500 Command syntax error.` for 949 s, while 948 and 950 are answered); a refused query is
sent once more with the length the CDDB disc ID counts (lead-out − first track, / 75; 947 for that disc). A
`202` (no match) is not asked again. An exact match (`200`/`210`) names the album, album artist, track titles
and year; an inexact one (`211`) is only used when it has as many tracks as the disc. gnudb entries are
submitted by other people and nobody checks them; they can be wrong.

When gnudb lists several entries, Listen reads up to five of them and drops every one whose stored frame
offsets or track count differ from the disc's. One left: it names the disc, as a single match does. Several
left (different records typed in for the same TOC, which the TOC cannot tell apart): an entry is only used
when an outside source confirms exactly one of them. It is confirmed when iTunes has its album (artist and
title, without bracketed parts) with the disc's track count and every track within 3 s of the disc's; when
iTunes has no album of that name, Deezer's album search with the same track count counts instead. Zero or
several confirmed: none is used, and the disc goes on to CD-Text / "Unknown Album". Listen never picks
among such entries by list order, genre, year or title; the log has one line per entry with its verdict.
An entry that couldn't be read still counts as one of the colliding entries.
The album gets no MusicBrainz IDs, stays `LISTEN_NEEDS_FIX=1` and is listed under "Rips to fix" with the link
for adding the disc to MusicBrainz.

## The rip window

**Details** on the rip card, or next to **Rip CD** in Settings → Advanced → CD ripping, opens the rip
window (a page, `--route rips`; it never opens by itself). Per rip (several drives: one below the other):

- header: cover, album artist – album, metadata source (MusicBrainz / gnudb / CD-Text / unknown), mode, drive and
  read offset, destination;
- the card's actions (mode switch, Cancel, version picker, Publish anyway / Discard, Choose folder… / Retry);
- one bar for the disc, elapsed time, time left and read speed ("3.8× real time" = audio seconds read ÷
  seconds spent reading, measured from the first track on; the time left is the unread audio at that speed);
- a table with every TOC track from the start: number, title ("Track NN" until identified), length, state
  (Waiting → Reading → Verifying (Secure) → Encoding → Tagged ✓, or Failed / Skipped), the track's progress
  and its AccurateRip result once the disc is read; the current row is highlighted;
- the log: the ripper's steps and warnings with the time into the rip, scrollable.

The track states are derived in the app from the `Progress` / `Activity` updates the pipeline already sent;
the pipeline adds the track titles (`Update::Tracks`), the per-track read result (`Update::Read`: whipper-style
track log entries and the tracks without a file) and the ripper's own warnings (`Update::Log`).

## Checking a rip

1. `listen --check-cd` with a disc inserted shows the TOC, disc ID, the MusicBrainz match (or, when there
   is none, gnudb's answer) and where the album would be filed.
2. After the rip, open the album folder: `NN Title.flac`, `cover.jpg`, `<Album>.log`, and no
   `.listen-incoming` left behind. `metaflac --list` shows the tags, MusicBrainz IDs, cover and ReplayGain.
3. Pulling the disc mid-rip (or pressing Cancel) leaves nothing in the music folder.
4. Logs: `~/.local/share/listen/logs/listen.log` and `rip-*.log`.
