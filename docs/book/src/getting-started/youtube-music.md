# YouTube Music

This page explains how to connect YouTube Music to Listen, what works, and what to do when sign-in fails.

> **Note:** YouTube Music support is unofficial. Listen is not affiliated with or endorsed by Google or YouTube. It uses YouTube Music's web interface and the separately installed program yt-dlp, may stop working whenever YouTube changes something, and may not be allowed by YouTube's Terms of Service. You use it with your own account and at your own risk; YouTube may restrict accounts it considers to be accessed by unofficial clients. A YouTube Music Premium subscription is recommended.

## How sign-in works

YouTube Music does not let third-party apps sign in with Google directly. Instead, Listen uses the YouTube session of a browser you already use on the same computer. You never type your Google password into Listen.

Listen stores only which browser profile and which channel to use. The cookies themselves are read again from the browser at every start and, while Listen runs, whenever YouTube has rotated its session cookie — an old one is answered as signed out, which used to empty the library and the playlists until the next restart. They stay on your computer and are sent only to YouTube. This means Listen stays signed in for as long as the browser does.

Supported browsers: Firefox, Chrome, Chromium, Brave, Edge, Vivaldi and Ecosia, plus the Flatpak versions of Firefox, Chrome, Chromium, Brave and Ecosia.

## Sign in with a browser

1. Sign in to [music.youtube.com](https://music.youtube.com) in one of the browsers above.
2. In Listen, open **Add server** and choose the **YouTube Music** tab. Listen looks for a signed-in browser and shows a button such as **Sign in with Firefox**.
3. Press it. If your Google account has more than one YouTube channel (a personal one and one or more brand channels), Listen asks **Which &lt;browser&gt; channel has your music?** and lists the channels. Pick the one that holds your music library.
4. Listen connects and opens your library.

If the library looks empty, you probably picked the wrong channel. Open **Edit** on the server (see below) and choose another one.

### Use another browser

If several browsers are signed in, possibly with different Google accounts, press **Use another browser**. Each entry shows its state, for example "signed in to YouTube", or the reason it cannot be used. If a browser has several profiles, the profile folder name is shown in brackets. If no browser is found, the page shows why and a **Look again** button.

### The browser is open

Chromium-based browsers keep their cookie file locked while they run. If Listen reports that the browser keeps its cookies locked, close the browser completely (including background processes and tray icons) and try again.

### Windows: Chrome, Edge and Brave

Recent versions of Chrome, Edge and Brave on Windows encrypt their cookies in a way that only the browser itself can read ("app-bound" encryption). Listen then shows a message to that effect. You have two options:

- Sign in with **Firefox**, which stores its cookies unencrypted. This is the simplest way.
- Use a cookies file (next section).

## Cookies file

1. In a browser where you are signed in to music.youtube.com, export the site's cookies with an extension such as **Get cookies.txt LOCALLY**. JSON and cookies.txt formats work; a Cookie-Editor JSON export and a raw `Cookie` header work too.
2. In Listen, open **Add server**, choose **YouTube Music** and press **Other ways to connect (cookies file, guest)**.
3. Press **Browse…** and pick the file, drop the file onto the window, or paste the cookies into the field below.
4. Press **Connect with cookies / as guest**.

Listen keeps only the cookies a browser would send to music.youtube.com and needs the `SAPISID` cookie among them. A cookies file stops working when you sign out of YouTube in the browser it came from.

## Guest mode

In the same **Other ways to connect** section, leave the file and the paste field empty and press **Connect with cookies / as guest**. As a guest you can search, open albums and artists, start radio and play songs. You have no personal library, likes, playlists or history.

## What works

When you are signed in:

- Your library: liked songs, saved albums, artists, and playlists, including creating, editing and reordering them. An **Uploads** list appears if you have uploaded music.
- A home feed of quick picks, mixes and albums.
- Search, album and artist pages, radio and Auto-DJ based on YouTube's "play next" lists, and lyrics.
- Music videos for YouTube Music songs, with the song/video switch in the player bar. See [Music videos](../playback/videos.md).
- Playing a song reports it to YouTube Music, so your history and recommendations stay current there.
- Mixing YouTube Music songs with DJ transitions, once you switch on **Analyse YouTube Music songs** and choose an analysis folder. See [Song analysis](../dj/analysis.md).

## Your own copy of a song

When a YouTube Music song is also on one of your own servers (Navidrome, Jellyfin or a local folder), Listen plays that copy, which usually sounds better. It looks the song up by title and artist when the song enters the queue, ignores things like "(Official Video)", "(Remastered 2011)" or "- Topic", compares the length, and never swaps a live, remix, instrumental or karaoke version for a different one. If nothing is found, or the server does not answer in time, the YouTube Music version plays.

The song stays a YouTube Music song: likes, playlists, radio and the music video work as before, and the play is reported to YouTube Music and also to the server that played it. A small server icon next to the title shows the swap; hover it to see the server, and press it to play the YouTube Music version instead (and again to go back). The video switch keeps working: the video runs silently alongside the server's audio.

Turn it off with **Settings > Playback > Prefer my own servers for YouTube Music songs**. See [Music videos](../playback/videos.md) for how the picture is lined up.

Audio is streamed, not saved. Listen does not download YouTube Music songs for offline use, and the offline download feature works only for your own servers.

## Requirements

yt-dlp finds the audio stream of each song. It is a separate program, and it needs a JavaScript runtime (Deno or Node.js). On Windows the [first-start wizard](first-start.md) installs both; on Linux install them with your package manager (see [Installation](installation.md)).

## Managing the connection

Open **Settings → General → Servers** and press **Edit** next to the YouTube Music entry. The dialog offers:

- **Change channel…** to list your channels and pick another one.
- **Use &lt;browser&gt; instead** to switch to a different signed-in browser.
- **Reconnect (read the cookies again)** after you signed in again in the browser.

## When something stops working

YouTube changes things often.

- **Songs do not play, or play only some of the time.** Update yt-dlp first. On Fedora: `sudo dnf upgrade yt-dlp`. On Windows: **Settings → Advanced → Updates → Helper programs → Update**.
- **The library is empty or errors appear.** Listen reads the browser cookies again by itself when YouTube reports it as signed out, so this normally clears up without a restart. If the library still stays empty, press **Reconnect** in the edit dialog, then check the channel. If it still fails, a new Listen version may be needed; see [Updates](updates.md).
- **Sign-in fails after you changed your Google password or signed out in the browser.** Sign in to music.youtube.com in the browser again, then reconnect.
- **Windows security warnings.** Some antivirus programs find reading a browser's YouTube sign-in unusual. Listen reads only the cookies of youtube.com and music.youtube.com.

You can also run `listen --check-youtube-music` in a terminal to see what Listen can see (channels, counts and whether playlists and albums open) without starting the interface. It never prints cookies. See [Troubleshooting](../more/troubleshooting.md).
