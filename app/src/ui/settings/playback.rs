//! Playback tab: playback and EQ, visualizer, Auto-DJ, AI DJ, DJ transitions, lyrics.

use super::{line, section, stepper, switch, switch_with};
use crate::app::audio::{AudioMsg, CompParam, device_choices};
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{
    button, column, container, pick_list, row, slider, text_editor, text_input, toggler, vertical_slider,
};
use iced::{Alignment, Element, Padding};
use listen_core::settings::{self, ReplayGain, StreamQuality, TransitionEdges};

/// Audio sections of the Playback tab: playback, output, ReplayGain, mpv, EQ, compressor, transcoding.
pub(super) fn playback(state: &Listen) -> Vec<Element<'_, Message>> {
    // Separate sections, so the settings search can hide each one on its own.
    let mut v = vec![
        playback_section(state),
        output(state),
        replay_gain(state),
        mpv(state),
        equalizer(state),
        compressor(state),
    ];
    if state.transcoding_available() {
        v.push(transcoding(state));
    }
    v
}

fn audio_switch<'a>(value: bool, f: fn(bool) -> AudioMsg) -> Element<'a, Message> {
    toggler(value).on_toggle(move |v| Message::Audio(f(v))).size(20).into()
}

/// A slider with its value next to it, sending an [`AudioMsg`].
fn audio_slider<'a>(
    state: &Listen,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    step: f32,
    shown: String,
    f: impl Fn(f32) -> AudioMsg + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    row![
        slider(range, value, move |v| Message::Audio(f(v))).step(step).width(160),
        container(label(shown, metrics::font_md(), t.muted)).width(70),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .into()
}

fn small_button<'a>(state: &Listen, text: &'static str, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(
        text,
        metrics::font_sm(),
        if msg.is_some() { t.foreground } else { t.muted },
        Weight::Semibold,
    ))
    .padding(Padding::from([6.0, 10.0]))
    .style(style::default_button(t))
    .on_press_maybe(msg)
    .into()
}

fn playback_section(state: &Listen) -> Element<'_, Message> {
    let p = &state.config.settings.playback;
    let gapless = ["weak", "yes", "no"].map(String::from).to_vec();
    let mut rows = vec![
        line(
            state,
            "Gapless playback",
            "mpv gapless-audio (Feishin default: weak).",
            pick_list(gapless, Some(p.gapless.clone()), |v| Message::Setting(Setting::Gapless(v))).into(),
        ),
        line(
            state,
            "Streaming quality",
            "Lower quality saves bandwidth: Navidrome transcodes to MP3 up to the chosen bit rate, YouTube \
             Music picks a smaller audio format. Songs that are already downloaded stay original.",
            pick_list(StreamQuality::ALL, Some(p.stream_quality), |v| {
                Message::Setting(Setting::StreamQuality(v))
            })
            .into(),
        ),
        line(
            state,
            "Fade songs in and out",
            "Each song fades in at its start and out at its end (0 = off). The AI DJ turns the music \
             down and up at this speed too (at least 1 s).",
            stepper(
                state,
                0.0..=10.0,
                p.track_fade_secs,
                if p.track_fade_secs <= 0.0 { "off".into() } else { format!("{:.1} s", p.track_fade_secs) },
                |v| Setting::TrackFade((v * 2.0).round() / 2.0),
            ),
        ),
        line(
            state,
            "Fade on pause",
            "Fade out briefly when pausing and back in when resuming.",
            switch(p.fade_on_pause, Setting::FadeOnPause),
        ),
        line(
            state,
            "Cast to DLNA devices",
            "Look for speakers and TVs (DLNA / UPnP media renderers) on the network and show a cast button \
             in the player bar when some answer. Playing on one hands the queue over to it.",
            switch_with(p.dlna_enabled, |v| Message::Dlna(crate::app::dlna::DlnaMsg::SetEnabled(v))),
        ),
    ];
    // Only where a YouTube Music account and some other server can both exist.
    if state.config.servers.iter().any(|s| s.kind == listen_core::model::SourceKind::YouTubeMusic) {
        rows.push(line(
            state,
            "Prefer my own servers for YouTube Music songs",
            "When a YouTube Music song is also on your Navidrome, Jellyfin or local library, play that copy \
             (usually better quality). The song stays a YouTube Music song: likes, playlists, radio and \
             the music video work as before.",
            switch(p.prefer_own_servers, Setting::PreferOwnServers),
        ));
    }
    // The server's jukebox, only when it answered (Feishin: audio player type).
    if state.jukebox_offered() {
        rows.insert(
            0,
            line(
                state,
                "Audio player",
                "mpv plays on this computer. Jukebox lets the server play the queue through its own \
                 speakers; this computer then only controls it.",
                pick_list(settings::AudioPlayer::ALL, Some(p.audio_player), |v| {
                    Message::Radio(crate::app::radio::RadioMsg::SetAudioPlayer(v))
                })
                .into(),
            ),
        );
    }
    section(state, "Playback", rows)
}

/// Output device, pitch, sample rate, exclusive mode and format.
fn output(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let p = &state.config.settings.playback;
    let choices = device_choices(&state.audio.devices, &p.audio_device);
    let selected = choices.iter().find(|c| c.id == p.audio_device).cloned();
    let device = row![
        pick_list(choices, selected, |c| Message::Audio(AudioMsg::SetDevice(c.id))).width(260),
        small_button(
            state,
            if state.audio.devices_loading { "Loading…" } else { "Refresh" },
            Some(Message::Audio(AudioMsg::RefreshDevices))
        ),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let rate_value = state.audio.sample_rate_input.clone().unwrap_or_else(|| {
        if p.sample_rate_hz >= 8000 { p.sample_rate_hz.to_string() } else { String::new() }
    });
    let rate = row![
        text_input(crate::i18n::tr("48000"), &rate_value)
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .width(110)
            .on_input(|v| Message::Audio(AudioMsg::SampleRateInput(v)))
            .on_submit(Message::Audio(AudioMsg::SampleRateSubmit)),
        label("Hz", metrics::font_md(), t.muted),
        small_button(
            state,
            "Apply",
            state.audio.sample_rate_input.is_some().then_some(Message::Audio(AudioMsg::SampleRateSubmit))
        ),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let formats: Vec<String> = ["auto", "s16", "s32", "float", "u8"].map(String::from).to_vec();
    let format = if p.audio_format.trim().is_empty() { "auto".to_string() } else { p.audio_format.clone() };
    let mut rows = vec![
        line(
            state,
            "Audio device",
            "Where mpv plays. Also on the volume icon (right-click) and in the player config.",
            device.into(),
        ),
        line(
            state,
            "Preserve pitch",
            "Keep the pitch when the playback speed changes (mpv audio-pitch-correction). Off: the speed \
             also changes the pitch, with semitone steps in the player config.",
            audio_switch(p.preserve_pitch, AudioMsg::PreservePitch),
        ),
        line(
            state,
            "Microtonal pitch controls",
            "Adds 10 cent steps between the semitone buttons of the player config (when the pitch is not \
             preserved).",
            audio_switch(p.microtonal_pitch, AudioMsg::Microtonal),
        ),
        line(
            state,
            "Sample rate",
            "Output sample rate in Hz; below 8000 (or empty) means the device's default. Press Enter to apply.",
            rate.into(),
        ),
        line(
            state,
            "Exclusive mode",
            "Take the sound device for Listen alone (mpv audio-exclusive; WASAPI and some ALSA setups).",
            audio_switch(p.audio_exclusive, AudioMsg::Exclusive),
        ),
        line(
            state,
            "Audio format",
            "Sample format sent to the sound card (mpv audio-format); auto lets mpv choose.",
            pick_list(formats, Some(format), |f| {
                Message::Audio(AudioMsg::AudioFormat(if f == "auto" { String::new() } else { f }))
            })
            .into(),
        ),
    ];
    if let Some(e) = &state.audio.devices_error {
        rows.insert(1, label(format!("Could not list the devices: {e}"), metrics::font_sm(), t.error).into());
    }
    section(state, "Audio output", rows)
}

fn replay_gain(state: &Listen) -> Element<'_, Message> {
    let p = &state.config.settings.playback;
    let rg = [ReplayGain::No, ReplayGain::Track, ReplayGain::Album];
    section(
        state,
        "ReplayGain",
        vec![
            line(
                state,
                "Mode",
                "Normalize loudness with ReplayGain tags.",
                pick_list(rg, Some(p.replay_gain), |v| Message::Setting(Setting::ReplayGain(v))).into(),
            ),
            line(
                state,
                "Preamp",
                "Gain applied on top of the ReplayGain value (dB).",
                audio_slider(
                    state,
                    -15.0..=15.0,
                    p.replay_gain_preamp_db,
                    0.5,
                    format!("{:+.1} dB", p.replay_gain_preamp_db),
                    AudioMsg::ReplayGainPreamp,
                ),
            ),
            line(
                state,
                "Prevent clipping",
                "Lower the gain so that the peak never exceeds full scale.",
                audio_switch(p.replay_gain_clip, AudioMsg::ReplayGainClip),
            ),
            line(
                state,
                "Fallback gain",
                "Gain for songs without ReplayGain tags (dB).",
                audio_slider(
                    state,
                    -15.0..=15.0,
                    p.replay_gain_fallback_db,
                    0.5,
                    format!("{:+.1} dB", p.replay_gain_fallback_db),
                    AudioMsg::ReplayGainFallback,
                ),
            ),
        ],
    )
}

fn mpv(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let p = &state.config.settings.playback;
    let path = row![
        text_input(crate::i18n::tr("mpv"), &p.mpv_path)
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .width(260)
            .on_input(|v| Message::Audio(AudioMsg::MpvPath(v))),
        small_button(
            state,
            "Clear",
            (!p.mpv_path.is_empty()).then_some(Message::Audio(AudioMsg::MpvPath(String::new())))
        ),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let args = text_editor(&state.audio.args)
        .placeholder("--gapless-audio=weak\n--prefetch-playlist=yes")
        .on_action(|a| Message::Audio(AudioMsg::MpvArgs(a)))
        .size(metrics::font_md())
        .height(110)
        .width(300);
    section(
        state,
        "mpv",
        vec![
            line(
                state,
                "Executable path",
                "The mpv binary to run; empty uses `mpv` from the PATH. Reload mpv to apply.",
                path.into(),
            ),
            line(
                state,
                "Extra parameters",
                "One per line, e.g. --audio-channels=stereo. See https://mpv.io/manual/stable/#audio. \
                 Reload mpv to apply.",
                args.into(),
            ),
            line(
                state,
                "Reload mpv",
                "Stops playback and starts mpv again with the path and parameters above.",
                small_button(state, "Reload", Some(Message::Audio(AudioMsg::ReloadMpv))),
            ),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn preset_buttons<'a>(
    state: &Listen,
    names: Vec<String>,
    current: &str,
    name_input: &str,
    on_pick: fn(String) -> AudioMsg,
    on_name: fn(String) -> AudioMsg,
    on_save: AudioMsg,
    on_delete: Option<AudioMsg>,
    reset: Option<AudioMsg>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let selected = (!current.is_empty()).then(|| current.to_string());
    let mut r = row![
        pick_list(names, selected, move |n| Message::Audio(on_pick(n))).placeholder("Preset").width(160)
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    if let Some(reset) = reset {
        r = r.push(small_button(state, "Reset", Some(Message::Audio(reset))));
    }
    if let Some(del) = on_delete {
        r = r.push(small_button(state, "Delete", Some(Message::Audio(del))));
    }
    let save = row![
        text_input(crate::i18n::tr("Save current as…"), name_input)
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .width(160)
            .on_input(move |v| Message::Audio(on_name(v)))
            .on_submit(Message::Audio(on_save.clone())),
        small_button(state, "Save", (!name_input.trim().is_empty()).then_some(Message::Audio(on_save))),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    column![r, save].spacing(metrics::space_sm()).into()
}

fn equalizer(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let p = &state.config.settings.playback;
    let mut eq = row![].spacing(metrics::space_md()).align_y(Alignment::End);
    for (i, band) in p.equalizer.iter().enumerate() {
        let freq =
            if band.freq >= 1000.0 { format!("{}k", band.freq / 1000.0) } else { format!("{}", band.freq) };
        eq = eq.push(
            column![
                label(format!("{:+.1}", band.gain), metrics::font_xs(), t.muted),
                vertical_slider(-12.0..=12.0, band.gain, move |g| Message::Setting(Setting::EqBand(i, g)))
                    .step(0.5_f32)
                    .height(120),
                label(freq, metrics::font_xs(), t.muted),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        );
    }
    let mut names: Vec<String> = settings::EQ_PRESETS.iter().map(|(n, _)| (*n).to_string()).collect();
    names.extend(p.custom_eq_presets.keys().filter(|k| !names.contains(k)).cloned().collect::<Vec<_>>());
    let custom = p.custom_eq_presets.contains_key(&p.equalizer_preset);
    let rows = vec![
        line(
            state,
            "Equalizer",
            "12-band EQ applied by mpv.",
            switch(p.equalizer_enabled, Setting::EqEnabled),
        ),
        line(
            state,
            "Presets",
            "Built-in presets, and your own: set the bands, type a name and save.",
            preset_buttons(
                state,
                names,
                &p.equalizer_preset,
                &state.audio.eq_preset_name,
                AudioMsg::EqPreset,
                AudioMsg::EqPresetName,
                AudioMsg::EqSavePreset,
                custom.then_some(AudioMsg::EqDeletePreset),
                Some(AudioMsg::EqReset),
            ),
        ),
        row![
            column![
                label(format!("Pre {:+.0}", p.equalizer_preamp), metrics::font_xs(), t.muted),
                vertical_slider(-12.0..=12.0, p.equalizer_preamp, |g| Message::Setting(Setting::EqPreamp(g)))
                    .step(0.5_f32)
                    .height(120),
                label("dB", metrics::font_xs(), t.muted),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
            eq,
        ]
        .spacing(metrics::space_xl())
        .into(),
    ];
    section(state, "Equalizer", rows)
}

fn compressor(state: &Listen) -> Element<'_, Message> {
    let p = &state.config.settings.playback;
    let c = &p.compressor;
    let mut names: Vec<String> = settings::COMPRESSOR_PRESETS.iter().map(|(n, _)| (*n).to_string()).collect();
    names.extend(
        p.custom_compressor_presets.keys().filter(|k| !names.contains(k)).cloned().collect::<Vec<_>>(),
    );
    let custom = p.custom_compressor_presets.contains_key(&p.compressor_preset);
    let param = |name: &'static str,
                 hint: &'static str,
                 which: CompParam,
                 range: std::ops::RangeInclusive<f32>,
                 step: f32,
                 value: f32,
                 unit: &'static str| {
        line(
            state,
            name,
            hint,
            audio_slider(state, range, value, step, format!("{value:.1} {unit}"), move |v| {
                AudioMsg::CompParam(which, v)
            }),
        )
    };
    section(
        state,
        "Compressor",
        vec![
            line(
                state,
                "Compressor",
                "Evens out loud and quiet parts (ffmpeg acompressor in mpv's filter chain).",
                audio_switch(c.enabled, AudioMsg::CompEnabled),
            ),
            line(
                state,
                "Presets",
                "Built-in presets, and your own.",
                preset_buttons(
                    state,
                    names,
                    &p.compressor_preset,
                    &state.audio.comp_preset_name,
                    AudioMsg::CompPreset,
                    AudioMsg::CompPresetName,
                    AudioMsg::CompSavePreset,
                    custom.then_some(AudioMsg::CompDeletePreset),
                    None,
                ),
            ),
            param(
                "Threshold",
                "Level above which the compression starts.",
                CompParam::Threshold,
                -60.0..=0.0,
                1.0,
                c.params.threshold,
                "dB",
            ),
            param(
                "Ratio",
                "How strongly loud parts are reduced (n:1).",
                CompParam::Ratio,
                1.0..=20.0,
                0.5,
                c.params.ratio,
                ":1",
            ),
            param(
                "Attack",
                "How fast the compressor reacts.",
                CompParam::Attack,
                0.1..=2000.0,
                1.0,
                c.params.attack,
                "ms",
            ),
            param(
                "Release",
                "How fast it lets go again.",
                CompParam::Release,
                1.0..=9000.0,
                10.0,
                c.params.release,
                "ms",
            ),
            param(
                "Makeup gain",
                "Gain added after compressing.",
                CompParam::Makeup,
                0.0..=30.0,
                0.5,
                c.params.makeup,
                "dB",
            ),
            param(
                "Knee",
                "Width of the soft knee around the threshold.",
                CompParam::Knee,
                1.0..=8.0,
                0.5,
                c.params.knee,
                "dB",
            ),
        ],
    )
}

fn transcoding(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let tc = &state.config.settings.playback.transcode;
    let number = |value: u32, placeholder: &'static str, f: fn(String) -> AudioMsg| {
        text_input(crate::i18n::tr(placeholder), &if value > 0 { value.to_string() } else { String::new() })
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .width(110)
            .on_input(move |v| Message::Audio(f(v)))
    };
    let mut rows = vec![line(
        state,
        "Transcode",
        "Ask the server to convert songs while streaming (Navidrome, Jellyfin). Takes the place of \
             \"Streaming quality\" for those servers; YouTube Music keeps that setting. Applies to the \
             songs that start from now on.",
        audio_switch(tc.enabled, AudioMsg::TranscodeEnabled),
    )];
    if tc.enabled {
        rows.push(line(
            state,
            "Format",
            "mp3, opus, flac, ... Empty: the server's choice (Jellyfin: mp3).",
            text_input(crate::i18n::tr("mp3, opus, flac"), &tc.format)
                .size(metrics::font_md())
                .padding(Padding::from([6.0, 8.0]))
                .style(style::input(t))
                .width(160)
                .on_input(|v| Message::Audio(AudioMsg::TranscodeFormat(v)))
                .into(),
        ));
        rows.push(line(
            state,
            "Maximum bit rate",
            "In kbit/s; 0 or empty lets the server pick.",
            number(tc.bitrate, "0", AudioMsg::TranscodeBitrate).into(),
        ));
        rows.push(line(
            state,
            "Maximum sample rate",
            "In Hz (Jellyfin only); empty = no limit.",
            number(tc.max_sample_rate, "192000", AudioMsg::TranscodeSampleRate).into(),
        ));
    }
    section(state, "Transcoding", rows)
}

pub(super) fn visualizer(state: &Listen) -> Element<'_, Message> {
    let _t = state.tokens;
    let s = &state.config.settings;
    section(
        state,
        "Visualizer",
        vec![
            line(
                state,
                "In the queue panel",
                "Show the spectrum under the queue, like Feishin's side panel (captures only Listen's own audio).",
                switch(s.visualizer.in_queue_panel, Setting::QueueVisualizer),
            ),
            line(
                state,
                "Visualizer settings",
                "Mode, gradients, channel layout, frequency scale, peaks, reflex, radial, presets and more (audioMotion options).",
                iced::widget::button(crate::ui::label(
                    "Configure…",
                    crate::style::metrics::font_md(),
                    _t.foreground,
                ))
                .padding(iced::Padding::from([6.0, 14.0]))
                .style(crate::style::default_button(_t))
                .on_press(Message::Viz(crate::app::viz::VizMsg::Open))
                .into(),
            ),
        ],
    )
}

pub(super) fn auto_dj(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings;
    let dj = &s.auto_dj;
    section(
        state,
        "Auto-DJ",
        vec![
            line(
                state,
                "Enabled",
                "Keep the queue going with similar songs.",
                switch(dj.enabled, Setting::AutoDj),
            ),
            line(
                state,
                "Songs to add",
                "Added whenever the queue runs out.",
                row![
                    slider(1.0..=20.0, dj.item_count as f32, |v| Message::Setting(Setting::AutoDjCount(
                        v as u32
                    )))
                    .width(160),
                    label(dj.item_count.to_string(), metrics::font_md(), t.muted),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center)
                .into(),
            ),
        ],
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PersonaChoice(&'static listen_dj::Persona);

impl std::fmt::Display for PersonaChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0.cohost {
            Some(co) => write!(f, "{} ({} & {})", self.0.name, self.0.host, co.name),
            None => write!(f, "{} ({})", self.0.name, self.0.voice),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TtsChoice(&'static str);

impl std::fmt::Display for TtsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            "gemini-3.8-flash-tts" => "Flash TTS (best voice acting)",
            _ => "Flash-Lite TTS (fast)",
        })
    }
}

pub(super) fn ai_dj(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let dj = &state.config.settings.ai_dj;
    let persona = state.dj_persona();
    let key_hint = if state.dj.has_key() {
        "Saved in the system keyring. Paste a new one to replace it, or save an empty field to remove it."
    } else {
        "Free key from aistudio.google.com → Get API key (no billing needed). Stored in the system keyring."
    };
    let mut rows = vec![
        line(
            state,
            "Enabled",
            "Picks sets of songs that fit the mood of the song you started it on, and talks between \
             them like a radio host. Replaces the Auto-DJ while on.",
            switch(dj.enabled, Setting::AiDj),
        ),
        line(
            state,
            "Gemini API key",
            key_hint,
            row![
                text_input(if state.dj.has_key() { "••••••••" } else { "AIza… / AQ.…" }, &state.dj.key_input)
                    .secure(true)
                    .on_input(|v| Message::Setting(Setting::AiDjKeyInput(v)))
                    .on_submit(Message::Setting(Setting::AiDjSaveKey))
                    .style(style::input(t))
                    .width(220),
                button(label("Save", metrics::font_md(), t.foreground))
                    .style(style::default_button(t))
                    .on_press(Message::Setting(Setting::AiDjSaveKey)),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        ),
        line(
            state,
            "Persona",
            persona.description,
            row![
                pick_list(
                    listen_dj::PERSONAS.iter().map(PersonaChoice).collect::<Vec<_>>(),
                    Some(PersonaChoice(persona)),
                    |c| Message::Setting(Setting::AiDjPersona(c.0.id.into())),
                ),
                button(label(
                    if state.dj.previewing { "Playing…" } else { "Preview" },
                    metrics::font_md(),
                    t.foreground
                ))
                .style(style::default_button(t))
                .on_press_maybe(
                    (state.dj.has_key() && !state.dj.previewing)
                        .then_some(Message::Setting(Setting::AiDjPreview))
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        ),
        line(
            state,
            "Voice model",
            "Both are on Gemini's free tier; the free speech quota per day is small.",
            pick_list(
                listen_dj::TTS_MODELS.iter().map(|m| TtsChoice(m)).collect::<Vec<_>>(),
                listen_dj::TTS_MODELS.iter().find(|m| **m == dj.tts_model).map(|m| TtsChoice(m)),
                |c| Message::Setting(Setting::AiDjTtsModel(c.0.into())),
            )
            .into(),
        ),
        line(
            state,
            "Songs per set",
            "The DJ picks 2 up to this many songs between its breaks.",
            stepper(state, 2.0..=8.0, dj.set_max as f32, format!("2–{}", dj.set_max), |v| {
                Setting::AiDjSetMax(v as u32)
            }),
        ),
        line(
            state,
            "Talk after",
            "Every set, or only every 2nd/3rd… (saves speech quota).",
            stepper(
                state,
                1.0..=5.0,
                dj.talk_every as f32,
                if dj.talk_every == 1 { "every set".into() } else { format!("every {}.", dj.talk_every) },
                |v| Setting::AiDjTalkEvery(v as u32),
            ),
        ),
        line(state, "Spoken breaks", state.dj_quota_line(), iced::widget::Space::new().into()),
        line(
            state,
            "Voice volume",
            "Relative to the music.",
            stepper(state, -12.0..=12.0, dj.voice_gain_db, format!("{:+.0} dB", dj.voice_gain_db), |v| {
                Setting::AiDjVoiceGain(v.round())
            }),
        ),
        line(
            state,
            "Location",
            "Where the show airs, e.g. your town. The hosts mention it now and then. Empty = no home town.",
            text_input(crate::i18n::tr("e.g. Kiel"), &dj.location)
                .on_input(|v| Message::Setting(Setting::AiDjLocation(v)))
                .style(style::input(t))
                .width(220)
                .into(),
        ),
        line(
            state,
            "Pronunciation",
            "How the voice should say the place name if it gets it wrong.",
            text_input(crate::i18n::tr("Optional"), &dj.location_pronunciation)
                .on_input(|v| Message::Setting(Setting::AiDjPronunciation(v)))
                .style(style::input(t))
                .width(220)
                .into(),
        ),
        line(
            state,
            "Language",
            "The language the hosts speak. Song titles stay in their own language.",
            pick_list(dj_languages(), dj_languages().into_iter().find(|l| l.0 == dj.language), |l| {
                Message::Setting(Setting::AiDjLanguage(l.0.to_string()))
            })
            .into(),
        ),
        line(
            state,
            "Details for the DJ",
            "Anything the hosts should know: your name, what you like, topics to avoid. Sent to Gemini with every break.",
            text_input(crate::i18n::tr("e.g. Call me Sam. I love 90s R&B. No weather talk."), &dj.notes)
                .on_input(|v| Message::Setting(Setting::AiDjNotes(v)))
                .style(style::input(t))
                .width(320)
                .into(),
        ),
    ];
    rows.push(
        crate::ui::t_text(
            "Sent to Google: song titles, artists, the DJ's lines and your details for the DJ. Outside the EU, UK \
             and Switzerland, Google may use free-tier requests to improve its models. Nothing else leaves your computer.",
        )
        .font(crate::appearance::current_font())
        .size(metrics::font_sm())
        .color(t.muted)
        .into(),
    );
    if let Some(status) = &state.dj.status {
        rows.push(label(status.clone(), metrics::font_md(), t.primary).into());
    }
    section(state, "AI DJ", rows)
}

/// Song analysis (tempo, key, sections) and the DJ transitions built on it.
pub(super) fn dj_transitions(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let a = &state.config.settings.analysis;
    let m = &state.config.settings.mixing;
    let status = match &state.analysis_status {
        _ if !a.enabled => "Off.".to_string(),
        None => "Starting…".to_string(),
        Some(st) => {
            let mut parts = vec![format!("{} songs analysed", st.songs)];
            if st.machines.len() > 1 {
                let per: Vec<String> = st.machines.iter().map(|(name, n)| format!("{name}: {n}")).collect();
                parts.push(format!("({})", per.join(", ")));
            }
            if let Some((n, done)) = st.library {
                parts.push(format!("· library {n} songs{}", if done { "" } else { " (listing…)" }));
            }
            if st.queued > 0 {
                parts.push(format!("· {} waiting", st.queued));
            }
            if let Some(why) = st.waiting {
                parts.push(format!("({why})"));
            }
            let mut line = parts.join(" ");
            if let Some(cur) = &st.current {
                line.push_str(&format!("\nNow: {cur}"));
            }
            if let Some(e) = &st.error {
                line.push_str(&format!("\n{e}"));
            }
            line.push_str(&format!("\nBeats: {} · this machine: {}", st.beats, st.machine));
            // Its fetches are budgeted, so it's never crawled.
            if state.config.servers.iter().any(|s| s.kind == listen_core::model::SourceKind::YouTubeMusic) {
                line.push_str(if state.config.settings.analysis.youtube_music {
                    "\nYouTube Music: analysed on demand (queue and DJ candidates)"
                } else {
                    "\nYouTube Music: not analysed"
                });
            }
            line
        }
    };
    let detected = state.analysis_status.as_ref().map(|s| s.dir.display().to_string()).unwrap_or_default();
    let dir_value = state.analysis_dir_input.clone().unwrap_or_else(|| a.shared_dir.clone());
    let dir = row![
        text_input(if detected.is_empty() { "Listen's data folder" } else { &detected }, &dir_value)
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .width(320)
            .on_input(|v| Message::Setting(Setting::AnalysisDirInput(v)))
            .on_submit(Message::Setting(Setting::AnalysisDirSave)),
        button(label_w("Save", metrics::font_sm(), t.foreground, Weight::Semibold))
            .padding(Padding::from([6.0, 10.0]))
            .style(style::default_button(t))
            .on_press_maybe(
                state.analysis_dir_input.is_some().then_some(Message::Setting(Setting::AnalysisDirSave))
            ),
        button(label_w("Choose…", metrics::font_sm(), t.foreground, Weight::Semibold))
            .padding(Padding::from([6.0, 10.0]))
            .style(style::default_button(t))
            .on_press(Message::Setting(Setting::AnalysisDirPick)),
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center);
    let mut rows = vec![
        line(
            state,
            "Analyse songs",
            "Tempo, beats, key, energy and song sections of Navidrome songs, for the DJs.",
            switch(a.enabled, Setting::Analysis),
        ),
        line(
            state,
            "YouTube Music songs",
            "Also analyse YouTube Music songs so they can be mixed. Listen fetches a temporary copy of each song \
             to measure it and keeps only the results. Off by default; asks for an analysis folder first.",
            switch(a.youtube_music && !a.shared_dir.trim().is_empty(), Setting::AnalysisYtm),
        ),
        container(label(status, metrics::font_sm(), t.muted)).into(),
        line(
            state,
            "Whole library",
            "Work through the library in the background (the queue always comes first).",
            switch(a.analyze_library, Setting::AnalysisLibrary),
        ),
        line(
            state,
            "Also on battery",
            "Keep analysing the library when unplugged.",
            switch(a.on_battery, Setting::AnalysisOnBattery),
        ),
        line(
            state,
            "Analysis folder",
            "Where the results are kept. A synced folder lets your other computers reuse them.",
            dir.into(),
        ),
        line(
            state,
            "DJ transitions",
            "Mix songs the Auto-DJ or AI DJ picked: beat-matched blends, or fades when songs don't fit.",
            switch(m.enabled, Setting::Mixing),
        ),
        line(
            state,
            "Order by flow",
            "Pick songs whose tempo, key and energy follow on from the current one.",
            switch(m.flow_select, Setting::MixFlow),
        ),
        line(
            state,
            "Largest tempo change",
            "Beyond this the songs fade instead of being beat-matched.",
            stepper(state, 2.0..=8.0, m.max_tempo_pct, format!("{:.0} %", m.max_tempo_pct), |v| {
                Setting::MixMaxTempo(v.round())
            }),
        ),
        line(
            state,
            "Long blends",
            "Length of a blend over instrumental intros/outros.",
            stepper(
                state,
                8.0..=16.0,
                f32::from(m.long_blend_bars),
                format!("{} bars", m.long_blend_bars),
                |v| Setting::MixLongBars(if v >= 12.0 { 16 } else { 8 }),
            ),
        ),
        line(
            state,
            "Transition edges",
            "Whether songs with vocals at the end or start are blended. Smart allows it for club genres such \
             as garage, house, techno and drum & bass; Instrumental only needs instrumental edges.",
            pick_list(TransitionEdges::ALL.map(EdgeChoice), Some(EdgeChoice(m.transition_edges)), |c| {
                Message::Setting(Setting::MixEdges(c.0))
            })
            .into(),
        ),
        line(
            state,
            "Tempo ramps",
            "Glide the tempo between songs instead of jumping.",
            switch(m.tempo_ramps, Setting::MixTempoRamps),
        ),
        line(
            state,
            "Bass swap",
            "Swap the basslines on a downbeat so only one plays at a time.",
            switch(m.bass_swap, Setting::MixBassSwap),
        ),
        line(
            state,
            "Mix queued songs",
            "Also mix into songs added with \"Add to queue\" while a DJ is on.",
            switch(m.mix_manual_queue, Setting::MixManualQueue),
        ),
    ];
    if state.demo {
        rows.remove(1);
    }
    section(state, "DJ transitions & song analysis", rows)
}
pub(super) fn lyrics(state: &Listen) -> Element<'_, Message> {
    let fetch = switch(state.config.settings.lyrics.fetch, Setting::LyricsFetch);
    let rows = crate::ui::lyrics_settings::rows(
        state,
        &|name, hint, control| line(state, name, hint, control),
        fetch,
    );
    section(state, "Lyrics", rows)
}

/// An entry of the transition edges picker, shown translated.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EdgeChoice(TransitionEdges);

impl std::fmt::Display for EdgeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::i18n::tr;
        f.write_str(match self.0 {
            TransitionEdges::Always => tr("Always"),
            TransitionEdges::Instrumental => tr("Instrumental only"),
            TransitionEdges::Smart => tr("Smart"),
        })
    }
}

/// A language in the AI DJ's picker; an empty code follows the app's language.
#[derive(Debug, Clone, PartialEq)]
struct DjLanguage(&'static str, String);

impl std::fmt::Display for DjLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

fn dj_languages() -> Vec<DjLanguage> {
    let mut v = vec![DjLanguage("", crate::i18n::tr("Same as the app").to_string())];
    v.extend(crate::i18n::LANGUAGES.iter().map(|(c, n)| DjLanguage(c, (*n).to_string())));
    v
}
