# Internet radio

Listen plays the internet radio stations stored on your Navidrome or other OpenSubsonic server. You can create, edit and delete stations from the app.

## Availability

The **Radio** page is offered when the server you are browsing supports internet radio stations. Navidrome and other OpenSubsonic servers do; Jellyfin, YouTube Music and local folders do not. The **Radio** item is in the sidebar by default; if it is missing, check the sidebar items in **Settings → General → Sidebar**.

The stations are kept on the server, not in Listen, so they follow you to every app and computer that uses the same server account.

## Playing a station

1. Open **Radio** in the sidebar. Each station is a card with its name, stream address, home page address and, if the server has one, a picture.
2. Click a card to play it. The card is highlighted, and the current stream title ("Now playing: Artist - Title") appears on it when the station sends one.
3. Click the playing station again, or press **Stop** in the player bar, to stop it.

While a station plays:

- The player bar shows the station and the stream title. Only **Stop** and **Play / Pause** are available. There is no next, previous or seek, because a stream has no tracks.
- Your queue is left untouched and waits. Press Play on a song or start something else to go back to music.
- Starting a station ends [casting](casting.md) to a device; the station plays on this computer.
- If the stream fails, Listen shows a message with the station name and the reason.

## Finding and sorting stations

The page header shows the number of stations. Use the search field to filter by name, the sort list to order by **Name** or **ID** and the arrow to reverse the order. The refresh button reloads the stations from the server.

## Creating, editing and deleting stations

To add a station:

1. Press **Create station**.
2. Fill in **Name** and **Stream URL**. The address must start with `http://` or `https://`.
3. Optionally add a **Home page URL**.
4. Press **Create**.

The pencil button on a card opens the same dialog to change the station (**Save**). The bin button deletes it after a confirmation; this cannot be undone. If you delete the station that is playing, it stops.

> **Tip:** Use the direct stream address of the station (often ending in `.mp3`, `.aac` or `.m3u8`), not the address of its web page. The web page address belongs in **Home page URL**.
