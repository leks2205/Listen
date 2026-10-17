# Music videos

For YouTube Music songs, Listen can show the music video in the full-screen player while the audio keeps playing as usual. This page explains the song/video switch and its limits.

> **Note:** YouTube Music support is unofficial. Listen is not affiliated with, endorsed by or sponsored by Google or YouTube, and the feature may stop working whenever YouTube changes something. You use it with your own account and at your own risk. Listen does not download YouTube Music songs or videos for offline use. See [YouTube Music](../getting-started/youtube-music.md).

## Switching between song and video

The switch appears in the player bar only while a YouTube Music song is playing. It is a small video-camera button among the right-hand buttons:

- **Show video** switches the video on.
- **Show song** (the same button, now highlighted) switches back to the cover.

The switch is off by default. Turning it on opens the [full-screen player](fullscreen.md) and shows the video in place of the cover. The switch stays on for the following songs until you turn it off. In a narrow window, the phone-style full-screen player has the same switch in its bottom row.

## How it behaves

- The audio plays through the normal player. The video is a separate, silent picture, and the audio is the clock: the picture follows it, and if it drifts too far, it catches up by skipping ahead or restarting.
- Seeking, pausing, resuming and skipping to the next song move the video along.
- The video of the next song is prepared in advance when possible, so it starts quickly after a song change.
- The picture is rendered at 640 by 360 pixels and 30 frames per second, with black bars for other aspect ratios. This keeps the load low, and the picture is not saved anywhere.
- For a song that has a separate music-video version, Listen shows the video; otherwise it shows the song's own video.
- If no video can be played, Listen shows a message ("No video: …") and the cover stays.

## Requirements and limits

- Listen starts a second copy of **mpv** for the picture, and needs **yt-dlp** (a separately installed program, see [Installation](../getting-started/installation.md)) to find the stream, as it does for the audio. If YouTube Music playback fails, update yt-dlp first.
- The switch is not offered for songs from other servers.
- It is turned off automatically when you start [casting](casting.md), and not offered while you cast.
- The video is only drawn in the full-screen player, not in the small cover of the player bar.
- A video uses noticeably more processor power and network traffic than audio alone.
