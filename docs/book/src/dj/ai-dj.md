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

The break is spoken with a voice model and placed at random where it fits:

- **between songs**, in the silence after the current song;
- **over the end** of the current song, which is turned down while the host talks;
- **over the start** of the first song of the new set;
- **across the change**, starting over the end of one song and finishing over the start of the next.

The music is turned down while the voice speaks and back up afterwards. Breaks are short, at most 10 to 20 seconds of speech.

### How it uses the analysis

If songs are analysed, the DJ knows more than titles. Each candidate is described with tempo, key and energy, and the DJ is told how the mix engine would bring it in after the current song. The candidate list is ordered by how well the songs flow from the current one, so the DJ is offered songs that mix well. The DJ can also mention its mixes, and it keeps a talk-over shorter than the instrumental intro of the next song so that it never talks over a singer.

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
| **Spoken breaks per day** | 15 | 0 to 50 | The most breaks Listen asks to be spoken per day. After that the lines are shown as text. |
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

The text model can also be changed in the `[ai_dj]` section of the settings file (`text_model`). Use a model name that your key can access.

### Location and Pronunciation

Type a town or region into **Location**, for example `Kiel` or `Kiel, Germany`. The hosts will mention it occasionally. Text after a comma or bracket is only context; for **Pronunciation**, Listen uses the first part (the place name). If the voice says the name wrongly, write it the way it should sound in **Pronunciation**, for example `Keel`, and use **Preview** to test. The hint is passed to the voice model for lines that contain the place name.

### Details for the DJ

A line of free text, for example `Call me Sam. I love 90s R&B. No weather talk.` It is sent to Gemini with every break, so keep it short and do not put anything in it that you do not want to send to Google.

## Quotas and the text fallback

The free tier of the Gemini API has daily and per-minute limits, and the speech models have a small daily allowance. Listen is built to stay inside it:

- **Spoken breaks per day** limits how many breaks are voiced. The line under the option shows how many are left today. A day is counted like Google counts it, resetting at midnight Pacific time.
- **Talk after** lets the DJ skip talking after some sets.
- The Preview counts as one spoken break the first time you play a persona with a model.
- When Google says the daily speech quota is used up, the DJ keeps picking songs and shows its lines as text on screen. It tries speech again after an hour. Saving a new key clears this state.
- If the limit is only per-minute, Listen waits briefly and tries once more.

If the text model fails or its limit is reached, similar songs fill in (as if the Auto-DJ were on) and a message tells you that the AI DJ is unavailable. Listen then waits before trying again, a little longer after each failure, up to an hour. A rejected key shows a warning that points you to **Settings → AI DJ**.

Playback never waits for the DJ: if the answer is late, the music simply continues.

## Privacy and data sent to Google

Listen has no server of its own. Requests go from your computer straight to Google under your own key.

What is sent:

- Song titles and artists the DJ chooses from, plus year and up to two genres where known.
- Short analysis summaries (tempo, key, energy) and a description of how songs were mixed.
- Which songs you skipped and which you queued yourself.
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
| Status line says "Didn't work" with a quota message | You hit Google's limit. Wait, lower **Spoken breaks per day**, set **Talk after** to a higher number or try again tomorrow. |
| Lines appear as text and nothing is spoken | The daily speech quota or **Spoken breaks per day** is used up, or you are casting over DLNA. |
| Songs repeat or feel off | Start the DJ from a song that fits the mood you want. The seed decides the mood. |
| "AI DJ unavailable; similar songs fill in" | A network or Google error. The Auto-DJ keeps the music going and the DJ retries later. |
| Place name sounds wrong | Fill **Pronunciation** and test with **Preview**. |
| The voice is too quiet or loud | Adjust **Voice volume**. |
| The DJ never mixes songs | Mixing needs analysed Navidrome or YouTube Music songs, see [DJ transitions](transitions.md). |
