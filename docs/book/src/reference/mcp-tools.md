# MCP tools

Every tool of Listen's [MCP server](../integration/mcp.md), with its arguments. The server offers 43 tools, which a client reads with `tools/list`. Arguments marked "required" must be given; the rest are optional.

## References

Songs, albums, artists and playlists are named by a `ref` string:

| Kind | Format |
|---|---|
| Song | `song:<server>\|<id>` |
| Album | `album:<server>\|<id>` |
| Artist | `artist:<server>\|<id>` |
| Playlist | `playlist:<server>\|<id>` |

`<server>` is the id of a music server as `list_servers` shows it, and `<id>` is the server's own id for the item. Do not make refs up: every tool that returns items (`search`, `get_album`, `get_artist`, `get_playlist`, `list_playlists`, `get_queue`, `get_now_playing`) includes a `ref`, and Listen remembers the items it has returned since it started. A ref it has not returned gives the error `Unknown ref "...". Use a ref returned by search, get_album, get_playlist or get_queue.`

Queue entries are not refs; they are named by an integer `uid` from `get_queue`. Playlist entries are named by their 0-based position from `get_playlist`.

Tools that take refs accept songs, albums, artists and playlists unless said otherwise; albums, artists and playlists are expanded to their songs.

## Results and errors

A successful call returns the result as pretty-printed JSON text. A tool that fails returns the message as text with `isError` set to true, for example `The queue is empty.` or `This server can't rename playlists.` A call to a tool that does not exist, or without a tool name, is a JSON-RPC error (-32602). A call that takes longer than 60 seconds fails with "timed out".

Song objects look like this:

| Field | Meaning |
|---|---|
| `ref`, `title`, `artist`, `album` | Identity and names |
| `album_ref`, `artist_refs` | Refs to open the album or artists |
| `duration` | Seconds |
| `year`, `favorite`, `rating` | Metadata |

Album, artist and playlist objects have `ref`, `name`, `favorite` or `owner`/`editable`, and counts (`songs`, `albums`).

## Playback

| Tool | Description | Arguments |
|---|---|---|
| `get_now_playing` | The playing song, position and duration (seconds), whether it plays, volume, muted, play mode, repeat, speed and the server it comes from. | none |
| `play` | Resume playback. Error `The queue is empty.` if nothing is queued. | none |
| `pause` | Pause playback. | none |
| `toggle_play` | Play or pause. | none |
| `next` | Skip to the next song. | none |
| `previous` | Go to the previous song (or the start of this one). | none |
| `seek` | Jump to a position in the playing song. | `seconds` (number, required): position in seconds |
| `set_volume` | Set the volume. | `volume` (integer, required): 0 to 100 |
| `set_play_mode` | What happens when the queue runs out. `ai_dj` needs a Gemini key. | `mode` (string, required): `off`, `shuffle`, `auto_dj`, `ai_dj` |
| `set_repeat` | Repeat nothing, the whole queue or the current song. | `repeat` (string, required): `off`, `all`, `one` |
| `set_speed` | Playback speed. | `speed` (number, required): 0.5 to 2.0, 1.0 is normal |
| `sleep_timer` | Stop playback later, or cancel the timer. Give `minutes` or `at`. | `minutes` (integer): stop after this many minutes (at most 1440); `at` (string): `end_of_song`, `end_of_album`, `cancel`, used instead of `minutes` |

## Queue

| Tool | Description | Arguments |
|---|---|---|
| `get_queue` | The queue. Each entry is a song object plus `uid`, `current` (true for the playing one) and `auto` (added by a play mode). | none |
| `play_songs` | Play songs, albums, artists or playlists. | `refs` (array of strings, required); `how` (string): `now` (replace the queue and play, the default), `next` (play after the current song), `last` (add to the end); `shuffle` (boolean): shuffle them first |
| `add_to_queue` | Add items to the end of the queue, or after the current song. | `refs` (array of strings, required); `next` (boolean): insert after the current song instead |
| `play_queue_entry` | Jump to a queue entry. | `uid` (integer, required) |
| `remove_from_queue` | Remove entries; the playing one stays. Returns how many were removed. | `uids` (array of integers, required) |
| `move_in_queue` | Move entries to a position. | `uids` (array of integers, required); `position` (integer, required): 0-based index they start at |
| `shuffle_queue` | Shuffle the queue; the playing song stays where it is. | none |
| `clear_queue` | Destructive: empties the whole queue and stops playback. | none |

With `play_songs`, when the refs mix kinds (for example an album and a playlist), the first kind follows `how` and the rest are added to the queue.

## Library

| Tool | Description | Arguments |
|---|---|---|
| `list_servers` | The music servers set up in Listen: `id`, `name`, `kind`, `active`, `connected`. | none |
| `switch_server` | Make another server the active one. | `server` (string, required): id from `list_servers` |
| `search` | Search songs, albums, artists and playlists on a server. | `query` (string, required); `server` (string): server id, default the active server; `limit` (integer): results per kind, default 20, 1 to 100 |
| `get_album` | An album with its songs. | `ref` (string, required): album ref |
| `get_artist` | An artist with biography, albums and top songs (up to 10). | `ref` (string, required): artist ref |
| `list_playlists` | The playlists on a server. | `server` (string): default the active server |
| `get_playlist` | A playlist with its songs; each song has a 0-based `position`. | `ref` (string, required): playlist ref |
| `start_radio` | Play a radio of similar songs, seeded with a song or artist. | `ref` (string, required): song or artist ref |
| `get_lyrics` | The lyrics of the playing song: `provider`, `synced`, and `lines` with `at` (seconds), `text` and `translation`. `{"lyrics": null}` if none were found. | none |

## Playlists

| Tool | Description | Arguments |
|---|---|---|
| `create_playlist` | Create a playlist on a server, optionally with songs. Only song refs are accepted; add albums later with `add_to_playlist`. Fails if the server cannot create playlists. | `name` (string, required); `server` (string): default the active server; `refs` (array of strings): song refs |
| `add_to_playlist` | Add songs, or the songs of albums and artists, to a playlist. All songs must come from the playlist's server. Returns `added_songs`. | `playlist` (string, required): playlist ref; `refs` (array of strings, required) |
| `remove_from_playlist` | Remove entries by 0-based position (see `get_playlist`). | `playlist` (string, required); `positions` (array of integers, required) |
| `rename_playlist` | Rename a playlist. | `playlist` (string, required); `name` (string, required) |
| `delete_playlist` | Destructive: deletes a playlist on its server. This cannot be undone. Fails for playlists that are not editable. | `playlist` (string, required) |

Servers that cannot do an operation answer with an error such as `This server can't add to playlists.`

## Favourites and ratings

| Tool | Description | Arguments |
|---|---|---|
| `set_favorite` | Like or unlike songs, albums or artists (YouTube Music: like or save to library). | `refs` (array of strings, required); `favorite` (boolean, required): true to like, false to unlike |
| `set_rating` | Rate songs, albums or artists on servers that support ratings. Playlists cannot be rated. | `refs` (array of strings, required); `stars` (integer, required): 0 (clear) to 5 |

## AI DJ

| Tool | Description | Arguments |
|---|---|---|
| `get_ai_dj` | The AI DJ's setup: `enabled`, `key_stored`, persona, voices, models, language, location, pronunciation, details, set size range, `talk_every`, `max_voiced_per_day`, `voice_gain_db`. Never the key itself. | none |
| `list_personas` | The AI DJ's personas with `id`, `name`, `description` and voices. | none |
| `set_ai_dj` | Change the AI DJ. Only the given fields change; returns the new setup. | see below |
| `set_gemini_key` | Store a Gemini API key for the AI DJ in the system keyring. An empty string removes it. The key is never returned by any tool. | `key` (string, required): the key from Google AI Studio |

`set_ai_dj` arguments (all optional):

| Argument | Type | Meaning |
|---|---|---|
| `enabled` | boolean | Switch the AI DJ on or off. Switching on needs a Gemini key (`set_gemini_key`). |
| `persona` | string | Persona id from `list_personas`. |
| `tts_model` | string | Voice model: `gemini-3.8-flash-lite-tts` or `gemini-3.8-flash-tts`. |
| `language` | string | Language code the hosts speak (`en`, `de`, `fr`, ...); empty means the app's language. |
| `location` | string | Where the show airs; empty means no home town. |
| `pronunciation` | string | How the voice says the place name. |
| `details` | string | Free text for the hosts: names, likes, topics to avoid. |
| `set_max` | integer | Songs per set, 2 to 8. |
| `talk_every` | integer | Talk after every n-th set, 1 to 5. |
| `voice_gain_db` | number | Voice loudness relative to the music, -12 to 12 dB. |
| `max_voiced_per_day` | integer | Spoken breaks per day at most. |

Remember that the AI DJ sends song titles, artists, the DJ's lines and your "Details" text to Google; see [AI DJ](../dj/ai-dj.md).

## Settings and analysis

| Tool | Description | Arguments |
|---|---|---|
| `get_settings` | All of Listen's settings as JSON, with passwords and keys left out. | none |
| `set_settings` | Change settings with a partial object that is merged key by key, for example `{"playback":{"gapless":"yes"}}`. Invalid values are refused and listed; the rest is applied. Returns `applied` and `refused`. | `settings` (object, required): the same shape `get_settings` returns |
| `get_analysis_status` | How far the song analysis for DJ transitions is: whether analysis and DJ transitions are on, songs analysed, songs waiting, the current song and any error. | none |

`set_settings` also applies changes that need work, such as audio settings and the remote and MCP servers.

See [Settings](settings.md) for what each setting does.
