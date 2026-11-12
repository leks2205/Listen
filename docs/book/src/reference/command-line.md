# Command line

Listen is a normal desktop program, so you rarely need the command line. This page lists every option of the `listen` command, the self-check commands that help with problems, and the environment variables Listen reads. On Windows, use `listen.exe` from a terminal; its output appears in the terminal you started it from.

```text
listen [OPTIONS]
```

## Starting the app

| Option | What it does |
|---|---|
| `--demo` | Uses the built-in demo library and generated audio. No server needed and nothing is saved. A good way to look around first. |
| `--queue` | Opens the queue sidebar at start (with `--demo`). |
| `--video` | Starts with the song/video switch on (with `--demo`). |
| `--fullscreen` | Opens the full-screen player at start (with `--demo`). |
| `--theme <ID>` | Forces a theme by its id for this run, for example `tokyoNight` or `defaultLight`. |
| `--size <WxH>` | Initial window size in pixels, for example `--size 1280x800`. The default is `1600x950`. |
| `--route <PAGE>` | Opens on `home`, `albums`, `tracks`, `artists`, `favorites`, `recent`, `stats`, `mixes`, `radio` or `settings`. If Listen is already running, the running window is brought forward and shown that page. |
| `--login <URL> <USER>` | Signs in to a server for this run only. The password is read from the environment variable `LISTEN_PASSWORD`. Nothing is saved. |
| `--autoplay` | Starts playing the first album on the home page (with `--login`). |
| `--ephemeral` | Does not read or write settings or credentials. Meant for tests and demos. |
| `--onboarding` | Shows the first-start wizard again (combine with `--ephemeral` to see it without changing your settings). |
| `--screenshot <PATH>` | Saves a PNG of the window after start-up and exits. |
| `--screenshot-delay <MS>` | Waits this long before the screenshot. The default is `2500`. |
| `-h`, `--help` | Prints the usage text and exits. |
| `-V`, `--version` | Prints the version and exits. |

An unknown option prints an error with the usage text and exits with status 2.

Listen allows one running copy per user. Starting it a second time brings the first window forward and exits (it prints "Listen is already running; brought it forward." and returns 0), except with `--demo`, `--ephemeral`, `--screenshot` and `--login`, which start their own copy.

## Check commands

These commands do their job in the terminal and exit; they never open the window. They must be the **first** argument. They read your normal configuration and sign-ins, print what they find with a check mark or cross for each step, and exit with `0` when everything is fine and `1` otherwise. Use `RUST_LOG=debug` to see more detail.

| Command | What it does |
|---|---|
| `listen --check-youtube-music` | Signs in to YouTube Music the way the app does (with your browser's session) and prints what Listen sees of your library: counts and page titles only, never cookies. Also checks that a stream can be resolved with yt-dlp. |
| `listen --check-ai-dj [--no-voice] [--rounds N] [--no-judge] [--dump-prompt] [--request R] [--offline]` | Runs AI DJ rounds (2 by default) against Gemini with a sample song pool that carries play counts, "new to you" tags and tempo/key/energy, and saves the first two spoken breaks as WAV files. A second Gemini call grades every break (no invented facts, persona, language, no repeats, no talk about the mixing, a new-to-you song gets introduced); the command fails if fewer than 80 % of the breaks pass, fewer than 80 % of the sets with a new song introduce one, or (with Gemini) fewer than 80 % of the picked songs carry a "Why this song?" note. `--no-voice` skips the speech, `--no-judge` the grading, `--dump-prompt` prints each prompt. `--request` (repeatable: `calmer`, `more_energy`, `more_new`, `only_favourites`, `focus`, `korean_only`, `no_ballads` or free text) tells the DJ a request before round 2, as **Tell the DJ** does; with Calmer or More energy the command fails unless the next 5 songs have less (more) energy on average than the first set. At the end it prints the tokens per plan. `--offline` needs no key and costs nothing: a stand-in planner picks against the request (the most energetic songs left for Calmer, the calmest for More energy), so only Listen's own song filter can keep the request; no speech, no grading. Otherwise it needs a Gemini key (saved in Settings or in `LISTEN_GEMINI_API_KEY`) and uses your Gemini quota: one text request per round, one more per round for the grade, and up to two speech requests. |
| `listen --song-card <TITLE> <ARTIST> [--album A] [--year Y] [--lang de\|en]` | Builds the full-screen **About** card for a song and prints it as JSON with a summary line (facts, credits, whether it would be shown). Fails when a fact has no source link. Needs a Gemini key and costs up to three Gemini requests; nothing is cached. |
| `listen --known-words <TITLE> <ARTIST> [--lrc FILE] [--hub URL]` | Prints a song's Korean words with their Lute status and the share of known words as JSON, the way the lyrics' **WORDS** mode colours them and the Learnable songs mix counts them. Lyrics come from lrclib (or an LRC file); the hub from `--hub` or the lyrics settings. Only reads from the hub. |
| `listen --weekly-stats [--history PATH] [--now UNIX]` | Prints the numbers of Statistics → **Last 4 weeks** as JSON: this week and last week (plays, listening time, skips, different songs and artists), the 4 weeks' totals and top lists, new artists this week and the songs skipped most. `--history` reads another `history.jsonl` (default: this computer's), `--now` sets the moment the rolling windows end. |
| `listen --check-cd` | Checks CD ripping without ripping: tools, drive and read offset, the disc's table of contents and MusicBrainz match, the music folder and Navidrome. See [CD ripping](../more/cd-ripping.md). |
| `listen --check-analysis [N \| FILE-OR-URL \| ytm:ID \| ytm [N]]` | Analyses N random Navidrome songs (default 20) and compares the tempo with BPM tags, or prints the full analysis of one file, URL or YouTube Music song. |
| `listen --analyze-library [--jobs N]` | Analyses every not-yet-analysed Navidrome song, N at a time (default 3), into the analysis folder, outside the app. It writes under its own name, so a running Listen does not collide with it. |
| `listen --check-mix A B [--render OUT.wav] [--lyrics-a A.lrc] [--lyrics-b B.lrc] [--levels OUT.jsonl]` | Plans the DJ transition from song A into song B as the player would and optionally renders a preview file. A song is a Navidrome song id, `ytm:<videoId>`, a local file or a URL. Synced lyrics given as LRC files are used like fetched ones (where the singing is, line ends, chorus). Prints the plans the check rejected and the chosen plan's check; with `--render` it also prints the rendered preview's loudness hole and bump. `--levels` writes each song's modelled and measured local loudness every 0.5 s (also through the bass swap's shelf) with its bar curves. |
| `listen --check-ytm-session [--write]` | For developers. Tests the write functions of the YouTube Music session (like and unlike, a throwaway playlist) and undoes them. Without `--write` it only prints its plan. It acts on your real account, so only use it if you know why. |
| `listen --memory [--history PATH] [--now UNIX] [--limit N]` | Prints the memory model as JSON: the top forgotten favourites (used by the Auto-DJ, the AI DJ pool and Rediscover) and the worn-out songs rested from automatic picks now. |
| `listen --flow-weights [--history FILE] [--store DIR] [--simulate N [--seed S]]` | Prints the flow weights Listen learns from your skips: the labelled handovers, the cross-validated AUC next to the default weights' AUC, and the weights. `--simulate` uses a simulated listener over the analysis store instead of the history. |
| `listen --focus [--history PATH] [--store DIR] [--fills N] [--per-fill N] [--pool N] [--seed N]` | Compares focus-mode fills with ordinary ones on your history's songs and prints the mean share of sung bars of both as JSON, for you to judge (focus should be at most half of normal). Always exits with `0`. |
| `listen --check-bridges <ANALYSIS> <EMBEDDINGS> <HISTORY> [--pairs N] [--all] [--gain X]` | For developers. Replays the DJ bridge chooser over consecutive plays of a `history.jsonl` and prints each bridge with and without the scene gate. |
| `listen --check-ai-dj-pool <ANALYSIS> <EMBEDDINGS> <HISTORY> [--at TITLE]…` | For developers. Rebuilds the AI DJ's candidate pool at moments of the history and shows what the scene gate keeps. |
| `listen --check-tempo <ANALYSIS> <EMBEDDINGS> <HISTORY> [--bpm N] [--want N] [--current HINT] [--before HINT]… [--probe HINT]…` | For developers. Builds a heart-rate tempo fill for a target tempo around a session of songs (hints are `Artist – Title` as the analysis store spells them; an unknown `--current` is an error). |

For the three replay commands, `ANALYSIS` is a copy of the analysis folder (the one that holds `v1/`; the command creates its own machine folder in it), `EMBEDDINGS` is `…/embeddings/v1/clamp3` and `HISTORY` a `history.jsonl`. Each fails with a message naming the path when one of them is empty or at the wrong level.

`listen --analysis-worker` is used internally by Listen to analyse songs in a separate process. You do not run it yourself.

## Environment variables

Set these in the terminal before the command (`LISTEN_LANG=de listen`) or, for a permanent effect, in your desktop session or the launcher.

### Useful for everyone

| Variable | Effect |
|---|---|
| `LISTEN_LANG` | Overrides the language for this run. Use a code from the language list (`en`, `de`, `fr`, `es`, `zh-Hans`, …) or a locale name such as `de_DE.UTF-8`. See [Languages](../appearance/languages.md). |
| `LISTEN_GEMINI_API_KEY` | Gemini API key for the AI DJ. When set, it is used instead of the key saved in the keyring. See [Creating a Gemini API key](../dj/gemini-key.md). |
| `LISTEN_YTDLP` | Full path of the `yt-dlp` program to use for YouTube Music. Without it Listen runs `yt-dlp` from the `PATH` (on Windows also next to `listen.exe` and in `%LOCALAPPDATA%\Listen\bin`). |
| `LISTEN_MPV` | Full path of the `mpv` program. Takes precedence over **Settings → Playback → mpv → Executable path**. |
| `LISTEN_MPV_ARGS` | Extra mpv options for this run, separated by spaces, added to the **Extra parameters** from the settings. Example: `LISTEN_MPV_ARGS="--ao=null"` for a silent run. |
| `LISTEN_PASSWORD` | The password for `--login`. |
| `RUST_LOG` | How much Listen prints to the terminal, in the usual Rust syntax, for example `RUST_LOG=debug` or `RUST_LOG=listen=trace,warn`. The default is `listen=info,listen_core=info,warn`. It does not change `listen.log`; that is set by **Log level** in the settings. |

### Display and graphics

| Variable | Effect |
|---|---|
| `WINIT_UNIX_BACKEND` | Set to `x11` on a Wayland session to run Listen through X11 instead. |
| `LISTEN_MILKDROP_EGL` | Forces how the Milkdrop visualizer creates its graphics context: `device` (a GPU device), `surfaceless` (Mesa) or `default`. By default Listen tries them in that order. Useful when Milkdrop shows nothing. |
| `LISTEN_VIZ_DEBUG` | Set to `1` to log visualizer frames and audio capture start and stop once a second. |

### Analysis

| Variable | Effect |
|---|---|
| `RTEN_NUM_THREADS` | Number of threads for the beat-detection model. If you set it, it wins over the `threads` setting; otherwise Listen sets it from the setting. |
| `LISTEN_ANALYSIS` | Any value turns song analysis on in an `--ephemeral` run (it is off there by default). |

### Folders

On Linux the standard XDG variables `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_RUNTIME_DIR` decide where Listen keeps its files; see [Files and folders](files.md). On Windows `APPDATA` and `LOCALAPPDATA` do.

### Testing and development

You will not need these in normal use.

| Variable | Effect |
|---|---|
| `LISTEN_AI_DJ_PERSONA` | Persona id for `--check-ai-dj`. |
| `LISTEN_AI_DJ_JUDGE_MODEL` | Text model that grades the breaks in `--check-ai-dj` (default: the AI DJ's text model). |
| `LISTEN_AI_DJ_MODES` | Forces the order of AI DJ break types, for example `outro,intro,across,between`. |
| `LISTEN_AUTOPLAY_ALBUM` | With `--autoplay`, plays the first search result for this album name instead of the first home album. |
| `LISTEN_CHECK_STREAMS`, `LISTEN_CHECK_ALL`, `LISTEN_CHECK_SHAPE` | Extend `--check-youtube-music`: open N random tracks in a silent mpv, open every playlist and album instead of a few, and print the page structure of the first album that fails to open. |
| `LISTEN_STARTUP_DELAY_MS` | Waits up to 5000 ms before starting. Listen sets it itself when it restarts on Windows. |
| `LISTEN_I18N_COLLECT` | Writes the interface texts that are shown to the given file, for translators. See [Translations](../development/translations.md). |
| `LISTEN_DEMO_VIDEO_KBPS`, `LISTEN_DEMO_VIDEO_FIRST_BYTE_MS` | Speed of the demo video stream. |
