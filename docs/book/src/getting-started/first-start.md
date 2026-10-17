# First start

The first time you start Listen, a short setup wizard offers the extras you may want. This page walks through its steps and what happens afterwards.

The wizard appears only on a fresh install: when no setup has been completed and no server is configured. Every step can be skipped and every choice can be changed later in **Settings**. The steps are listed on the left; click one to jump to it, or use **Back** and **Next**. **Skip setup** at the bottom left closes the wizard at any time.

## 1. Welcome

Choose the interface **Language**. The default, **System default**, follows your system language. The list has 17 languages; see [Languages](../appearance/languages.md). The wizard switches language immediately.

## 2. Features

Tick what you want. Everything here is a normal setting that you can change later.

| Option | What it does |
|---|---|
| **Auto-DJ** | Keeps the music going with similar songs when the queue runs out. See [Play modes and Auto-DJ](../playback/play-modes.md). |
| **DJ transitions** | Mixes songs the DJs pick with beat-matched blends instead of a gap. See [DJ transitions](../dj/transitions.md). |
| **Analyse songs** | Measures tempo, beats, key and energy of your server's songs in the background, for the DJ transitions. See [Song analysis](../dj/analysis.md). |
| **Analyse YouTube Music songs** | Fetches a temporary copy of each YouTube Music song to measure it, so those songs can be mixed too. Needs an analysis folder (next step). |
| **AI DJ** | Picks sets of songs and talks between them. Needs your own Gemini key, so the box is greyed out until you save one in step 4. |
| **Synced lyrics** | Looks up lyrics online when your server has none. See [Lyrics](../playback/lyrics.md). |
| **Discord status** | Shows what you are listening to in your Discord profile. See [Discord status](../integration/discord.md). |
| **Tray icon** | Keeps Listen in the system tray. See [Tray, window and notifications](../integration/window.md). |
| **Check for updates** | Looks for a new version on GitHub when Listen starts. Nothing is installed without asking. See [Updates](updates.md). |

## 3. Analysis folder

DJ transitions need each song's tempo, beats and key. Listen measures them once and keeps the results. Press **Choose folder…** to store them in a folder of your choice. If you pick a synced folder (Nextcloud, Syncthing and so on), your other computers reuse the results instead of measuring again.

Without a folder, results stay in Listen's own data folder and YouTube Music songs are not analysed. **Analyse YouTube Music songs** is off by default and can only be switched on once a folder is chosen; Listen then fetches a temporary copy of each YouTube Music song it is about to mix and keeps only the measurements.

## 4. AI DJ

This step takes your Gemini API key for the AI DJ. You can skip it. The wizard shows the three steps to create a key (open Google AI Studio, click **Create API key**, copy the key that starts with `AIza`), a button **Open Google AI Studio**, a field for the key and **Save key**. After saving, tick **Turn on the AI DJ**.

The full walkthrough, including what is sent to Google, is in [Creating a Gemini API key](../dj/gemini-key.md) and [AI DJ](../dj/ai-dj.md). In short: requests go from your computer straight to Google, and the key is kept in your system keyring. Outside the EU, UK and Switzerland, Google may use free-tier requests to improve its products.

## 5. Helper programs (Windows only)

On Windows the last step installs mpv, yt-dlp and Deno with winget. See [Installation](installation.md#helper-programs). On Linux the wizard has four steps and ends at the AI DJ.

## Connect your music

On the last step the **Next** button reads **Connect your music**. It closes the wizard and opens the sign-in page, where you add your first source. Continue with [Connecting your music](sources.md).

If you cancel out of the wizard with **Skip setup**, the same sign-in page appears. The wizard is not shown again once it is finished, or once a server exists.
