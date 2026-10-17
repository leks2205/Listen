# Introduction

Listen is a native desktop music player for Linux and Windows. It plays music from your own server, from YouTube Music, from folders on your computer and from internet radio, all in one queue. This page explains what it is, what it can do and how this guide is organised.

## What Listen is

Listen is written in Rust. It has no web view and no Electron runtime: it is one small program that starts quickly and keeps memory use low. The interface is drawn with [iced](https://iced.rs) and audio is played by [mpv](https://mpv.io).

Its layout follows [Feishin](https://github.com/jeffvli/feishin) closely, so if you have used Feishin you will find the home feed, the album, artist and genre pages, the command palette, the full-screen player and the settings where you expect them. Listen is a separate program written from scratch, and it can import your Feishin settings (see [Importing from Feishin](more/feishin-import.md)).

## Where the music comes from

| Source | Notes |
|---|---|
| Navidrome and other OpenSubsonic servers | Server address, user name and password. Gonic and Airsonic-style servers work too. |
| Jellyfin | Login or Quick Connect. Servers on your network are found automatically. Music libraries only. |
| YouTube Music | Unofficial. Uses your own account through a browser you are signed in with. See [YouTube Music](getting-started/youtube-music.md). |
| Local folders | Tags and cover art are read from the files. |
| Internet radio | Stations you add yourself. See [Internet radio](playback/radio.md). |

You can add several servers and switch between them, and one queue can hold songs from different sources. See [Connecting your music](getting-started/sources.md).

## What it adds to a player

- Gapless playback with ReplayGain, an equalizer, a compressor, speed and pitch control and a choice of output device.
- Beat-matched DJ transitions between songs ([DJ transitions](dj/transitions.md)) and an optional AI DJ that picks sets and talks between them ([AI DJ](dj/ai-dj.md)).
- Synced lyrics, a visualizer, music videos for YouTube Music songs and DLNA casting to speakers and TVs.
- Offline downloads from your own server, listening history with statistics, playlists and smart playlists.
- Media keys, global hotkeys, a tray icon, Discord status, a remote-control web page and a built-in MCP server.
- 32 built-in themes, custom themes and 17 interface languages.
- A built-in updater that installs new releases from the project's public GitHub page.

## What Listen does not do

- It does not provide any music. It plays what your servers, files and accounts give you access to.
- It sends no telemetry and has no account of its own. See [Privacy and security](more/privacy.md).
- It does not download YouTube Music songs for offline use.

> **Note:** Listen is an independent project. It is not affiliated with or endorsed by Google, YouTube, Navidrome, Jellyfin or Feishin.

## How to read this guide

If you are new, read the **Getting started** chapters in order: [Installation](getting-started/installation.md), [First start](getting-started/first-start.md), [Connecting your music](getting-started/sources.md), and [YouTube Music](getting-started/youtube-music.md) if you use it. The chapters under **Using Listen** and **Playback** describe everyday use. **Reference** lists every setting, MCP tool and command-line option, and **Development** is for people who want to build or change Listen.

If you want to look around before connecting anything, start the app with `listen --demo` (see [Installation](getting-started/installation.md#try-it-without-a-server)).
