# CD ripping

Listen can rip audio CDs on Linux. Put a disc in the drive and Listen copies it, checks the copy against AccurateRip, tags it from MusicBrainz, adds cover art and files the album into your music folder, much like the auto-rip of MusicBee. This page covers the setup, every option under **Settings → Advanced → CD ripping**, and how to check a rip.

> **Note:** CD ripping is available on Linux only. It uses the separately installed programs `cd-paranoia` (Fast mode) and [whipper](https://github.com/whipper-team/whipper) (Secure mode). On Windows the **CD ripping** section does not do anything.

## What you get

- A Fast rip (one read at the drive's full speed, about 4 minutes) or a Secure rip with whipper (Test & Copy, about 20 minutes), both checked against AccurateRip. See [Rip modes](#rip-modes).
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
| **Rip mode** | Fast | **Fast** or **Secure**, see [Rip modes](#rip-modes). Each rip can still be switched on its card until the first track is read. |
| **Music folder** | your system Music folder, such as `~/Music` | Where albums are filed. Type a path and press **Save**, or use **Choose…** to pick a folder. It can be a network share, for example the folder your Navidrome server scans (`/mnt/nas/Music`). Listen never deletes or overwrites anything in it. |
| **Eject when done** | on | Ejects the disc after the rip. |
| **Notification sound** | on | Plays a sound with the "Album ripped" notification. The sound needs `pw-play` (PipeWire); without it you get the notification only. |
| **ReplayGain tags** | on | Measures track and album loudness while tagging, so players can even out volume. |
| **Scan Navidrome afterwards** | on | Asks the active Navidrome server to scan for the new album. This needs an admin login. Without one, the server's own file watcher finds the album within a minute or two. |
| **Version picker** | 30 s (5 to 120 s, in steps of 5) | How long Listen waits when several MusicBrainz releases match the disc (for example regional editions) before it takes the best match. |
| **gnudb e-mail address** | empty | Your e-mail address, for naming discs MusicBrainz does not know from [gnudb](https://gnudb.org) (the successor of FreeDB). gnudb asks every program for a contact address, and Listen sends this one with every gnudb lookup. Listen has no address of its own: while the field is empty, gnudb is never asked and album names come only from MusicBrainz and the disc's CD-Text. See [How a disc is identified](#how-a-disc-is-identified). |
| **Drive read offset** | automatic | The read offset of your drive in samples. See below. |
| **Rip now** | | The **Rip CD** button rips the disc in the drive, also one you ripped before. It appears when a drive is connected. |

### Rip modes

| | Fast | Secure |
|---|---|---|
| How | `cd-paranoia` in burst mode reads every track once, the drive set to its full speed | whipper reads the table of contents (`cdrdao`), then every track twice and compares the two reads |
| Time for a 15:53 CD | about 4 minutes (one read at 3.8x) | about 19 minutes |
| Checked by | AccurateRip | the second read, and AccurateRip |
| CD-Text | read from the drive | from whipper's table scan |

The times were measured on an LG GU70N with a 6-track CD-R of 15:53: whipper's table scan alone took 202 s before any audio was read, each of its two paranoia reads of the first track (2:47) took 91 s, while one burst read of the same track at full speed took 44 s. The burst copy was byte-identical to the paranoia copy.

What Fast gives up: there is no second read to compare with. A read error that the drive does not report goes unnoticed unless AccurateRip knows the disc; then the track shows up as a mismatch ("AccurateRip: 11/12 tracks verified, 1 mismatch") and the files are kept. For a disc AccurateRip does not know ("not in the AccurateRip database"), a Fast rip is unchecked; rip it again in Secure mode if it matters. A track that cd-paranoia cannot read at all in burst mode is read once more with paranoia's checks before it counts as unreadable.

While the rip card shows **Ripping**, its **Secure** / **Fast** switch changes the mode of this rip. It works until the first track is being read; switching during whipper's table scan starts the rip over in Fast mode.

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
| Rip | Fast: sets the drive to full speed and reads each track once with `cd-paranoia`, then checks it with AccurateRip. Secure: runs a whipper secure rip. Both use the drive's read offset. |
| Identify | Looks up the MusicBrainz disc ID; if that fails, asks gnudb (when an e-mail address is set); if that fails, uses CD-Text; if that fails, names the album "Unknown Album (…)". See [How a disc is identified](#how-a-disc-is-identified). |
| Several matches | Shows a picker with the matching releases. You can change the choice until tagging starts; after the timeout the best match is taken. |
| Cover | Listen asks three sources at the same time and keeps the best answer: Apple Music (the iTunes Search API, at the artwork's full size), the Cover Art Archive (the uploaded original of the release's front cover, else of the release group's; its 1200 px version when the original is missing or over 25 MB) and Deezer (1000 px). A disc named from gnudb or CD-Text has no MusicBrainz release, so it gets Apple Music and Deezer, searched by artist and album. Every image is measured from the downloaded file. Images smaller than 500×500, further from square than 1:1.15 (back covers, booklet scans), under 10 KB or not JPEG or PNG are left out. The highest resolution wins; when two are within 10 % of each other, Apple Music is preferred, then the Cover Art Archive, then Deezer, so a 1400 px Apple cover beats a 1500 px scan while a 3000 px original wins outright. A winner larger than 3000×3000 or 8 MB is scaled down to fit 3000×3000 and saved as JPEG. The cover is embedded and saved as `cover.jpg` (or `cover.png`); the log lists every candidate with its size. |
| Publish | Copies the album into `.listen-incoming` and renames it into place. |
| Navidrome | Asks for a scan if the option is on and you are signed in as an admin. |

The panel shows progress ("Ripping track 3 of 12", "Tagging", "Copying to the music folder…") and ends with the folder and the AccurateRip result. While a rip runs you can press **Cancel**. Cancelling, or pulling the disc, leaves nothing in the music folder.

If some tracks could not be read perfectly, the panel asks "Publish the rest anyway?" with **Publish anyway** and **Discard**.

### The rip window

The card above the player bar is the short view. For the whole picture, press **Details** on the card, or **Details** next to **Rip CD** in **Settings → Advanced → CD ripping**. The rip window is a page of its own; it never opens by itself. When two drives rip at once, it shows both rips one below the other.

For each rip it shows:

- **The header**: the cover, album artist and album, where the metadata came from (MusicBrainz, CD-Text or unknown), the rip mode, the drive with its read offset, and the folder rips go to.
- **The card's buttons**: the **Secure** / **Fast** switch, **Cancel**, the version picker, **Publish anyway** / **Discard**, and **Choose folder…** / **Retry**.
- **The disc's progress**: one bar for the whole disc, the time since the rip started, the estimated time left and the read speed as a multiple of real time (for example "3.8× real time" means 3.8 seconds of audio per second). The time left is worked out from the speed measured so far in this rip, so it gets more accurate as the rip goes on.
- **The tracks**: one row per track of the disc, there from the start, with its number, title (once the album is identified, "Track 03" until then), length, state, its own progress and its AccurateRip result once the disc is read (✓ with the database's confidence, **Mismatch** or **Not in database**). A track goes through **Waiting**, **Reading**, **Verifying** (Secure only), **Encoding** and **Tagged ✓**; a track that could not be read shows **Failed**, and the tracks a cancelled or failed rip never got to show **Skipped**. The row being read or tagged is highlighted.
- **The log**: the steps and warnings the ripper reported, newest at the bottom, each with the time into the rip. If a rip seems stuck, the last lines show what it is doing.

When the rip is done the window keeps it as a record: the folder it was filed into, the AccurateRip line and every warning.

## Rips to fix

### How a disc is identified

A CD carries no title, only the lengths of its tracks. Listen turns those into the MusicBrainz *disc ID* and asks MusicBrainz for it. When the disc ID is in the database, that release is the disc.

When it is not, MusicBrainz falls back to searching by the table of contents, and that search compares the disc as a whole: it answers with releases that are about as long in total, which is frequently a completely different album. Listen therefore checks every candidate track by track and only uses one whose every track is within three seconds of the disc's. A release that merely has the same total length is discarded with a note in the rip card, and the disc counts as unknown. One that fits is used, also with a note, because it is a guess — usually a different pressing of the same album.

If MusicBrainz does not know the disc and you entered a **gnudb e-mail address**, Listen asks [gnudb](https://gnudb.org) next. gnudb is the old FreeDB database: anyone can submit an entry, nobody checks them, and entries can be misspelled, in the wrong order or simply wrong. Listen sends gnudb the disc's table of contents and reads the entry gnudb answers with. An exact match names the album, the album artist, the track titles and the year. When gnudb only has an inexact match (a disc that is merely close), Listen uses it only if it has exactly as many tracks as the disc; otherwise it ignores it. gnudb refuses the disc length Listen sends for some discs ("500 Command syntax error."); Listen then asks once more with the length counted from the first track, which gnudb answers.

Sometimes gnudb has several entries with exactly the same table of contents: different records that someone typed in for the same disc layout, so the disc itself cannot tell them apart. Listen first drops every entry whose stored track positions or track count differ from the disc's. If more than one is left, Listen checks each one against iTunes (and Deezer when iTunes doesn't have the album): an entry counts as confirmed when the store has that album with the same number of tracks and every track within 3 seconds of the disc's. Listen only uses an entry when exactly one is confirmed. If none or several are confirmed, Listen uses none of them, and the disc is named from CD-Text or filed as "Unknown Album (…)". It never picks one by its place in gnudb's list. A gnudb entry carries no MusicBrainz IDs. The rip card says "MusicBrainz doesn't know this disc — tagged from gnudb", and the album folder gets the gnudb name, for example `HEART OF WOMAN/ETERNAL FADE/01 Footsteps.flac`.

If gnudb does not know the disc either, or no address is set, Listen uses CD-Text if the disc has it, and otherwise files it as "Unknown Album (…)". All of these discs, including the ones gnudb named, are still not on MusicBrainz. They are listed under **Rips to fix (not on MusicBrainz)** in the CD ripping section, newest first, with the album folder and a link: "Add the disc: …". Open the link, add the release to MusicBrainz, then press **Rip CD** again to rip the disc with proper tags. The list shows the last 10.

## Checking a rip

1. Run `listen --check-cd` with a disc inserted. It shows the track list, the disc ID, the MusicBrainz matches (best first, marked with an arrow), gnudb's answer when MusicBrainz has nothing ("gnudb: no address set" or "gnudb: unknown disc" otherwise), the cover it found and the exact folder and file name the album would get. It does not rip anything.
2. After a rip, open the album folder. You should see `NN Title.flac` files, `cover.jpg` and `<Album>.log` (the rip log; in Fast mode Listen writes it in whipper's layout, with the AccurateRip result per track). No `.listen-incoming` folder should remain. `metaflac --list` on a file shows the tags, MusicBrainz IDs, cover and ReplayGain values.
3. Logs: Listen's own `listen.log` and one `rip-<date>-<disc id>.log` per rip are in the logs folder, `~/.local/share/listen/logs/`. See [Backup, logs and diagnostics](backup.md).

For problems with the tools, drive or share, see [Troubleshooting](troubleshooting.md).
