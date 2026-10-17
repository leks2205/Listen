# CD ripping

Listen can rip audio CDs on Linux. Put a disc in the drive and Listen makes a secure copy, tags it from MusicBrainz, adds cover art and files the album into your music folder, much like the auto-rip of MusicBee. This page covers the setup, every option under **Settings → Advanced → CD ripping**, and how to check a rip.

> **Note:** CD ripping is available on Linux only. It uses the separately installed program [whipper](https://github.com/whipper-team/whipper). On Windows the **CD ripping** section does not do anything.

## What you get

- A secure rip with whipper (Test & Copy, checked against AccurateRip).
- Always FLAC at compression level 8.
- Tags and track names from MusicBrainz, cover art embedded in every file and saved as `cover.jpg`.
- ReplayGain tags for each track and for the album.
- Files named `Album Artist/Album/NN Title.flac`. Multi-disc releases use `D-NN Title.flac`.
- A notification when the album is done, and the disc is ejected.

## Setup

1. Install the tools. On Fedora the Listen RPM already pulls them in. Elsewhere install whipper, flac and cdrdao with your package manager, for example:

   ```bash
   sudo dnf install whipper flac cdrdao
   ```

2. Run the check without a disc, or better with one in the drive:

   ```bash
   listen --check-cd
   ```

   It lists the tools, the drive and its read offset, the disc's table of contents and MusicBrainz match, the music folder and your Navidrome server, and ends with "ready to rip" or the number of problems it found. See [Checking a rip](#checking-a-rip).

3. Open **Settings → Advanced → CD ripping** and set the **Music folder**.

If you plug in a USB drive later, Listen picks it up. The section shows the drive, its device (for example `/dev/sr0`) and the read offset it will use. Without a drive it says "No CD drive connected".

## The settings

| Setting | Default | What it does |
|---|---|---|
| **Auto-rip inserted CDs** | on | Starts ripping as soon as an audio CD goes into the drive, also while Listen sits in the tray. Turn it off to rip only with **Rip CD**. |
| **Music folder** | your system Music folder, such as `~/Music` | Where albums are filed. Type a path and press **Save**, or use **Choose…** to pick a folder. It can be a network share, for example the folder your Navidrome server scans (`/mnt/nas/Music`). Listen never deletes or overwrites anything in it. |
| **Eject when done** | on | Ejects the disc after the rip. |
| **Notification sound** | on | Plays a sound with the "Album ripped" notification. The sound needs `pw-play` (PipeWire); without it you get the notification only. |
| **ReplayGain tags** | on | Measures track and album loudness while tagging, so players can even out volume. |
| **Scan Navidrome afterwards** | on | Asks the active Navidrome server to scan for the new album. This needs an admin login. Without one, the server's own file watcher finds the album within a minute or two. |
| **Version picker** | 30 s (5 to 120 s, in steps of 5) | How long Listen waits when several MusicBrainz releases match the disc (for example regional editions) before it takes the best match. |
| **Drive read offset** | automatic | The read offset of your drive in samples. See below. |
| **Rip now** | | The **Rip CD** button rips the disc in the drive, also one you ripped before. It appears when a drive is connected. |

### Destination that is not available

If the music folder is a network share that is not mounted, the rip waits and tries again every 30 seconds. The panel says "Waiting for the music folder". You can pick another folder with **Choose folder…** or press **Retry**. Listen first copies the finished album into a hidden `.listen-incoming` folder inside the destination and then renames it into place in one step, so a half-copied album never shows up in your library.

### Read offset

Every CD drive reads audio shifted by a small number of samples. AccurateRip needs the right value to confirm that your copy matches other people's. Listen takes the offset from, in this order:

1. The number you saved in **Drive read offset**.
2. whipper's own configuration (`~/.config/whipper/whipper.conf`, written by `whipper offset find`).
3. The AccurateRip drive list built into Listen, matched by the drive's model name.

The line under the setting shows which one is used, for example `read offset +48 (AccurateRip)`.

If the drive is not in the list, the line says the offset is unknown and a rip stops with a message asking you to set it. Either type the offset into the field and press **Save**, or press **Verify offset**. **Verify offset** runs whipper's offset search, which takes a few minutes and needs a well-known, commercially pressed CD in the drive. When it succeeds the result is stored in whipper's configuration. The offset is a whole number between -3000 and 3000, with or without a plus sign. Clear the field and press **Save** to go back to automatic.

## What happens when you insert a disc

| Step | What Listen does |
|---|---|
| Detect | Notices the disc through UDisks2. This also works while Listen is only in the tray. |
| Rip | Runs a whipper secure rip with the drive's read offset. |
| Identify | Looks up the MusicBrainz disc ID; if that fails, uses CD-Text; if that fails, names the album "Unknown Album (…)". |
| Several matches | Shows a picker with the matching releases. You can change the choice until tagging starts; after the timeout the best match is taken. |
| Cover | Cover Art Archive first (the release, then the release group), then iTunes. The image is scaled to 1200 px, embedded and saved as `cover.jpg`. |
| Publish | Copies the album into `.listen-incoming` and renames it into place. |
| Navidrome | Asks for a scan if the option is on and you are signed in as an admin. |

The panel shows progress ("Ripping track 3 of 12", "Tagging", "Copying to the music folder…") and ends with the folder and the AccurateRip result. While a rip runs you can press **Cancel**. Cancelling, or pulling the disc, leaves nothing in the music folder.

If some tracks could not be read perfectly, the panel asks "Publish the rest anyway?" with **Publish anyway** and **Discard**.

## Rips to fix

If MusicBrainz does not know the disc, Listen still rips and files it as "Unknown Album (…)", using CD-Text if the disc has it. Such discs are listed under **Rips to fix (not on MusicBrainz)** in the CD ripping section, newest first, with the album folder and a link: "Add the disc: …". Open the link, add the release to MusicBrainz, then press **Rip CD** again to rip the disc with proper tags. The list shows the last 10.

## Checking a rip

1. Run `listen --check-cd` with a disc inserted. It shows the track list, the disc ID, the MusicBrainz matches (best first, marked with an arrow), the cover it found and the exact folder and file name the album would get. It does not rip anything.
2. After a rip, open the album folder. You should see `NN Title.flac` files, `cover.jpg` and `<Album>.log` (whipper's log). No `.listen-incoming` folder should remain. `metaflac --list` on a file shows the tags, MusicBrainz IDs, cover and ReplayGain values.
3. Logs: Listen's own `listen.log` and one `rip-<date>-<disc id>.log` per rip are in the logs folder, `~/.local/share/listen/logs/`. See [Backup, logs and diagnostics](backup.md).

For problems with the tools, drive or share, see [Troubleshooting](troubleshooting.md).
