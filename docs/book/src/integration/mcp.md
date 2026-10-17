# MCP server

Listen has a built-in [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server. It lets an AI assistant that supports MCP, such as Claude Code or Claude Desktop, play music, manage the queue and playlists, change the AI DJ and adjust settings for you. The server runs inside Listen on your own computer; nothing is hosted elsewhere. It is off by default.

## Turning it on

1. Open **Settings → Window → MCP server**.
2. Switch on **Enable MCP server**. The row then shows "Running at http://127.0.0.1:4334/mcp".
3. Listen creates an access token the first time it is needed and keeps it in the system keyring.

| Setting | Meaning | Default |
|---|---|---|
| **Enable MCP server** | Starts and stops the server. | Off |
| **MCP server port** | TCP port, 1024 to 65535. Press `Enter` to apply. | 4334 |
| **Allow other computers** | Off: only programs on this computer can connect (`127.0.0.1`). On: the server listens on all network addresses. | Off |
| **Access token** | **Copy token** copies it; **New token** makes a new one and disconnects every client. | created on first use |
| **Connect Claude Code** | **Copy command** copies a ready-made `claude mcp add` command. | |

If the server cannot start, for example because the port is in use, the row shows "Not running" with the reason. Pick another port and press `Enter`.

## Connecting a client

Every request must carry the header `Authorization: Bearer <token>`. The endpoint is `http://127.0.0.1:4334/mcp` (use your port; for another computer use this computer's address instead of `127.0.0.1`).

### Claude Code

Press **Copy command** and run it in a terminal, or type it yourself:

```bash
claude mcp add --transport http listen http://127.0.0.1:4334/mcp --header "Authorization: Bearer <token>"
```

Start a new Claude Code session afterwards. The tools appear as `listen`.

### Claude Desktop and other clients

Clients that support remote (HTTP) MCP servers take the URL and the header. Press **Copy token** to get the token.

Clients that only speak stdio can use a bridge such as `mcp-remote`. For Claude Desktop, add this to its `claude_desktop_config.json`, with your own token:

```json
{
  "mcpServers": {
    "listen": {
      "command": "npx",
      "args": [
        "mcp-remote",
        "http://127.0.0.1:4334/mcp",
        "--header",
        "Authorization: Bearer <token>"
      ]
    }
  }
}
```

This needs Node.js (for `npx`). Restart the client afterwards.

### What the server speaks

Streamable HTTP with JSON answers. Only `POST /mcp` is offered: there are no sessions and no server-sent event stream, so a `GET` gets "405 Method Not Allowed". It understands the protocol revisions 2025-06-18, 2025-03-26 and 2024-11-05. Up to 16 clients can be connected at once.

## What an assistant can do

| Area | What it can do |
|---|---|
| Playback | Read what plays; play, pause, skip, seek; volume, play mode, repeat, speed; start a sleep timer. |
| Queue | Read the queue; play or add songs, albums, artists and playlists; jump, remove, move, shuffle or clear. |
| Library | List servers and switch the active one; search; open albums, artists and playlists; start a radio; read the lyrics of the playing song. |
| Playlists | Create, fill, trim, rename and delete playlists on your servers. |
| Favourites | Like or unlike, and rate, songs, albums and artists. |
| AI DJ | Read and change the persona, voice model, language, location, notes and set size; store or remove the Gemini key. |
| Settings | Read all settings and change any of them with a partial JSON object. |
| Analysis | Check the progress of the song analysis for DJ transitions. |

The complete list with every argument is in [MCP tools](../reference/mcp-tools.md).

## Example prompts

- "What is playing right now, and what is next in the queue?"
- "Search for albums by Radiohead and play the newest one."
- "Make a playlist called Sunday Morning with ten calm songs from my library."
- "Add the songs from this album to my playlist Road Trip."
- "Lower the volume to 30 percent and stop the music in 45 minutes."
- "Turn on the AI DJ with the evening persona and set the language to German."
- "Switch the gapless setting to yes."
- "Remove everything after the current song from the queue."

The assistant first searches, which returns references to songs, albums and so on, and then passes those references to the next tool. You do not need to know them.

## Security

- Only programs on this computer can connect, unless you switch on **Allow other computers**. Even then every request needs the token. Listen shows a red warning while the server is open to the network.
- Web pages in your browser cannot talk to the server: requests that come from a web page on another site (an `Origin` that is not `localhost`) are refused.
- Whatever has the token can control Listen fully: change settings, edit or delete playlists on your servers, and use your Gemini quota through the AI DJ. Treat the token like a password and do not paste it into shared places.
- No tool ever returns a password, cookie, API key or the token. `get_settings` leaves them out, and `get_ai_dj` only says whether a Gemini key is stored.
- The connection is plain HTTP. On your own computer that is fine; over a network, anyone who can see the traffic can read the token. Prefer keeping **Allow other computers** off and using an SSH tunnel or similar if you need access from elsewhere.
- Destructive tools (`clear_queue`, `delete_playlist`) run without asking Listen's confirmation dialog. Most MCP clients ask you before they run a tool; keep that on for these.
- **New token** immediately disconnects every client. Update the token in each client afterwards.
