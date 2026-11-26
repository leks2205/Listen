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
| Identify | MusicBrainz disc ID → fuzzy TOC match whose every track fits within 3 s → gnudb (only with an e-mail address set) → CD-Text → "Unknown Album (…)"; everything after MusicBrainz is listed under "Rips to fix" |
| Several matching releases | a picker; the best match is taken after 30 s |
| Cover | Cover Art Archive (release → release group) → iTunes (by artist and album for gnudb and CD-Text discs); 1200 px, embedded + `cover.jpg` |
| Files | `Album Artist/Album/NN Title.flac`, multi-disc `D-NN Title.flac`, ReplayGain track + album |
| Publish | copied into a hidden `.listen-incoming` folder, then renamed into place in one step |
| Navidrome | asked to scan when you are signed in as an admin; otherwise its watcher picks the album up |

## gnudb

A disc MusicBrainz doesn't know is looked up on [gnudb](https://gnudb.org) (the FreeDB successor), but only
after you enter an address in **Settings → Advanced → CD ripping → gnudb e-mail address**. gnudb asks every
program for a contact address and Listen sends it with every lookup (`hello=<user> <host>
ListenMusicPlayer <version>`); Listen ships no address of its own, so with the field empty gnudb is never asked.

Listen sends `cddb query` with the disc's CDDB ID, track offsets and length, then `cddb read` with the
category and ID the query answered (which can differ from the computed one). An exact match (`200`/`210`)
names the album, album artist, track titles and year; an inexact one (`211`) is only used when it has as many
tracks as the disc. gnudb entries are submitted by other people and nobody checks them; they can be wrong.
The album gets no MusicBrainz IDs, stays `LISTEN_NEEDS_FIX=1` and is listed under "Rips to fix" with the link
for adding the disc to MusicBrainz.

## Checking a rip

1. `listen --check-cd` with a disc inserted shows the TOC, disc ID, the MusicBrainz match (or, when there
   is none, gnudb's answer) and where the album would be filed.
2. After the rip, open the album folder: `NN Title.flac`, `cover.jpg`, `<Album>.log`, and no
   `.listen-incoming` left behind. `metaflac --list` shows the tags, MusicBrainz IDs, cover and ReplayGain.
3. Pulling the disc mid-rip (or pressing Cancel) leaves nothing in the music folder.
4. Logs: `~/.local/share/listen/logs/listen.log` and `rip-*.log`.
