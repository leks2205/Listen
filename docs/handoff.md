# Handoff between devices (listen-server)

Every device that has a listen-server address and token tells the server what it plays, reads what the
others play, and can take the music over: the song, its position and the queue move to the device you
are at, and the device that played pauses. The same server carries the merged history
(`docs/history-sync.md`); handoff uses its address and token. listen-server 0.5.0 and later.

The server keeps this in memory only. Every device repeats its state while it plays, so a restart of the
server loses at most a few seconds.

## Routes

All routes need `Authorization: Bearer <token>`.

`PUT /v1/now/{device}`, body:

```json
{
  "name": "Office PC",
  "kind": "desktop",
  "playing": true,
  "position_s": 83.2,
  "song": {"server_key": "youtubemusic", "song": <Song>},
  "queue": {"index": 20, "items": [{"server_key": "...", "song": <Song>}]},
  "took_from": "other-device"
}
```

- `device` is the device's file-name-safe id (the analysis machine id).
- `kind` is `desktop` or `phone`.
- `song` is the current song with its `server_key` (the same key as in `docs/history-sync.md`), or
  `null` while nothing (or an internet radio station) plays. A DJ bridge stretch is never sent.
- `queue` is optional; leave it out when the queue did not change and the server keeps the last one.
  At most 20 songs before and 150 after the current one; `index` points at the current song (during a DJ
  bridge, which is never sent, at the song after it).
- `took_from` is sent once, with the state that follows a takeover. The server stamps it with the record's
  `seq` (`took_seq`) and keeps it until the next takeover.

Answer: `{"seq": n, "now_ms": server_clock}`.

`GET /v1/now?after=N&wait=S`: answers at once when anything changed after `seq` N, otherwise holds the
request up to S seconds (at most 30) for a change. Answer:

```json
{"seq": 42, "now_ms": 1767000000000, "devices": [
  {"device", "name", "kind", "playing", "position_s", "song", "seq", "at_ms", "played_ms",
   "took_from", "took_seq", "started_seq", "queue_len"}
]}
```

- `at_ms`: when the server got the device's last state (server clock).
- `played_ms`: the last time the device was heard playing (0 = never).
- `started_seq`: the `seq` at which the device last started playing (or took over).
- Devices silent for seven days are dropped. Queues are left out of this list.

`GET /v1/now/{device}`: the same record with its `queue` and the server's `now_ms`.

## A device's part

Sending (every 2 s, only when due):
1. Send when playing/paused, the current song or the queue changed, when the position jumped by more
   than 3 s (a seek), and at least every 20 s while playing.
2. A playing device's position now is `position_s + (now_ms - at_ms) / 1000`.

Listening: a long poll on `GET /v1/now?after=<last seq>&wait=25` while the app runs (the phone: while
it is on screen or playing). After a failure, wait 15 s.

- **Another device plays** when `playing` and `now_ms - at_ms < 75 s` (a device not heard from for that
  long counts as gone). The app shows it with *Play here*.
- **The music was taken from this device** when a record has `took_from` = this device, a `took_seq`
  newer than this device's own `started_seq`, and newer than the last takeover handled. If this device
  plays, it pauses and says where the music went.

Taking over, in this order:
1. A trigger: *Play here*; Play pressed while nothing plays here; or a new audio output here (headphones,
   Bluetooth, USB) while nothing plays here.
2. The device to take from: one that plays now (the most recent), otherwise the one that played last,
   if it played more recently than this device and stopped within 10 minutes (Play, Play here) or 3
   minutes (a new output). None: Play plays this device's own queue as usual.
3. `GET /v1/now/{device}`, map each song to a server of this device (the rules of
   `docs/history-sync.md`: same `server_key`, else the only server of that kind; local folders never
   match) and drop the songs that cannot play here. When the current song cannot play here, nothing
   is taken over.
4. Replace the queue, start the current song at the other device's position now, and send the new
   state with `took_from`.

The desktop watches mpv's `audio-device-list` for new outputs; the phone uses `AudioDeviceCallback`
(Bluetooth A2DP/LE, wired, USB headsets and hearing aids).
