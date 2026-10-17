# CD ripping (Linux)

Insert an audio CD and Listen rips it, much like MusicBee's auto-rip: a secure rip with
[whipper](https://github.com/whipper-team/whipper) (Test & Copy, AccurateRip), always FLAC level 8,
MusicBrainz tags, cover art and ReplayGain, filed as `Album Artist/Album/NN Title.flac` into your music
folder. When it is done the disc is ejected and you get a notification.

## Setup

```bash
sudo dnf install whipper flac cdrdao        # the RPM pulls these in already
listen --check-cd                           # tools, drive, read offset, music folder
```

Pick the destination in **Settings → Advanced → CD ripping**. It can be a network share (for example the
folder Navidrome scans); Listen waits for it if it isn't mounted and never deletes or overwrites anything there.

## What it does

| Step | Listen |
|---|---|
| Detect | UDisks2; also while Listen sits in the tray |
| Rip | whipper secure rip, drive offset from the AccurateRip drive database (or **Verify offset**) |
| Identify | MusicBrainz disc ID → CD-Text → "Unknown Album (…)" (listed under "Rips to fix") |
| Several matching releases | a picker; the best match is taken after 30 s |
| Cover | Cover Art Archive (release → release group) → iTunes; 1200 px, embedded + `cover.jpg` |
| Files | `Album Artist/Album/NN Title.flac`, multi-disc `D-NN Title.flac`, ReplayGain track + album |
| Publish | copied into a hidden `.listen-incoming` folder, then renamed into place in one step |
| Navidrome | asked to scan when you are signed in as an admin; otherwise its watcher picks the album up |

## Checking a rip

1. `listen --check-cd` with a disc inserted shows the TOC, disc ID, the MusicBrainz match and where the
   album would be filed.
2. After the rip, open the album folder: `NN Title.flac`, `cover.jpg`, `<Album>.log`, and no
   `.listen-incoming` left behind. `metaflac --list` shows the tags, MusicBrainz IDs, cover and ReplayGain.
3. Pulling the disc mid-rip (or pressing Cancel) leaves nothing in the music folder.
4. Logs: `~/.local/share/listen/logs/listen.log` and `rip-*.log`.
