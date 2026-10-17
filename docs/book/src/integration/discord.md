# Discord status

Listen can show what you are listening to as a Discord Rich Presence status. It is off by default and needs the Discord desktop app to be running on the same computer. The settings are under **Settings → Window**.

## Turning it on

1. Start the Discord desktop app.
2. Open **Settings → Window** and switch on **Discord Rich Presence**.
3. Play a song. Your Discord profile shows it within a few seconds.

If Discord is not running, Listen cannot connect and tries again with the next update (Listen re-sends the status every 15 seconds). Nothing is shown in private mode.

## Options

| Setting | What it does | Default |
|---|---|---|
| **Discord Rich Presence** | Master switch. | Off |
| **Discord application ID** | The Discord application whose name and logo the status uses. Press `Enter` to apply. | Listen's own application |
| **Show presence when paused** | Keep the status while paused, with a paused icon and no timer. Off clears the status on pause. | On |
| **Show playing icon** | Small "playing" icon next to the cover. | On |
| **Show status as listening** | "Listening to ..." instead of "Playing ...". | Off |
| **Discord display type** | What the short status line under your name shows: **Listen** (the application name), **Song name** or **Artist name**. | Listen |
| **Discord link type** | Make the song, artist and cover clickable: **None**, **Last.fm**, **MusicBrainz**, or **MusicBrainz (Last.fm fallback)**. | None |
| **Show server cover image** | Send the cover from your music server to Discord. | Off |
| **Last.fm API key** | Look up the album cover on Last.fm when there is no other cover. Press `Enter` to save. | empty (off) |

The status shows the song as the first line, the artists as the second, the album as the cover's tooltip and, while playing, a progress timer. Lines are cut at 127 characters.

### Link types

- **Last.fm** links the artist to its Last.fm page and the song and cover to the album and track pages.
- **MusicBrainz** links use the MusicBrainz ids your server stores for the song (track or recording id, and release id for the cover). Servers that do not provide ids give no link.
- **MusicBrainz (Last.fm fallback)** uses MusicBrainz where ids exist and Last.fm otherwise.

### Covers

Discord can only load images from public web addresses, so Listen picks a cover in this order:

1. A YouTube Music song uses its public https thumbnail.
2. With **Show server cover image** on, a song from a server that is reachable from the internet over https (a public host name, not a local name or private address) uses the server's cover. The link includes your server's access token, so Discord and the people who see your status can in principle read it. Only switch this on for a server you are comfortable exposing that way.
3. With a **Last.fm API key**, the album cover is looked up on Last.fm (a free key from Last.fm's API pages). Results are cached for the session.
4. Otherwise Listen's logo is shown.

Covers from local folders are never sent.

## Listen's own Discord application

By default the status uses an application registered for Listen, so Discord shows the name "Listen" and Listen's logo. If you prefer your own name and artwork, create an application in the Discord Developer Portal, upload an asset named `icon` (plus `playing` and `paused` if you want the small icons), and paste its application ID into **Discord application ID**.

## Private mode

With **Private mode** on (**Settings → Window → Private mode** or the application menu), Listen clears the status and does not show anything on Discord until you turn it off again or restart Listen. Private mode also stops scrobbling and history; see [Tray, window and notifications](window.md#scrobbling).
