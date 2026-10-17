# Remote control

Listen can serve a small web page that works as a remote control for your phone or another computer on your network. It is off by default. The settings are under **Settings → Window → Remote control**.

## Turning it on

1. Open **Settings → Window → Remote control**.
2. Switch on **Enable remote control server**. The row then shows "Running: open http://...:4333 on the phone". If it cannot start (for example the port is taken), it shows "Not running" and the reason.
3. On the phone, open that address in a browser, replacing `localhost` with this computer's address on your network, for example `http://192.168.1.20:4333`.
4. Optionally use the browser's "Add to Home Screen" to install the page like an app.

## Settings

| Setting | Meaning | Default |
|---|---|---|
| **Enable remote control server** | Starts and stops the server. | Off |
| **Remote server port** | TCP port, 1 to 65535. Press `Enter` to apply. | 4333 |
| **Remote server address** | `0.0.0.0` lets other devices in your network connect; `127.0.0.1` only this computer. Press `Enter` to apply. | `0.0.0.0` |
| **Remote server username** | User name for sign-in. | empty |
| **Remote server password** | Password for sign-in. Stored in the system keyring, not in the settings file. Press `Enter` to save; `Enter` on an empty field clears it. | empty |

Leave both username and password empty for no sign-in. If you set either, the browser asks for a login (HTTP basic authentication) before it shows the page.

## What the page can do

The page has six tabs:

| Tab | Content |
|---|---|
| **Home** | Cover, title and artist, seek bar, shuffle, previous, play/pause, next, repeat, volume slider, favourite heart and star rating. While internet radio plays it shows the station name. |
| **Queue** | The queue with the playing entry highlighted. Tap an entry to jump to it, the cross removes it, and **Clear queue** empties it. |
| **Albums** | Searchable list of albums; tap to play. |
| **Playlists** | Your playlists; tap to play. |
| **Radio** | Internet radio stations; tap to play. |
| **Tracks** | Searchable list of songs; tap to play. |

The star rating only appears for songs from a server that supports ratings. The page updates live over a WebSocket and reconnects on its own if the connection drops. Its message format is compatible with the Feishin remote.

## Security

- The page and the connection use plain HTTP and WebSocket, without encryption. A basic-auth password is therefore visible to anyone who can watch your network traffic. Use the remote only on a network you trust, or keep the address at `127.0.0.1` and reach it through your own secure tunnel.
- With an empty username and password and an address other than `127.0.0.1`, Listen shows a red warning: "Anyone in your network can control Listen. Set a username and password."
- The page only receives what it needs: the song's details and the cover, which Listen serves itself at `/cover`. Your music server's address, login and tokens never leave Listen.
- Do not forward the port to the internet.
- Some systems ask you to allow Listen through the firewall the first time the server listens on a network address.

To use Listen from an AI assistant rather than a phone, see [MCP server](mcp.md).
