# Merged listening history (listen-server)

Every device keeps its own `history.jsonl` (`listen_core::history`). listen-server (0.4.0 and later)
holds one append-only log, `state/history.jsonl`, that carries plays between them. All routes need the
same `Authorization: Bearer <LISTEN_SERVER_TOKEN>` as the rest of the API.

## Identity of a play
`(ts, song_id)`. Server ids (`entry.server_id`) are random per device, so they are no part of it.

## `server_key`
The name every device gives a server. It travels with each play so the receiver can swap
`entry.server_id` for its own id.

| Source | `server_key` |
|---|---|
| YouTube Music | `youtubemusic` |
| Navidrome / Jellyfin | `<source>:<url without scheme, user info and trailing slash, lower case>`, e.g. `subsonic:music.example.org` |
| Local folder, or a server this device no longer has | `<source>:<entry.server_id>` |

`<source>` is the `SourceKind` as JSON: `subsonic`, `jellyfin`, `youtubemusic`, `local`.

Receiving a play, in this order:
1. A server of this device with the same `server_key`: use its id.
2. Exactly one server of that source kind: use its id (the same server under another address; song ids
   are the same).
3. Otherwise keep the entry's `server_id`. The play counts in statistics but cannot be played here.

Local-folder plays never match a server (rule 3).

## Routes
`POST /v1/history/{device}`, body `{"entries": [{"server_key": "...", "entry": <Entry>}]}`.
`entry` is a `listen_core::history::Entry` as the file stores it, and is passed on unchanged. A repeated
play or one already removed is ignored, so a push can safely repeat. Answer `{"changed": n, "seq": last}`.
The request limit is 2 MB: send at most 500 plays at a time.

`POST /v1/history/{device}/remove`, body `{"keys": [{"ts": 1, "song_id": "..."}]}`. Answer as above.
A removal is permanent: that play can never be added again.

`GET /v1/history?after=N&limit=M` (default 1000, at most 2000). Answer `{"items": [...], "seq": last}`,
oldest first. Items:
- `{"seq", "device", "op": "add", "ts", "song_id", "server_key", "entry"}`
- `{"seq", "device", "op": "remove", "ts", "song_id"}`

Your own plays come back too; drop them by `(ts, song_id)`.

`device` is a file-name-safe name of the device (the phone uses its analysis `machine` id).

## A device's sync
State kept on the device: `pushedTs` (0 at first), `cursor` (0 at first), the queue of removals.
1. Push the plays with `ts >= pushedTs`, then set `pushedTs` to the newest one sent. `>=` is on purpose:
   two plays can share a second. Plays that came from other devices may go out again; the server ignores them.
2. Send the queued removals, and clear them only after the 200.
3. Pull until `items` is empty, saving the highest `seq` as `cursor` after each page. Add the plays not
   yet in the file, drop the removed ones, sort by `ts` (file order is what Recently played shows),
   write atomically, then apply the retention limit.

Skip the sync while the history is switched off. A private session records nothing, so nothing goes out.

## What counts as removal
- Removing one play: queued, removed on every device.
- Clear history: this device only, by design. It sends no removals, and the cursor stays, so the plays
  do not come back. A desktop's history must not be wiped by a tap on the phone.
- The retention limit: this device only, for the same reason.

## Release radar (listen-server 0.7.0)
`GET /v1/releases` (same token) answers `{updated, artists, releases: [{id, title, kind, year?, image?,
artist, artist_id, found, top}]}`: new albums and singles (YouTube Music `MPREb_…` ids) from the artists
the listener subscribed to plus the 50 most played in the merged history of the last year, newest find
first, kept 60 days. `top` marks the 10 most played artists (the phone notifies for those).
`POST /v1/releases/scan` runs the job now (202). The job reads the pages once a day with the shared
YouTube Music session, at most 200 requests a day; state in `releases.json` in the state dir. Rules:
`listen_core::releases`.
