# Settings

This page lists every setting on the Settings page, tab by tab and section by section, in the order they appear. For each row you get what it does, its default, and its key in `config.toml`.

## How to read this page

- Open the Settings page from the sidebar. The tabs are **General**, **Playback**, **Hotkeys**, **Window**, **Advanced** and **About**. The search box at the top filters the rows of all tabs by name and description. **Reset to default** restores the settings after asking for confirmation.
- Settings are saved in `config.toml` in Listen's config folder (see [Files and folders](files.md)). All keys below are written as paths from the top of that file; for example `settings.ai_dj.location` is the key `location` in the table `[settings.ai_dj]`.
- Values written like `"single"` are the exact text stored in the file. Sliders are stored as plain numbers.
- Missing keys fall back to their defaults, so a hand-edited file only needs the keys you change. Edit the file while Listen is closed, or Listen may overwrite your change.
- Secrets (server passwords, the remote password, the MCP token, the Gemini API key) are kept in the system keyring or a restricted file, not in these keys.
- Rows marked Linux or Windows only are hidden on the other system. Some rows only appear when another option is on, or when the connected server supports the feature.

## General

### Servers

Lists your music sources with the buttons **Switch** (shown as **Browsing** for the active one), **Rescan** (local folders), **Edit**, **Remove** and **Add server**. See [Connecting your music](../getting-started/sources.md). Servers are stored in the `[[servers]]` tables, not under `settings`.

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Password storage | Where passwords are kept: **System keyring**, or **File only** (a file only you can read). The row says which one is in use. | System keyring | `secret_store` (`"auto"` or `"file"`; top level) |

### Scrobble

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Scrobble | Reports plays to the server (Last.fm and others the server forwards to). Nothing is reported in private mode. | On | `settings.playback.scrobble` |
| Scrobble minimum mode | When a play counts: **Percentage or seconds** (whichever comes first), **Percentage**, or **Seconds**. Skipping ahead does not count as listening. | Percentage or seconds | `settings.integrations.scrobble_mode` (`"both"`, `"percentage"`, `"seconds"`) |
| Minimum scrobble percentage | The share of the song that has to be played, 25 to 90 %. Hidden in **Seconds** mode. | 75 | `settings.playback.scrobble_at_percent` |
| Minimum scrobble seconds | The time that has to be played, 0 to 1200 s in steps of 5. Hidden in **Percentage** mode. | 240 | `settings.playback.scrobble_at_secs` |
| Song change notifications | Shows a desktop notification with the song, artists, album and cover when the song changes. | Off | `settings.integrations.notify_song_change` |

### Appearance

See [Themes and colours](../appearance/themes.md) for details.

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Follow desktop theme | Linux only. Uses the KDE colour scheme and follows changes live. Takes precedence over the theme choices. | On (Linux) | `settings.appearance.follow_desktop_theme` |
| Follow system theme | Uses the dark or light theme below depending on the desktop's setting. | On | `settings.appearance.follow_system_theme` |
| Dark theme | Shown when **Follow system theme** is on. Used when the desktop is dark. | Tokyo Night | `settings.appearance.theme_dark` (`"tokyoNight"`) |
| Light theme | Shown when **Follow system theme** is on. Used when the desktop is light. | Default Light | `settings.appearance.theme_light` (`"defaultLight"`) |
| Theme | Shown when **Follow system theme** is off. The theme that is always used. | Default Dark | `settings.appearance.theme` (`"defaultDark"`) |
| Use the theme's accent colour | On: the theme keeps its own accent. Off: your accent colour replaces it. | Off | `settings.appearance.use_theme_accent` |
| Accent colour | Five swatches and a field for a CSS colour such as `rgb(53, 116, 252)` or `#3574fc`. Disabled while the previous row is on. | `rgb(53, 116, 252)` | `settings.appearance.accent` |
| Use the theme's primary shade | On: buttons and links use the theme's shade. | On | `settings.appearance.use_theme_primary_shade` |
| Primary shade | 0 (lightest) to 9 (darkest) shade of the accent for buttons and links. Used when the previous row is off. | 6 | `settings.appearance.primary_shade` |
| Custom themes folder | Shows how many custom themes were found and any problems; **Open folder** opens the folder. See [Custom themes](../appearance/custom-themes.md). | | |
| Touchpad scroll speed | 25 to 200 % in steps of 5. At 100 %, a swipe scrolls as far as in Chromium-based browsers. | 100 % | `settings.appearance.scroll_speed` (`1.0` = 100 %) |

### Application

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Font type | **Built-in**, **System** or **Custom**. | Built-in | `settings.appearance.font_kind` (`"builtin"`, `"system"`, `"custom"`) |
| Font | Built-in: Inter or Poppins. System: any installed font. Used for all text. | Inter | `settings.appearance.font` |
| Font file | Custom only. Path to a `.ttf`, `.otf` or `.ttc` file; press **Use** to apply. | empty | `settings.appearance.font_path` |
| Language | The interface language. Applies after a restart. See [Languages](../appearance/languages.md). | System default | `settings.appearance.language` (`""` or a code such as `"de"`) |
| Zoom | Scales the interface, 50 to 300 % in steps of 5, with **-**, **+** and **Reset**. Applies when you let go of the slider. | 100 % | `settings.appearance.zoom` |
| Album background image | Blurred cover behind album page headers; the slider sets the blur (0 to 6). | On, 3.0 | `settings.appearance.album_background`, `settings.appearance.album_background_blur` |
| Artist background image | The same for artist pages. | On, 3.0 | `settings.appearance.artist_background`, `settings.appearance.artist_background_blur` |
| Blur explicit images | Blurs the covers of songs tagged explicit. | Off | `settings.appearance.blur_explicit_images` |
| Native cover aspect ratio | Shows covers whole instead of cropped square; wide covers make their card shorter. | Off | `settings.appearance.native_cover_aspect` |
| Image placeholder | What a cover shows while loading: **ThumbHash**, **Colour** or **Off**. | Colour | `settings.appearance.image_placeholder` (`"thumbHash"`, `"dominantColor"`, `"off"`) |

### Home

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Home featured carousel | Shows the large featured carousel at the top of the home page. | On | `settings.home.feature` |
| Home featured carousel style | **Single** or **Multiple** featured albums. | Single | `settings.home.feature_style` (`"single"`, `"multiple"`) |
| Home sections | Tick the sections to show and drag them into order. | Genres, Most played, Random, Recently added, Recently played, Recently released, Playlists | `settings.home.sections` (list of ids such as `"mostPlayed"`) |

### Player bar

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Player bar button size | Size of the player bar buttons, 15 to 30 px. | 15 | `settings.player_bar.button_size` |
| Show skip buttons | Shows the skip-backward and skip-forward buttons. | On | `settings.player_bar.skip_buttons` |
| Skip backward duration | Seconds to jump back, 0 to 120. | 5 | `settings.player_bar.skip_backward_secs` |
| Skip forward duration | Seconds to jump ahead, 0 to 120. | 10 | `settings.player_bar.skip_forward_secs` |
| Volume slider width | Width of the volume slider, 25 to 300 px. | 70 | `settings.player_bar.volume_width` |
| Volume wheel step | Volume change per mouse-wheel notch on the slider, 1 to 20. | 5 | `settings.player_bar.volume_wheel_step` |
| Player bar opens full-screen player | A click on the empty area of the player bar opens the full-screen player. | Off | `settings.player_bar.open_drawer` |
| Expand album art button | Shows the expand button on the player bar's cover. Turning it off also collapses the sidebar image. | On | `settings.sidebar.show_image` |
| Show remaining time | Shows the time left instead of the duration (a click on the duration also switches). | Off | `settings.player_bar.show_time_remaining` |
| Right-hand buttons | Tick the buttons to show and move them with the arrows: Sleep timer, Player config, Visualizer, Lyrics, Favorite, Queue, Volume. | All shown, in that order | `settings.player_bar.right_controls` (list of `id` and `visible`) |
| Playerbar slider | **Slider** or **Waveform** as the seek bar. The waveform is not recommended on a slow or metered connection. | Slider | `settings.player_bar.slider_type` (`"slider"`, `"waveform"`) |
| Waveform align | Waveform only. **Top**, **Center** or **Bottom**. | Center | `settings.player_bar.waveform_align` |
| Waveform bar width | Waveform only. 1 to 10 px. | 2 | `settings.player_bar.waveform_bar_width` |
| Waveform gap | Waveform only. 0 to 10 px. | 1 | `settings.player_bar.waveform_gap` |
| Waveform radius | Waveform only. Rounding of the bars, 0 to 20 px. | 4 | `settings.player_bar.waveform_radius` |
| Waveform stretch | Waveform only. Stretches the waveform to fill the available space. | Off | `settings.player_bar.waveform_stretch` |
| Waveform loading delay | Waveform only. Seconds to wait before the waveform of a new song is computed, 0 to 10. Raise it if you notice stutters when songs change. | 2 | `settings.player_bar.waveform_delay_secs` |

The metadata badges under the title in the full-screen player (codec, bit rate and so on) are chosen in the full-screen player's **Configure** dialog and stored in `settings.player_bar.info_items` (default `["codec", "release_year"]`).

### Full-screen player

See [Full-screen player and Now Playing](../playback/fullscreen.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Full-screen auto-open timeout | Opens the full-screen player after this many minutes without input while a song plays. 0 turns it off. Range 0 to 120. | 0 (off) | `settings.fullscreen.auto_open_minutes` |
| Configure | Opens a dialog with the options below. | | |

The **Configure** dialog:

| Option | What it does | Default | Key |
| --- | --- | --- | --- |
| Dynamic background | Background from the cover's average colour; off uses the theme background. | On | `settings.fullscreen.dynamic_background` |
| Use the cover as background | Uses the blurred cover instead of a flat colour. | Off | `settings.fullscreen.dynamic_is_image` |
| Image blur | Blur of that image, 0 to 6 rem. | 6 | `settings.fullscreen.image_blur` |
| Opacity | Darkening over the background, 0 to 100 %. | 25 | `settings.fullscreen.opacity` |
| Use the cover's aspect ratio | Keeps the cover's shape instead of a square. | Off | `settings.fullscreen.use_image_aspect_ratio` |
| Cover size | Percent of its column, 55 to 100. | 75 | `settings.fullscreen.cover_size` |
| Alignment | **Left**, **Center** or **Right**. | Center | `settings.fullscreen.alignment` |
| Title | **Scrolling** (one line that scrolls) or **Wrapped**. | Scrolling | `settings.fullscreen.title_display` (`"scroll"`, `"multi_line"`) |
| Title lines | Lines of a wrapped title, 1 to 10. | 1 | `settings.fullscreen.title_lines` |

### Sidebar

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Sidebar configuration | Tick the items to show and drag them into order. Items: Now playing, Search, Home, Favorites, Albums, Tracks, Album artists, Artists, Genres, Folders, Playlists, Collections, Recently played, Statistics, Mixes, Radio, Settings. | All shown except Now playing, Search, Playlists and Settings | `settings.sidebar.items` (list of `id`, `label`, `visible`) |
| Sidebar playlist list | Shows the playlist list in the sidebar. | On | `settings.sidebar.show_playlists` |
| Playlist filter regex | Hides playlists whose name matches this regular expression (case-insensitive). An invalid expression hides nothing. | empty | `settings.sidebar.playlist_filter_regex` |
| Sidebar playlist sorting | Lets you sort playlists by drag and drop instead of the server order. | Off | `settings.sidebar.playlist_sorting` |
| Sidebar playlist mode | **Compact** (name only) or **Expanded** (cover, name and song count). | Expanded | `settings.sidebar.playlist_mode` (`"compact"`, `"expanded"`) |
| Enable folders | Groups playlists whose names contain the separator into folders. | Off | `settings.sidebar.playlist_folders` |
| Folder separator | Shown when folders are on. The character or text that separates folder levels. | `/` | `settings.sidebar.playlist_folder_separator` |
| Folder view | Shown when folders are on. **Single folder**, **Tree view** or **Navigation view**. | Tree view | `settings.sidebar.playlist_folder_view` (`"single"`, `"tree"`, `"navigation"`) |
| Tree indent | Tree view only. Pixels per level, 0 to 64. | 16 | `settings.sidebar.folder_tree_indent` |
| Tree line color | Tree view only. Colour of the guide lines as `#rrggbb`; empty uses the theme. | empty | `settings.sidebar.folder_tree_line_color` |
| Sidebar (collapsed) navigation | Shows the navigation in the collapsed sidebar. | On | `settings.sidebar.collapsed_navigation` |

### Lists

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Play button behavior | What the Play buttons, a double-click on a song and the play hotkey do: **Play now**, **Add to queue next**, **Add to queue last**. Shuffle play and the DJs are not affected. | Play now | `settings.lists.play_behavior` (`"now"`, `"next"`, `"last"`) |
| Grid multi-select | Clicking a card's cover selects it. Off: it opens the item. | On | `settings.lists.grid_multi_select` |
| Show favourites | Shows heart buttons and favourite columns everywhere. | On | `settings.lists.show_favorites` |
| Show ratings | Shows star ratings (Navidrome, OpenSubsonic) on tracks, albums and artists. | On | `settings.lists.show_ratings` |

How each list looks (table, grid, columns) is changed in the list itself and stored under `settings.lists.lists`.

### Artist page

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Artist page configuration | Tick the sections to show and move them with the arrows: Biography, Favorite songs, Recent releases (always shown, only its place changes), Related artists, Top songs. | All on, in that order | `settings.artist.items` (list of `id`, `enabled`) |
| Artist release type configuration | Tick the release types to show and order them as groups: Appears on, Album, Audio drama, Audiobook, Broadcast, Compilation, Demo, DJ-mix, EP, Field recording, Interview, Live, Mixtape / street, Other, Remix, Single, Soundtrack, Spoken word. | All on | `settings.artist.release_types` |
| Artist radio count | Number of songs an artist radio queues, 1 to 100. | 20 | `settings.artist.radio_count` |

### External links

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| External links | Shows links to other sites on artist and album pages. The rows below appear when it is on. | On | `settings.artist.external_links.enabled` |
| Last.fm | Link to the artist or album on Last.fm. | On | `settings.artist.external_links.last_fm` |
| MusicBrainz | Link on MusicBrainz, only where the server knows the MusicBrainz id. | On | `settings.artist.external_links.music_brainz` |
| ListenBrainz | Link on ListenBrainz. | On | `settings.artist.external_links.listen_brainz` |
| Qobuz | Search link on Qobuz. | On | `settings.artist.external_links.qobuz` |
| Spotify | Search link on Spotify. | On | `settings.artist.external_links.spotify` |
| Open in the Spotify app | Opens the Spotify link in the desktop app instead of the web player. | Off | `settings.artist.external_links.native_spotify` |

### Query builder

Shown only when the connected server supports smart playlists.

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Custom fields | Adds your own fields for the smart-playlist builder. Each has a label, a type (string, number, boolean, date, playlist) and a tag. | none | `settings.query_builder.tags` |

### Sharing

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Use the server's default expiration | New share links get no expiry date from Listen; the server decides. | Off | `settings.sharing.expiration.use_server_default` |
| Default share expiration | How long a new share link works: a number and a unit (second, minute, hour, day, week, month, year). You can change it for each share. | 1 year | `settings.sharing.expiration.amount`, `settings.sharing.expiration.unit` (`"year"`) |

## Playback

### Playback

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Audio player | Only shown when the server offers a jukebox. **mpv** plays on this computer; **Jukebox** plays the queue through the server's speakers while this computer only controls it. | mpv | `settings.playback.audio_player` (`"mpv"`, `"jukebox"`) |
| Gapless playback | mpv's `gapless-audio`: `weak`, `yes` or `no`. | weak | `settings.playback.gapless` |
| Streaming quality | Caps the bit rate: **Original**, **High** (320 kbit/s), **Normal** (192), **Low** (96). Navidrome transcodes to MP3; YouTube Music picks a smaller format. Downloaded songs stay original. | Original | `settings.playback.stream_quality` (`"original"`, `"high"`, `"normal"`, `"low"`) |
| Fade songs in and out | Each song fades in at its start and out at its end, 0 to 10 s in half-second steps (0 is off). The AI DJ uses the same speed for turning the music down and up (at least 1 s). | 1.5 s | `settings.playback.track_fade_secs` |
| Fade on pause | Fades out briefly when pausing and back in when resuming. | On | `settings.playback.fade_on_pause` |
| Prefer my own servers for YouTube Music songs | Only shown with a YouTube Music account. Plays a YouTube Music song from your Navidrome, Jellyfin or local library when it has the same song. The song stays a YouTube Music song. See [YouTube Music](../getting-started/youtube-music.md). | On | `settings.playback.prefer_own_servers` |
| Cast to DLNA devices | Looks for DLNA / UPnP speakers and TVs on the network and shows a cast button when some answer. See [Casting](../playback/casting.md). | On | `settings.playback.dlna_enabled` |

The player bar's volume (0 to 100) is saved in `settings.playback.volume` (default 70).

### Audio output

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Audio device | The device mpv plays on, with a **Refresh** button. | auto | `settings.playback.audio_device` |
| Preserve pitch | Keeps the pitch when the playback speed changes. Off: speed also changes the pitch, and the player config offers semitone steps. | On | `settings.playback.preserve_pitch` |
| Microtonal pitch controls | Adds 10 cent steps between the semitone buttons (when the pitch is not preserved). | Off | `settings.playback.microtonal_pitch` |
| Sample rate | Output sample rate in Hz; below 8000 or empty means the device's default. Press Enter to apply. | 0 (device default) | `settings.playback.sample_rate_hz` |
| Exclusive mode | Takes the sound device for Listen alone (WASAPI and some ALSA setups). | Off | `settings.playback.audio_exclusive` |
| Audio format | Sample format sent to the sound card: auto, `s16`, `s32`, `float`, `u8`. | auto | `settings.playback.audio_format` (empty = auto) |

### ReplayGain

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Mode | Normalises loudness using ReplayGain tags: **No**, **Track** or **Album**. | No | `settings.playback.replay_gain` (`"no"`, `"track"`, `"album"`) |
| Preamp | Gain added on top of the ReplayGain value, -15 to +15 dB. | 0 | `settings.playback.replay_gain_preamp_db` |
| Prevent clipping | Lowers the gain so the peak never exceeds full scale. | On | `settings.playback.replay_gain_clip` |
| Fallback gain | Gain for songs without ReplayGain tags, -15 to +15 dB. | 0 | `settings.playback.replay_gain_fallback_db` |

### mpv

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Executable path | The mpv program to run; empty uses `mpv` from the PATH. Needs a reload. | empty | `settings.playback.mpv_path` |
| Extra parameters | One mpv option per line, for example `--audio-channels=stereo`. Needs a reload. | none | `settings.playback.mpv_extra_args` (list) |
| Reload mpv | Stops playback and starts mpv again with the path and parameters above. | | |

### Equalizer

See [Sound](../playback/sound.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Equalizer | Turns the 12-band equalizer on. | Off | `settings.playback.equalizer_enabled` |
| Presets | Built-in presets (Acoustic, Bass Boost, Bass Cut, Classical, Electronic, Flat, Hip-Hop, Jazz, Loudness, Pop, Rock, Treble Boost, Treble Cut, V-Shape and more) and your own: set the bands, type a name and save. **Reset** sets all bands to 0. | none | `settings.playback.equalizer_preset`, `settings.playback.custom_eq_presets` |
| Band sliders | A preamp and 12 bands, each -12 to +12 dB in half-dB steps. | 0 | `settings.playback.equalizer_preamp`, `[[settings.playback.equalizer]]` (`freq`, `gain`) |

### Compressor

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Compressor | Evens out loud and quiet parts. | Off | `settings.playback.compressor.enabled` |
| Presets | Broadcast, Default, Gentle, Heavy, Light, Limiter, Loud Master, Moderate, plus your own. | none | `settings.playback.compressor_preset`, `settings.playback.custom_compressor_presets` |
| Threshold | Level above which compression starts, -60 to 0 dB. | -24 | `settings.playback.compressor.params.threshold` |
| Ratio | How strongly loud parts are reduced, 1 to 20 (n:1). | 4 | `settings.playback.compressor.params.ratio` |
| Attack | How fast it reacts, 0.1 to 2000 ms. | 20 | `settings.playback.compressor.params.attack` |
| Release | How fast it lets go, 1 to 9000 ms. | 250 | `settings.playback.compressor.params.release` |
| Makeup gain | Gain added after compressing, 0 to 30 dB. | 6 | `settings.playback.compressor.params.makeup` |
| Knee | Width of the soft knee, 1 to 8 dB. | 2.83 | `settings.playback.compressor.params.knee` |

### Transcoding

Only shown for servers that can transcode (Navidrome, Jellyfin).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Transcode | Asks the server to convert songs while streaming. Takes the place of **Streaming quality** for those servers. Applies to songs that start from now on. | Off | `settings.playback.transcode.enabled` |
| Format | Shown when on. `mp3`, `opus`, `flac` and so on; empty is the server's choice (Jellyfin: mp3). | empty | `settings.playback.transcode.format` |
| Maximum bit rate | Shown when on. In kbit/s; 0 or empty lets the server pick. | 0 | `settings.playback.transcode.bitrate` |
| Maximum sample rate | Shown when on. In Hz, Jellyfin only; empty is no limit. | 0 | `settings.playback.transcode.max_sample_rate` |

### Queue

See [Player bar and queue](../playback/queue.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Follow current song | Scrolls the queue to the playing song whenever the track changes. | On | `settings.queue.follow_current_song` |
| Up next queue | Labels songs added with Play next and Add to queue as Up Next. They play right after the current song, ahead of the rest of the queue and the Auto-DJ songs. | On | `settings.queue.up_next_queue` |
| Confirm queue changes | Asks before playing something replaces the queue, or before clearing it. | Off | `settings.queue.confirm_queue_changes` |
| Save play queue | Keeps the queue and your position when Listen closes and restores them at the next start. | On | `settings.queue.save_play_queue` |
| Autosave play queue to server | Saves the queue to the server (OpenSubsonic, Navidrome) as the track changes, so another app can pick it up. | Off | `settings.queue.autosave_server` |
| Autosave every | Number of track changes between two saves, 1 to 20. | 1 | `settings.queue.autosave_every` |
| Side play queue style | **Attached** (a side panel) or **Detached** (a drawer that slides in when you rest the pointer on the right edge). | Attached | `settings.queue.side_style` |
| Side queue layout | **Horizontal** (a column on the right) or **Vertical** (a panel under the page). | Horizontal | `settings.queue.side_layout` |

### Player sidebar panels

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Queue panel | Shows the play queue in the right sidebar. | On | `settings.sidebar.show_queue` |
| Lyrics panel | Shows the lyrics of the playing song in the right sidebar. | Off | `settings.queue.lyrics_in_sidebar` |
| Visualizer panel | Shows the spectrum in the right sidebar (captures only Listen's own audio). | On | `settings.visualizer.in_queue_panel` |
| Combine lyrics and visualizer | Shows lyrics with the visualizer under them in one panel. | Off | `settings.queue.combined_panel` |
| Panel order | Order of the panels, top to bottom, set with the arrows. | Queue, Lyrics, Visualizer | `settings.sidebar.panel_order` |

### Player filters

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Player filters | **Add rule** adds a rule that keeps matching songs out of the queue when you play or queue them. Each rule has an on/off switch, a field (Title, Album artist, Artist, Duration (s), Genre, Year, Release year, Note, Path, Play count, Favorite, Rating), an operator (is, is not, contains, does not contain, starts with, ends with, is greater than, is less than, matches regex) and a value. Auto-DJ and the AI DJ are not filtered. | no rules | `settings.queue.filters` (list of `field`, `op`, `value`, `enabled`) |

### Visualizer

See [Visualizer and Milkdrop](../playback/visualizer.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| In the queue panel | Shows the spectrum under the queue. Same switch as **Visualizer panel** above. | On | `settings.visualizer.in_queue_panel` |
| Visualizer settings | **Configure...** opens the spectrum options: mode, gradients, channel layout, frequency scale, peaks, reflex, radial and more, plus saved presets. | | `[settings.visualizer]` |

Some defaults in `[settings.visualizer]`: `mode = 10` (line graph), `gradient = "prism"`, `fft_size = 16384`, `smoothing = 0.6`, `min_freq = 20`, `max_freq = 22050`, `min_db = -85`, `max_db = -25`.

### Milkdrop

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Visualizer type | **Spectrum** or **Milkdrop** (Milkdrop only when projectM is available). | Spectrum | `settings.milkdrop.kind` (`"spectrum"`, `"milkdrop"`) |
| Preset folder | Folder with `.milk` presets, with **Browse...** and **Rescan**; empty uses the default folder in Listen's data folder. | empty | `settings.milkdrop.preset_dir` |
| Preset | Search and pick the preset on screen. | none | `settings.milkdrop.current_preset` |
| Next preset | Switches now. | | |
| Cycle presets | Switches preset every few seconds. | On | `settings.milkdrop.cycle_presets` |
| Cycle time | Seconds a preset stays, 1 to 300. | 30 | `settings.milkdrop.cycle_time` |
| Blend time | Seconds a preset takes to blend into the next, 0 to 10. | 2.5 | `settings.milkdrop.blend_time` |
| Random next preset | Picks the next preset at random; off takes the next in the list. | On | `settings.milkdrop.randomize` |
| Include all presets | Cycles through every preset; off uses only the selected ones. | On | `settings.milkdrop.include_all` |
| Selected presets | Presets to cycle through when the previous row is off. | none | `settings.milkdrop.selected` |
| Ignored presets | Presets that are never picked. | none | `settings.milkdrop.ignored` |

Frame rate and opacity are `settings.milkdrop.max_fps` (0 means 30) and `settings.milkdrop.opacity` (0 to 1, default 1).

### Auto-DJ

See [Play modes and Auto-DJ](../playback/play-modes.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Enabled | Keeps the queue going with similar songs. | On | `settings.auto_dj.enabled` |
| Songs to add | Songs added whenever the queue runs out, 1 to 20. | 5 | `settings.auto_dj.item_count` |

### AI DJ

See [AI DJ](../dj/ai-dj.md) and [Creating a Gemini API key](../dj/gemini-key.md). Everything the DJ sends to Google is listed in the section's note: song titles, artists, the DJ's lines and your details for the DJ. Outside the EU, UK and Switzerland, Google may use free-tier requests to improve its models.

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Enabled | Picks sets of songs that fit the mood and talks between them like a radio host. Replaces the Auto-DJ while on. | Off | `settings.ai_dj.enabled` |
| Gemini API key | Your own key, saved in the system keyring. Save an empty field to remove it. | none | keyring, not in `config.toml` |
| Persona | The host or hosts: Late-Night, Morning Show, Calm, Hamburg Duo. **Preview** plays a sample (needs a key). | Late-Night | `settings.ai_dj.persona` (`"late_night"`, `"morning"`, `"calm"`, `"hamburg_duo"`) |
| Voice model | **Flash-Lite TTS (fast)** or **Flash TTS (best voice acting)**. Both are on Gemini's free tier; the daily speech quota is small. | Flash-Lite TTS | `settings.ai_dj.tts_model` |
| Songs per set | The DJ picks 2 up to this many songs between breaks, 2 to 8. | 5 | `settings.ai_dj.set_max` (and `set_min`, 2) |
| Talk after | Talk after every set, or only every 2nd to 5th set (saves speech quota). | 1 | `settings.ai_dj.talk_every` |
| Spoken breaks per day | Daily limit of spoken breaks, 0 to 50; beyond it the lines are shown as text. | 15 | `settings.ai_dj.max_voiced_per_day` |
| Voice volume | Relative to the music, -12 to +12 dB. | 0 | `settings.ai_dj.voice_gain_db` |
| Location | Where the show airs, for example your town; the hosts mention it now and then. Empty means no home town. | empty | `settings.ai_dj.location` |
| Pronunciation | How the voice should say the place name if it gets it wrong. | empty | `settings.ai_dj.location_pronunciation` |
| Language | The language the hosts speak. **Same as the app** or one of the 17 languages. | Same as the app | `settings.ai_dj.language` (`""` or a code) |
| Details for the DJ | Anything the hosts should know: your name, what you like, topics to avoid. Sent to Gemini with every break. | empty | `settings.ai_dj.notes` |

`settings.ai_dj.text_model` (default `gemini-3.5-flash-lite`) names the text model and has no row on the Settings page.

### DJ transitions & song analysis

See [Song analysis](../dj/analysis.md) and [DJ transitions](../dj/transitions.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Analyse songs | Measures tempo, beats, key, energy and sections of Navidrome songs for the DJs. | On | `settings.analysis.enabled` |
| YouTube Music songs | Also analyses YouTube Music songs: Listen fetches a temporary copy of each to measure it and keeps only the results. Needs an analysis folder first. Not shown in the demo. | Off | `settings.analysis.youtube_music` |
| Whole library | Works through the library in the background; the queue always comes first. | On | `settings.analysis.analyze_library` |
| Also on battery | Keeps analysing when the computer is unplugged. | Off | `settings.analysis.on_battery` |
| Analysis folder | Where results are kept, with **Save** and **Choose...**. A synced folder lets your other computers reuse them. Empty uses Listen's data folder. | empty | `settings.analysis.shared_dir` |
| DJ transitions | Mixes songs the Auto-DJ or AI DJ picked: beat-matched blends, or fades when songs do not fit. | On | `settings.mixing.enabled` |
| Order by flow | Picks songs whose tempo, key and energy follow on from the current one. | On | `settings.mixing.flow_select` |
| Largest tempo change | Beyond this the songs fade instead of being beat-matched, 2 to 8 %. | 6 | `settings.mixing.max_tempo_pct` |
| Long blends | Length of a blend over instrumental intros and outros: 8 or 16 bars. | 16 | `settings.mixing.long_blend_bars` |
| Transition edges | Whether vocals at a song's end or start may be blended over: Always, Instrumental only, or Smart (relaxed for club genres such as garage, house, techno and drum and bass). | Smart | `settings.mixing.transition_edges` (`"smart"`, `"always"`, `"instrumental"`) |
| Tempo ramps | Glides the tempo between songs instead of jumping. | On | `settings.mixing.tempo_ramps` |
| Bass swap | Swaps the basslines on a downbeat so only one plays at a time. | On | `settings.mixing.bass_swap` |
| Mix queued songs | Also mixes into songs added with Add to queue while a DJ is on. | On | `settings.mixing.mix_manual_queue` |

Other keys without a row: `settings.mixing.fallback_fade_secs` (plain fade length, default 4) and `settings.analysis.threads` (0 means automatic).

### Lyrics

See [Lyrics](../playback/lyrics.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Fetch lyrics | Gets lyrics from the server, YouTube Music or the providers below. | On | `settings.lyrics.fetch` |
| Prefer local lyrics | Uses the server's or YouTube Music's own lyrics before the providers'. Plain ones still give way to synchronized lyrics from a provider. | On | `settings.lyrics.prefer_local` |
| Lyric providers | Which providers are asked and in which order; the first with synchronized lyrics wins. Choices: NetEase, lrclib.net, Genius, SimpMusic. | NetEase, lrclib.net | `settings.lyrics.sources` |
| Lyric offset | Default shift of the lyrics against the music, in milliseconds (the lyrics view can change it per song). | 0 | `settings.lyrics.delay_ms` |
| Alignment | **Left**, **Center** or **Right**. | Center | `settings.lyrics.alignment` |
| Lyric size (synchronized) | Font size of timed lyrics. | 24 | `settings.lyrics.font_size` |
| Lyric size (unsynchronized) | Font size of untimed lyrics. | 24 | `settings.lyrics.font_size_unsync` |
| Lyric gap (synchronized) | Space between lines. | 24 | `settings.lyrics.gap` |
| Lyric gap (unsynchronized) | Space between lines of untimed lyrics. | 24 | `settings.lyrics.gap_unsync` |
| Left padding | Empty space left of the lyrics, in percent of the width. | 0 | `settings.lyrics.padding_left` |
| Right padding | Empty space right of the lyrics, in percent of the width. | 0 | `settings.lyrics.padding_right` |
| Follow the current lyric | Scrolls along with the line being sung. | On | `settings.lyrics.follow` |
| Follow position | Where the current line sits while following: -50 near the top, 0 centred, 50 near the bottom. | 0 | `settings.lyrics.follow_scroll_alignment` |
| Line lead time | A line lights up this many milliseconds before its time. | 800 | `settings.lyrics.line_lead_time_ms` |
| Opacity of other lines | How visible the lines that are not being sung are. | 0.2 | `settings.lyrics.opacity_non_active` |
| Size of other lines | Size of those lines compared to the current one. | 0.95 | `settings.lyrics.scale_non_active` |
| Show the match | Shows which song the provider matched. | Off | `settings.lyrics.show_match` |
| Show the provider | Shows where the lyrics came from. | On | `settings.lyrics.show_provider` |
| Romaji | Shows Japanese lyrics in Latin letters under the line. Kanji are read only in builds that include the Japanese dictionary. | Off | `settings.lyrics.enable_romaji` |
| Furigana | Shows the reading above kanji in Japanese lyrics. Only in builds with the dictionary. | Off | `settings.lyrics.enable_furigana` |
| NetEase translations | Fetches NetEase's own translation and shows it under each line. | Off | `settings.lyrics.enable_netease_translation` |
| Translation language | The language online translation translates into. | English | `settings.lyrics.translation_target_language` (`"en"`) |
| Translation provider | **None**, **Microsoft Azure** or **Google Cloud**. Needs your own API key. | None | `settings.lyrics.translation_api_provider` |
| Translation API key | The key of that provider. It is stored in `config.toml`, so keep that file private. | empty | `settings.lyrics.translation_api_key` |
| Automatic translation | Translates every song's lyrics when they load (needs the provider and key). | Off | `settings.lyrics.enable_auto_translation` |

## Hotkeys

See [Hotkeys](../integration/hotkeys.md) for how to record keys and what the global option does.

**Application hotkeys.** One row per action. Click the field, press the keys (`Esc` cancels, the cross clears it). A key bound twice is outlined in red. On Linux, a **global** checkbox next to an action registers it system-wide through the desktop's shortcut portal; it needs a key to be set first, and only some actions allow it.

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Each action | Browser back / forward, Favorite / Unfavorite / Toggle favorite of the current and previous song, Global search, Local search, List: navigate to page / play (default) / play last / play next / play now / show playing song, Navigate to home, Next / Previous track, Next / Previous album, Pause, Play, Play / pause, Rate 0 to 5 stars, Skip backward / forward, Stop, Toggle full-screen player, Toggle queue, Toggle repeat, Toggle shuffle, Volume up / down / mute, Zoom in / out. | Play / pause `space`; Global search `mod+k`; Local search `mod+f`; List: show playing song `mod+l`; List: navigate to page `mod+g`; List: play (default) `enter`; all others unbound | `settings.hotkeys.<action>` (for example `settings.hotkeys.playPause = "space"`) |
| global (Linux) | Registers the action system-wide. | none | `settings.hotkeys_global` (list of action ids) |

**System-wide** (Linux only):

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Global media hotkeys | Lets Listen grab the media keys through the desktop portal. Off: only MPRIS controls Listen. On KDE, GNOME, Cinnamon and Budgie the desktop already sends the keys over MPRIS, so Listen does not bind them again. | On | `settings.global_media_keys` |

## Window

### Window & integrations

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Private mode | Nothing is scrobbled, shown on Discord or saved in the history, and the window title says so. Turns off again when Listen restarts. | Off | not saved |
| Window bar style | **Windows** or **macOS** buttons in Listen's own bar, or **Native** for the desktop's frame. | Windows | `settings.window.custom_title_bar` and `settings.window.custom_bar_style` (`"windows"`, `"macos"`); Native sets `custom_title_bar = false` |
| Track info in window title | Shows the playing song in the window bar and title. | On | `settings.window.title_bar_track_info` |
| Tray icon | Shows Listen in the system tray. | On | `settings.window.tray` |
| Minimize to tray | Shown when the tray icon is on. Minimizing hides the window in the tray. | Off | `settings.window.minimize_to_tray` |
| Close to tray | Shown when the tray icon is on. Closing the window keeps Listen playing in the tray. | Off | `settings.window.exit_to_tray` |
| Start minimized | Shown when the tray icon is on. Starts with only the tray icon. | Off | `settings.window.start_minimized` |
| Prevent display sleep while playing | Keeps the screen on and the system awake while music plays. | Off | `settings.window.prevent_display_sleep` |
| Prevent system sleep while playing | Keeps the system from suspending; the screen may still turn off. Shown as on while the previous row is on. | Off | `settings.window.prevent_system_sleep` |
| Discord Rich Presence | Shows what you are listening to on Discord. See [Discord status](../integration/discord.md). | Off | `settings.window.discord_rpc` |
| Discord application ID | The Discord application used for the presence. Press Enter to apply. | Listen's own application | `settings.window.discord_client_id` |
| Show presence when paused | Keeps the presence while paused, with the paused icon and no timer. | On | `settings.integrations.discord.show_paused` |
| Show playing icon | Shows the small playing icon next to the cover while a song plays. | On | `settings.integrations.discord.state_icon` |
| Show status as listening | Shows "Listening to" instead of "Playing". | Off | `settings.integrations.discord.show_as_listening` |
| Discord display type | What the status line shows: **Listen**, **Song name** or **Artist name**. | Listen | `settings.integrations.discord.display` (`"feishin"`, `"song"`, `"artist"`) |
| Discord link type | Links the song, artist and cover to **None**, **Last.fm**, **MusicBrainz** or **MusicBrainz (Last.fm fallback)**. | None | `settings.integrations.discord.link` (`"none"`, `"last_fm"`, `"music_brainz"`, `"music_brainz_last_fm"`) |
| Show server cover image | Sends the server's cover to Discord. Only used when the server is reachable from the internet over https; the link includes your access token, so Discord and its users can see it. YouTube Music covers are always shown; local covers are never sent. | Off | `settings.integrations.discord.server_image` |
| Last.fm API key | Looks up the album cover on Last.fm when no other cover is available for Discord. Press Enter to save. | empty | `settings.integrations.discord.lastfm_key` |

### Remote control

See [Remote control](../integration/remote.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Enable remote control server | Serves a remote control page. The description shows the address while it runs, or why it did not start. | Off | `settings.integrations.remote.enabled` |
| Remote server port | The port of the server. Press Enter to apply. | 4333 | `settings.integrations.remote.port` |
| Remote server address | `0.0.0.0` lets other devices in your network connect; `127.0.0.1` only this computer. Press Enter to apply. | `0.0.0.0` | `settings.integrations.remote.bind` |
| Remote server username | The user name to sign in with. Leave both fields empty for no sign-in. | empty | `settings.integrations.remote.username` |
| Remote server password | The password to sign in with. Press Enter to save; it is kept in the keyring. | none | keyring, not in `config.toml` |

A warning appears when the server is on without a username and password: anyone in your network can then control Listen.

### MCP server

See [MCP server](../integration/mcp.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Enable MCP server | Lets an MCP client control Listen. | Off | `settings.integrations.mcp.enabled` |
| MCP server port | Press Enter to apply. | 4334 | `settings.integrations.mcp.port` |
| Allow other computers | Off: only programs on this computer can connect. On: anyone in your network who has the token can. | Off | `settings.integrations.mcp.allow_lan` |
| Access token | Clients send it as `Authorization: Bearer <token>`. Kept in the keyring. **Copy token** copies it, **New token** replaces it (old clients stop working). | generated | keyring, not in `config.toml` |
| Connect Claude Code | **Copy command** copies the command that adds Listen to Claude Code. Other clients take the same URL and header. | | |

## Advanced

### Updates

See [Updates](../getting-started/updates.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Check for updates | Shows the current version or the result; **Check now** looks for a new one. | | |
| Update available | Appears when a newer version exists; **View update** opens it. | | |
| Check on start | Looks for a new version when Listen starts. You decide whether to install it. | On | `settings.advanced.update_check_on_start` |
| Release channel | **Stable** offers full releases; **Beta** also offers pre-releases. | Stable | `settings.advanced.update_channel` (`"stable"`, `"beta"`) |
| Helper programs | Windows only. **Show** and **Update** manage mpv, yt-dlp and Deno, installed with winget. Update them when YouTube Music songs stop playing. | | |
| Release source | **Open releases** opens the public releases page; downloads are checked against their SHA256SUMS. No account needed. | | |

The version you chose to skip and the "remind me later" time are stored in `settings.advanced.skipped_version` and `settings.advanced.update_remind_after`.

### Listening history

See [History, statistics and mixes](../using/history.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Record listening history | Saved only on this computer (song, server, time); never uploaded or shared. Off writes nothing. | On | `settings.history.enabled` |
| Don't record this session | Pauses recording until you close Listen. | Off | not saved |
| Keep history for | **1 year**, **2 years** or **Unlimited**. Older entries are deleted automatically. Unlimited is needed for the year review. | Unlimited | `settings.history.retention` (`"oneyear"`, `"twoyears"`, `"forever"`) |
| Export | Writes the history to your Downloads folder as **JSON** or **CSV**. | | |
| Delete history | **Delete all** (asks to confirm) removes every entry; statistics and mixes disappear too. | | |

### CD ripping

Linux only. See [CD ripping](../more/cd-ripping.md).

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Auto-rip inserted CDs | Rips an audio CD as soon as it goes into the drive, also while Listen is in the tray. Always FLAC, secure rip with AccurateRip (whipper), tagged from MusicBrainz. | On | `settings.ripping.auto_rip` |
| Music folder | Albums go to `Album Artist/Album/NN Title.flac` here. Listen never deletes or overwrites anything in it. **Save** and **Choose...**. | Your system Music folder | `settings.ripping.destination` |
| Eject when done | Ejects the CD after ripping. | On | `settings.ripping.eject_when_done` |
| Notification sound | Plays a sound with the "Album ripped" notification. | On | `settings.ripping.notify_sound` |
| ReplayGain tags | Measures track and album loudness while tagging. | On | `settings.ripping.replay_gain` |
| Scan Navidrome afterwards | Asks Navidrome to look for the new album (needs an admin login; otherwise its file watcher finds it within a minute or two). | On | `settings.ripping.scan_navidrome` |
| Version picker | When several releases match, the best match is taken after this long, 5 to 120 s. | 30 | `settings.ripping.picker_timeout_secs` |
| Drive read offset | Offset in samples; empty uses AccurateRip's drive table. **Save**, or **Verify offset** to check it. | auto | `settings.ripping.read_offset` |
| Rip now | Shown when a CD is ready. **Rip CD** rips the disc in the drive. | | |

### Import from Feishin

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Feishin settings file | Path to a Feishin settings export (JSON); press **Import**. Passwords are ignored. See [Importing from Feishin](../more/feishin-import.md). | | not saved |

### Settings backup

See [Backup, logs and diagnostics](../more/backup.md).

| Row | What it does | Key |
| --- | --- | --- |
| Export settings | Writes all settings to a JSON file in your Downloads folder. Passwords and tokens are not included. | |
| Import settings | Reads a file made by **Export settings**. Every value is checked; values that do not fit are skipped and listed. Replaces your current settings. | |

### Logging

| Row | What it does | Default | Key |
| --- | --- | --- | --- |
| Log level | How much `listen.log` records: **Info** or **Debug**. Debug helps when something does not play. | Debug | `settings.advanced.log_level` (`"info"`, `"debug"`) |
| Export diagnostics | Saves a zip with the logs, your config and the program versions to Downloads. Passwords, cookies, tokens and URL credentials are removed first. | | |

### Cache

| Row | What it does |
| --- | --- |
| Clear memory cache | Soft clear: forgets covers and data held in memory; they load again as you browse. Files on disk stay. |
| Cover art cache | Downloaded album and artist images, with the size on disk and a **Clear** button. |
| YouTube Music audio cache | Songs kept for gapless play and replays, with size and **Clear**. |
| Waveform cache | Waveforms of the player bar slider, with size and **Clear**. |
| Clear all caches | **Refresh sizes** re-measures; **Clear all** deletes every cache above. Your library, playlists and settings are not touched. |

## About

The **About** tab has no options. It shows the version, the license (GNU Affero General Public License v3.0 or later) and a link to the source, followed by credits for the design (Feishin), music sources, online services, and the software, fonts and icons Listen uses.

## Keys that have no row

A few keys are stored automatically and are not on the Settings page: `settings.settings_tab` (the tab you last had open), `settings.onboarded` (the first-start wizard was finished), `settings.window.state` (last window size and position), `settings.sidebar.width` and `settings.sidebar.queue_width` (dragged panel widths), `settings.lists.saved_queries` and `settings.lists.collections` (saved list filters), and the sidebar's hand-made playlist order.
