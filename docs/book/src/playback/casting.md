# Casting to speakers and TVs

Listen can hand playback to a network speaker or TV that supports DLNA / UPnP (a "media renderer"). This page explains how devices are found, how to cast, how volume works and what does not work while you cast.

## Turning it on

Casting is on by default. The switch is **Cast to DLNA devices** in **Settings → Playback → Playback**. When it is on, Listen searches your local network a few seconds after it starts, and again whenever you open the cast popover or press **Refresh** in it. When it is off, Listen sends no discovery messages and shows no cast button.

Discovery uses the standard SSDP multicast on the local network, so the device and the computer must be on the same network (not a guest network that isolates devices), and a firewall must let the device's answers and its requests for the music through.

## Casting a song

1. Look for the cast button (a screen with a wave) among the right-hand buttons of the player bar. It only appears once at least one device has been found.
2. Click it. The popover **Cast to a device** lists **This computer** and the devices found, with their model names.
3. Click a device. It shows "Connecting…" and then a check mark. A message says "Casting to" the device's name.

If your device is not listed, make sure it is switched on and on the same network, then press **Refresh**.

If a song was playing, it continues on the device from the same position. If nothing played, the device gets the playing song when you press Play. The queue stays in Listen; the device plays the songs one after another as the queue advances. Pause, stop, seek, next and previous in the player bar control the device.

To go back, open the popover and choose **This computer**. A song that was playing continues here at the same position.

## Volume

While you cast, the volume slider and the mute button in the player bar control the **device's** volume. The speaker icon turns accent-coloured, the slider's tooltip names the device and the mute button's tooltip reads "Mute the device". When you stop casting, the local volume comes back as it was. A device that offers no volume control keeps its own level.

## How the music reaches the device

The device fetches each song itself. For a plain `http://` address, Listen simply passes the address on. For local files, and for streams that use HTTPS or need request headers, Listen starts a small web server on your computer, bound to the network address that reaches the device. The device then loads the audio from your computer. Each song gets an unguessable address, and Listen never passes your account's cookies on. The server stops when you stop casting.

The device shows the title, artist and album of the song.

## What does not work while casting

Everything that depends on this computer's audio engine is off:

- **DJ transitions** and the **AI DJ's voice** are off. The Auto-DJ still adds songs.
- The equalizer, compressor, ReplayGain, playback speed, pitch and fades, because mpv, which applies them, rests while you cast. See [Sound](sound.md).
- The visualizer has no audio to show.
- The song/video switch for [music videos](videos.md) is off and is switched off if it was on.
- Songs are loaded one by one, so there can be a short gap between songs instead of gapless playback.
- Whether a particular file plays depends on the device: renderers differ in the formats they support. If a device cannot play a song, Listen shows "could not play this song" with the reason.
- Starting an [internet radio](radio.md) station ends casting; the station plays on this computer.

If the device stops answering (five failed checks in a row), Listen says it lost the connection and continues playing on this computer.

The cast popover also notes these limits while a device is in use.
