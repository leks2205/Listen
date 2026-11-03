# Continue on another device

With your own listen-server set up (Settings, Advanced, Listening history: the listen-server address
and token), Listen on your computers and on the Android app knows what the others play.

- While another device plays, a strip above the player bar shows it: *Playing on Phone: song · artist*.
  **Play here** moves the music to this computer.
- **Play** on a device where nothing plays continues what another device plays, or what it played
  until a few minutes ago (10 minutes), at the same spot, with its queue.
- **Connecting headphones** (or any new audio output) to a device where nothing plays does the same when
  another device plays or stopped in the last 3 minutes. Take your Bluetooth headphones from the
  computer to the phone and the song goes along.

The device that played pauses and says where the music went. Songs that can't play on the new device
(a local folder only that device has) are left out of the queue; if the current song is one of them,
nothing moves and Play plays this device's own queue.

Turn it off with **Continue on other devices** in Settings, Advanced, Listening history. The wire
format is in `docs/handoff.md` in the source repository.
