# AI DJ

The AI DJ picks sets of songs from your library that fit the mood of the song you started it on, and talks between the sets like a radio host. It uses Google's Gemini models with your own API key. This page explains how to set it up, what every option does and what is sent to Google. To get a key, see [Creating a Gemini API key](gemini-key.md).

The AI DJ is optional and off by default. It replaces the [Auto-DJ](../playback/play-modes.md) while it is on, and it works together with [DJ transitions](transitions.md) and [song analysis](analysis.md).

## Turn it on

1. Create a Gemini API key ([instructions](gemini-key.md)).
2. Open **Settings → Playback → AI DJ**.
3. Paste the key into **Gemini API key** and press **Save**. Listen plays a short sample of the current persona's voice to check that the key works. The result appears in a status line below the options (for example "Late-Night works.").
4. Switch **Enabled** on. If there is no key yet, Listen refuses and tells you to add one. You can also tick **Turn on the AI DJ** in the setup wizard.
5. Start any song and choose **AI DJ** with the play-mode button in the player bar.

The song you start from becomes the seed: the mood the DJ tries to keep. Songs you add to the queue yourself while the DJ is on steer it too. If you want a new mood, start another song and switch the DJ on again.

## What it does

While the AI DJ is on, Listen plans one set at a time. When the last song of a set starts, it asks Gemini for the next set and a line to say. Gemini chooses the songs from a list of about 40 candidates taken from your music server: songs similar to the seed, some similar to the current song, songs similar to your latest manual picks and a few random ones. Songs that played this session, are in the queue or were picked recently are left out.

With the listening history on, up to 5 of the candidates come from it: songs from your heavy rotation of the last 30 days (not ones played today) and favourites you have not played for two months, the ones that flow best from the current song. Each candidate also tells the DJ what you know of it: how often you played it and when you last did, or that it is new to you (and whether its artist is). The DJ mixes familiar songs with new ones, and when it talks before a set that has a song new to you, it introduces one of them with a short line about it. It is told to state only facts it is sure of and otherwise to talk about the sound. "New to you" is only used once the history covers at least two weeks and 100 plays, so a fresh install does not call every song new. These tags go to Gemini with the song titles (see [Privacy](../more/privacy.md)).

With every set the DJ also writes a short note per song (at most 80 characters) on why it fits right now. Hold the pointer over the **AI** tag of a song in the queue to read it; the note of the playing song is shown in the full-screen player.

The break is spoken with a voice model and placed at random where it fits:

- **between songs**, in the silence after the current song;
- **over the end** of the current song, which is turned down while the host talks;
- **over the start** of the first song of the new set;
- **across the change**, starting over the end of one song and finishing over the start of the next.

The music is turned down while the voice speaks and back up afterwards. Breaks are short, at most 10 to 20 seconds of speech.

### Tell the DJ

While the AI DJ is on, the queue panel (and the Now Playing page) shows a **Tell the DJ…** box with quick requests under it: **Calmer**, **More energy**, **More new songs**, **Only favourites**, **Focus**, **Korean only** and **No ballads**. Click a chip to ask for it and click it again to take it back, or type your own request ("more 2010s girl groups") and press Enter; typed requests show as chips with an × to remove them.

A new request takes effect at once: the songs the DJ queued that have not started yet are taken out and it plans a new set with the request. The song playing now plays on (a transition already planned into the next DJ song keeps that song, and the new set follows it), and songs you queued yourself stay; while some of those are still queued, the new set comes after them. In its next break the DJ briefly says it heard you. Asking for a request that is already active changes nothing and costs no Gemini request.

Requests stay active, also after a restart, until you remove them, and every later set follows them. Calmer and More energy exclude each other, and so do More new songs and Only favourites. With analysed songs the requests also narrow the candidate list before Gemini sees it: Calmer keeps only songs with less energy than the ones it replaced (More energy: more), even when that leaves just one or two for a shorter set; only when no candidate has less (more) energy it keeps the eight calmest (most energetic), No ballads drops slow, soft songs, Focus drops the loudest ones, Only favourites keeps songs you have played at least three times (and takes more of them from your history) and More new songs keeps mostly songs new to you. Korean only is left to the DJ, because titles don't show the language reliably.

### How it uses the analysis

If songs are analysed, the DJ knows more than titles. Each candidate is described with tempo, key and energy, plus a short conclusion of how it follows the current song (keys compatible or clashing, tempo close or a big jump, energy up or down), and the DJ is told how the mix engine would bring it in after the current song. The candidate list is ordered by how well the songs flow from the current one, so the DJ is offered songs that mix well. The DJ can also mention its mixes, and it keeps a talk-over shorter than the instrumental intro of the next song so that it never talks over a singer.

Without analysis the DJ still works; it then relies on the server's own similarity and the server's BPM tag.

### Where it works

The AI DJ picks songs from the server that the seed song belongs to. Beat-matched transitions between the picks need analysis, which exists for Navidrome and (if switched on) YouTube Music songs. When a speech clip is planned, it is not used while you cast to a TV or speaker over DLNA; the lines are then shown as text.

## Personas

A persona sets the character, tone and voice of the host. Pick one in **Persona** and press **Preview** to hear it. The preview is cached, so after the first time it costs nothing.

| Persona | Hosts | Style |
|---|---|---|
| **Late-Night (Puck)** | One host | Relaxed late-night host with dry wit. Short, never cheesy. The default. |
| **Morning Show (Fenrir)** | One host | Upbeat morning-show energy. |
| **Calm (Kore)** | One host | Calm and minimal: one or two sentences, with music knowledge. Only says facts it is sure about. |
| **Hamburg Duo (Hauke & Merle)** | Two hosts | An energetic morning-show pair. Merle chimes in on roughly every other break and the two talk to each other. When the show is in German it has a light northern German flavour. |

The voices are Gemini's built-in voices. Two-host breaks are spoken in a single request.

## Settings

All options are in **Settings → Playback → AI DJ**.

| Setting | Default | Range | What it does |
|---|---|---|---|
| **Enabled** | Off | on/off | Turns the AI DJ on. |
| **Gemini API key** | empty | | Saved in the system keyring. Paste a new one to replace it, save an empty field to remove it. |
| **Persona** | Late-Night | 4 personas | Who talks. **Preview** plays a sample. |
| **Voice model** | Flash-Lite TTS (fast) | 2 models | See below. |
| **Songs per set** | up to 5 | 2 to 8 | The DJ picks 2 up to this many songs between its breaks. |
| **Talk after** | every set | 1 to 5 | Talk after every set, or only after every 2nd, 3rd and so on. Saves speech quota. The DJ keeps picking songs in the sets in between. |
| **Spoken breaks** | | | Shows how many breaks were voiced today. Listen sets no limit of its own; only Google's quota pauses speech (for an hour). |
| **Voice volume** | 0 dB | -12 to +12 dB | Loudness of the voice relative to the default level, which suits music with ReplayGain. |
| **Location** | empty | | Where the show airs, for example `Kiel`. The hosts mention it now and then. Empty means no home town. |
| **Pronunciation** | empty | | How the voice should say the place name if it gets it wrong. |
| **Language** | Same as the app | app languages | The language the hosts speak. Song titles stay in their own language. |
| **Details for the DJ** | empty | | Anything the hosts should know: your name, what you like, topics to avoid. |

### Models

Listen uses two Gemini models. They are chosen for the free tier:

- **Text model**, `gemini-3.5-flash-lite`: picks the songs and writes the lines.
- **Voice model**, one of:
  - **Flash-Lite TTS (fast)**, `gemini-3.8-flash-lite-tts`, the default.
  - **Flash TTS (best voice acting)**, `gemini-3.8-flash-tts`, with more expressive delivery.

The text model can also be changed in the `[settings.ai_dj]` table of the settings file (`text_model`). Use a model name that your key can access.

### Location and Pronunciation

Type a town or region into **Location**, for example `Kiel` or `Kiel, Germany`. The hosts will mention it occasionally. Text after a comma or bracket is only context; for **Pronunciation**, Listen uses the first part (the place name). If the voice says the name wrongly, write it the way it should sound in **Pronunciation**, for example `Keel`, and use **Preview** to test. The hint is passed to the voice model for lines that contain the place name.

### Details for the DJ

A line of free text, for example `Call me Sam. I love 90s R&B. No weather talk.` It is sent to Gemini with every break, so keep it short and do not put anything in it that you do not want to send to Google.

## Quotas and the text fallback

The free tier of the Gemini API has daily and per-minute limits, and the speech models have a small daily allowance. Listen sets no limit of its own: every break is voiced until Google says the quota is used up.

- **Spoken breaks** in the settings shows how many breaks were voiced today. A day is counted like Google counts it, resetting at midnight Pacific time.
- **Talk after** lets the DJ skip talking after some sets.
- The Preview counts as one spoken break the first time you play a persona with a model.
- When Google says the daily speech quota is used up, the DJ keeps picking songs and shows its lines as text on screen. It tries speech again after an hour. Saving a new key clears this state.
- If the limit is only per-minute, Listen waits briefly and tries once more.

If the text model fails or its limit is reached, similar songs fill in (as if the Auto-DJ were on) and a message tells you that the AI DJ is unavailable. Listen then waits before trying again, a little longer after each failure, up to an hour. A rejected key shows a warning that points you to **Settings → AI DJ**.

Playback never waits for the DJ: if the answer is late, the music simply continues.

If the player ever ends up idle while songs are still queued — a song that will not load, a break whose clip had nothing behind it — a check every five seconds starts the next queued song, so the music picks up again on its own instead of standing still until you press play.

## Privacy and data sent to Google

Listen has no server of its own. Requests go from your computer straight to Google under your own key.

What is sent:

- Song titles and artists the DJ chooses from, plus year and up to two genres where known.
- Short analysis summaries (tempo, key, energy) and a description of how songs were mixed.
- Which songs you skipped and which you queued yourself.
- Your requests from **Tell the DJ**.
- The DJ's own previous lines.
- Your **Persona**, **Language**, **Location**, **Pronunciation** and **Details for the DJ**.
- The lines to be spoken (to the voice model).
- Your API key, as a request header.

What is not sent: audio of your songs, your server address and sign-in, file names or paths, and the rest of your library.

Outside the EU, UK and Switzerland, Google may use free-tier requests to improve its products, and human reviewers may read them. With billing enabled on the key's project, requests are not used that way. Details are in Google's [Gemini API terms](https://ai.google.dev/gemini-api/terms), which require you to be 18 or older. The generated clips are stored briefly in Listen's cache folder and removed when Listen starts the next time.

The key is kept in the system keyring. The environment variable `LISTEN_GEMINI_API_KEY`, if set, takes precedence over the saved key, which is handy for testing.

## Troubleshooting

| Problem | What to do |
|---|---|
| Switching on says to add a key | Paste the key into **Gemini API key** and press **Save**. |
| "the Gemini API key was rejected" | The key is wrong, restricted or deleted. Create a new one and save it. |
| Status line says "Didn't work" with a quota message | You hit Google's limit. Wait, set **Talk after** to a higher number or try again tomorrow. |
| Lines appear as text and nothing is spoken | Google reported the daily speech quota as used up (Listen tries again after an hour), or you are casting over DLNA. |
| Songs repeat or feel off | Start the DJ from a song that fits the mood you want. The seed decides the mood. |
| "AI DJ unavailable; similar songs fill in" | A network or Google error. The Auto-DJ keeps the music going and the DJ retries later. |
| Place name sounds wrong | Fill **Pronunciation** and test with **Preview**. |
| The voice is too quiet or loud | Adjust **Voice volume**. |
| The DJ never mixes songs | Mixing needs analysed Navidrome or YouTube Music songs, see [DJ transitions](transitions.md). |
