//! Audio output and the player-config popover: output device, playback speed and pitch, mpv start-up
//! options, ReplayGain, sample rate, exclusive mode, equalizer and compressor presets, transcoding.
//!
//! DJ transitions own mpv `speed` while one is possible; the user's speed then rests at 1.0
//! (see [`Listen::effective_speed`]).

use super::{Listen, Message, err};
use crate::playback::PlayerConfig;
use iced::widget::text_editor;
use iced::{Point, Task};
use listen_core::settings::{self, Playback};

/// What the user is looking at in the audio popups.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Popup {
    /// The player-config popover (sliders button).
    Config,
    /// The output-device menu of the volume button, opened at the cursor.
    Devices(Point),
}

/// A compressor parameter (slider).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompParam {
    Threshold,
    Ratio,
    Attack,
    Release,
    Makeup,
    Knee,
}

#[derive(Debug, Clone)]
pub enum AudioMsg {
    TogglePopover,
    /// Closes whichever popup is open.
    ClosePopup,
    /// Right-click on the volume icon (the cursor position was published just before).
    OpenDeviceMenu,
    RefreshDevices,
    Devices(Result<Vec<(String, String)>, String>),
    SetDevice(String),
    SetSpeed(f32),
    ResetSpeed,
    /// Pitch in semitones (the speed changes with it when the pitch is not preserved).
    PitchStep(f32),
    PreservePitch(bool),
    Microtonal(bool),
    ReloadMpv,
    /// Closes the popover and opens the settings page.
    OpenSettings,
    MpvPath(String),
    MpvArgs(text_editor::Action),
    ReplayGainPreamp(f32),
    ReplayGainClip(bool),
    ReplayGainFallback(f32),
    Exclusive(bool),
    /// Typing the sample rate; `SampleRateSubmit` applies it.
    SampleRateInput(String),
    SampleRateSubmit,
    AudioFormat(String),
    EqPreset(String),
    EqReset,
    EqPresetName(String),
    EqSavePreset,
    EqDeletePreset,
    CompEnabled(bool),
    CompParam(CompParam, f32),
    CompPreset(String),
    CompPresetName(String),
    CompSavePreset,
    CompDeletePreset,
    TranscodeEnabled(bool),
    TranscodeFormat(String),
    TranscodeBitrate(String),
    TranscodeSampleRate(String),
}

/// UI state of the audio features.
pub struct AudioState {
    pub popup: Option<Popup>,
    /// `(mpv name, description)` of the output devices; "auto" is the system default.
    pub devices: Vec<(String, String)>,
    pub devices_loading: bool,
    pub devices_error: Option<String>,
    /// The user's playback speed (1.0 = normal).
    pub speed: f32,
    /// The "mpv extra parameters" text box.
    pub args: text_editor::Content,
    pub sample_rate_input: Option<String>,
    pub eq_preset_name: String,
    pub comp_preset_name: String,
}

impl std::fmt::Debug for AudioState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioState").field("popup", &self.popup).field("speed", &self.speed).finish()
    }
}

impl AudioState {
    pub fn new(p: &Playback) -> Self {
        Self {
            popup: None,
            devices: Vec::new(),
            devices_loading: false,
            devices_error: None,
            speed: 1.0,
            args: text_editor::Content::with_text(&p.mpv_extra_args.join("\n")),
            sample_rate_input: None,
            eq_preset_name: String::new(),
            comp_preset_name: String::new(),
        }
    }
}

pub const SPEED_MIN: f32 = 0.5;
pub const SPEED_MAX: f32 = 2.0;

/// Pitch (semitones) of a speed that is not pitch-corrected.
pub fn speed_to_pitch(speed: f32) -> f32 {
    12.0 * speed.log2()
}

pub fn pitch_to_speed(pitch: f32) -> f32 {
    2f32.powf(pitch / 12.0)
}

/// The speed after moving the pitch by `semitones`, clamped to the speed range.
pub fn step_speed(speed: f32, semitones: f32) -> f32 {
    pitch_to_speed(speed_to_pitch(speed) + semitones).clamp(SPEED_MIN, SPEED_MAX)
}

/// The label of an output device: mpv's description; `auto` is "System default".
pub fn device_label(devices: &[(String, String)], id: &str) -> String {
    if id == "auto" || id.is_empty() {
        return "System default".into();
    }
    devices.iter().find(|(name, _)| name == id).map_or_else(|| id.to_string(), |(_, d)| d.clone())
}

/// The choices of a device picker: "System default" first, then mpv's list
/// (without its own `auto` entry); the selected device stays listed when it is unplugged.
pub fn device_choices(devices: &[(String, String)], selected: &str) -> Vec<DeviceChoice> {
    let mut out = vec![DeviceChoice { id: "auto".into(), label: "System default".into() }];
    out.extend(
        devices
            .iter()
            .filter(|(name, _)| name != "auto")
            .map(|(name, desc)| DeviceChoice { id: name.clone(), label: desc.clone() }),
    );
    if !out.iter().any(|c| c.id == selected) && !selected.is_empty() {
        out.push(DeviceChoice { id: selected.into(), label: format!("{selected} (not found)") });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceChoice {
    pub id: String,
    pub label: String,
}

impl std::fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Demo devices (no mpv in demo mode).
fn demo_devices() -> Vec<(String, String)> {
    vec![
        ("auto".into(), "Autoselect device".into()),
        ("pipewire/alsa_output.pci-0000_00_1f.3.analog-stereo".into(), "Built-in Audio Analog Stereo".into()),
        ("pipewire/bluez_output.headphones".into(), "WH-1000XM5".into()),
    ]
}

impl Listen {
    /// mpv properties for `deck` from the settings.
    pub(super) fn audio_settings_for(&self, deck: usize) -> listen_player::AudioSettings {
        let s = &self.config.settings.playback;
        listen_player::AudioSettings {
            gapless: s.gapless.clone(),
            replay_gain: match s.replay_gain {
                settings::ReplayGain::No => "no",
                settings::ReplayGain::Track => "track",
                settings::ReplayGain::Album => "album",
            }
            .into(),
            replay_gain_preamp_db: s.replay_gain_preamp_db,
            replay_gain_clip: s.replay_gain_clip,
            replay_gain_fallback_db: s.replay_gain_fallback_db,
            audio_device: s.audio_device.clone(),
            audio_filters: if self.mixing_possible() || deck == 1 {
                settings::deck_audio_filters(s)
            } else {
                settings::mpv_audio_filters(s)
            },
            pitch_correction: s.preserve_pitch,
            audio_exclusive: s.audio_exclusive,
            audio_samplerate: s.sample_rate_hz,
            audio_format: s.audio_format.clone(),
        }
    }

    /// The speed the decks should run at. DJ transitions own `speed` while
    /// they are possible, so then the user's speed rests at 1.0.
    pub fn effective_speed(&self) -> f32 {
        if self.speed_locked() { 1.0 } else { self.audio.speed }
    }

    /// The speed control is disabled: a DJ transition may use the decks' speed.
    pub fn speed_locked(&self) -> bool {
        self.mixing_possible()
    }

    /// Sends the effective speed to the decks; never touches them during a blend.
    pub(super) fn apply_speed(&self) -> Task<Message> {
        if self.mix.phase != super::mix::Phase::Idle {
            return Task::none();
        }
        if self.speed_locked() && (self.audio.speed - 1.0).abs() < 1e-4 {
            return Task::none();
        }
        let speed = f64::from(self.effective_speed());
        let tasks = self.decks.d.iter().filter_map(|d| d.player.clone()).map(|p| {
            Task::perform(async move { p.set_speed(speed).await }, |r| Message::PlayerDone(r.map_err(err)))
        });
        Task::batch(tasks)
    }

    /// Pushes the transcoding settings to every connected source.
    pub(super) fn sync_transcode(&self) {
        let t = &self.config.settings.playback.transcode;
        for source in self.sources.values().chain(self.source.iter()) {
            source.set_transcode(t);
        }
    }

    /// Transcoding applies to sources other than YouTube Music (which keeps its
    /// own streaming-quality setting). Shown when any such server is set up.
    pub fn transcoding_available(&self) -> bool {
        self.demo
            || self.config.servers.iter().any(|s| s.kind != listen_core::model::SourceKind::YouTubeMusic)
    }

    fn refresh_devices(&mut self) -> Task<Message> {
        if self.mock_playback() {
            self.audio.devices = demo_devices();
            return Task::none();
        }
        let Some(player) = self.decks.active_player() else {
            self.audio.devices_error = Some("mpv is not running".into());
            return Task::none();
        };
        self.audio.devices_loading = true;
        Task::perform(async move { player.audio_devices().await.map_err(|e| e.to_string()) }, |r| {
            Message::Audio(AudioMsg::Devices(r))
        })
    }

    pub(super) fn on_audio(&mut self, msg: AudioMsg) -> Task<Message> {
        use AudioMsg::*;
        let mut apply = false;
        let mut transcode = false;
        let mut task = Task::none();
        match msg {
            TogglePopover => {
                if self.audio.popup == Some(Popup::Config) {
                    self.audio.popup = None;
                } else {
                    self.audio.popup = Some(Popup::Config);
                    return self.refresh_devices();
                }
                return Task::none();
            }
            ClosePopup => {
                self.audio.popup = None;
                return Task::none();
            }
            OpenDeviceMenu => {
                self.audio.popup = Some(Popup::Devices(self.right_click_at));
                return self.refresh_devices();
            }
            RefreshDevices => return self.refresh_devices(),
            Devices(result) => {
                self.audio.devices_loading = false;
                match result {
                    Ok(list) => {
                        self.audio.devices = list;
                        self.audio.devices_error = None;
                    }
                    Err(e) => self.audio.devices_error = Some(e),
                }
                return Task::none();
            }
            SetDevice(id) => {
                self.config.settings.playback.audio_device = id.clone();
                if matches!(self.audio.popup, Some(Popup::Devices(_))) {
                    self.audio.popup = None;
                }
                apply = true;
                // The AI DJ's voice follows the music to the same output.
                task = self.voice_task(move |v| async move {
                    v.mpv().set_property("audio-device", serde_json::json!(id)).await
                });
            }
            SetSpeed(v) => {
                if self.speed_locked() {
                    return Task::none();
                }
                self.audio.speed = ((v.clamp(SPEED_MIN, SPEED_MAX)) * 100.0).round() / 100.0;
                return self.apply_speed();
            }
            ResetSpeed => {
                self.audio.speed = 1.0;
                return self.apply_speed();
            }
            PitchStep(semitones) => {
                if self.speed_locked() {
                    return Task::none();
                }
                self.audio.speed = step_speed(self.audio.speed, semitones);
                return self.apply_speed();
            }
            PreservePitch(v) => {
                self.config.settings.playback.preserve_pitch = v;
                apply = true;
            }
            Microtonal(v) => self.config.settings.playback.microtonal_pitch = v,
            ReloadMpv => return self.reload_mpv(),
            OpenSettings => {
                self.audio.popup = None;
                return self.handle(Message::Navigate(super::Route::Settings));
            }
            MpvPath(text) => self.config.settings.playback.mpv_path = text,
            MpvArgs(action) => {
                let edit = action.is_edit();
                self.audio.args.perform(action);
                if edit {
                    self.config.settings.playback.mpv_extra_args =
                        settings::parse_mpv_extra_args(&self.audio.args.text());
                }
            }
            ReplayGainPreamp(v) => {
                self.config.settings.playback.replay_gain_preamp_db = v.clamp(-15.0, 15.0);
                apply = true;
            }
            ReplayGainClip(v) => {
                self.config.settings.playback.replay_gain_clip = v;
                apply = true;
            }
            ReplayGainFallback(v) => {
                self.config.settings.playback.replay_gain_fallback_db = v.clamp(-15.0, 15.0);
                apply = true;
            }
            Exclusive(v) => {
                self.config.settings.playback.audio_exclusive = v;
                apply = true;
            }
            SampleRateInput(text) => {
                self.audio.sample_rate_input = Some(text.chars().filter(char::is_ascii_digit).collect());
                return Task::none();
            }
            SampleRateSubmit => {
                if let Some(text) = self.audio.sample_rate_input.take() {
                    let hz = text.parse::<u32>().unwrap_or(0).min(768_000);
                    self.config.settings.playback.sample_rate_hz = if hz >= 8000 { hz } else { 0 };
                    apply = true;
                }
            }
            AudioFormat(v) => {
                if v.is_empty() && !self.config.settings.playback.audio_format.is_empty() {
                    self.toast_info("Reload mpv to go back to the automatic format.".into());
                }
                self.config.settings.playback.audio_format = v;
                apply = true;
            }
            EqPreset(name) => {
                let p = &mut self.config.settings.playback;
                if let Some(preset) = p.eq_preset(&name) {
                    p.apply_eq_preset(&name, &preset);
                    apply = true;
                }
            }
            EqReset => {
                self.config.settings.playback.reset_eq();
                apply = true;
            }
            EqPresetName(text) => {
                self.audio.eq_preset_name = text;
                return Task::none();
            }
            EqSavePreset => {
                let name = self.audio.eq_preset_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.config.settings.playback.save_eq_preset(&name);
                self.audio.eq_preset_name.clear();
                self.toast_success(format!("Saved the equalizer preset \"{name}\"."));
            }
            EqDeletePreset => {
                let p = &mut self.config.settings.playback;
                let name = p.equalizer_preset.clone();
                if p.custom_eq_presets.remove(&name).is_some() {
                    p.equalizer_preset.clear();
                }
            }
            CompEnabled(v) => {
                self.config.settings.playback.compressor.enabled = v;
                apply = true;
            }
            CompParam(param, v) => {
                let c = &mut self.config.settings.playback.compressor.params;
                match param {
                    self::CompParam::Threshold => c.threshold = v.clamp(-60.0, 0.0),
                    self::CompParam::Ratio => c.ratio = v.clamp(1.0, 20.0),
                    self::CompParam::Attack => c.attack = v.clamp(0.1, 2000.0),
                    self::CompParam::Release => c.release = v.clamp(1.0, 9000.0),
                    self::CompParam::Makeup => c.makeup = v.clamp(0.0, 30.0),
                    self::CompParam::Knee => c.knee = v.clamp(1.0, 8.0),
                }
                self.config.settings.playback.compressor_preset.clear();
                apply = true;
            }
            CompPreset(name) => {
                let p = &mut self.config.settings.playback;
                if let Some(preset) = p.compressor_preset(&name) {
                    p.compressor.params = preset;
                    p.compressor_preset = name;
                    apply = true;
                }
            }
            CompPresetName(text) => {
                self.audio.comp_preset_name = text;
                return Task::none();
            }
            CompSavePreset => {
                let name = self.audio.comp_preset_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let p = &mut self.config.settings.playback;
                p.custom_compressor_presets.insert(name.clone(), p.compressor.params);
                p.compressor_preset = name.clone();
                self.audio.comp_preset_name.clear();
                self.toast_success(format!("Saved the compressor preset \"{name}\"."));
            }
            CompDeletePreset => {
                let p = &mut self.config.settings.playback;
                let name = p.compressor_preset.clone();
                if p.custom_compressor_presets.remove(&name).is_some() {
                    p.compressor_preset.clear();
                }
            }
            TranscodeEnabled(v) => {
                self.config.settings.playback.transcode.enabled = v;
                transcode = true;
            }
            TranscodeFormat(v) => {
                self.config.settings.playback.transcode.format = v;
                transcode = true;
            }
            TranscodeBitrate(text) => {
                self.config.settings.playback.transcode.bitrate = digits(&text).min(4000);
                transcode = true;
            }
            TranscodeSampleRate(text) => {
                self.config.settings.playback.transcode.max_sample_rate = digits(&text).min(768_000);
                transcode = true;
            }
        }
        self.save_config();
        if transcode {
            self.sync_transcode();
            self.toast_info("Transcoding applies to the songs that start from now on.".into());
        }
        if apply {
            return Task::batch([task, self.apply_audio()]);
        }
        task
    }

    /// Stops playback and starts mpv anew with the current path and parameters.
    fn reload_mpv(&mut self) -> Task<Message> {
        if self.mock_playback() {
            return Task::none();
        }
        let stop = self.stop_player();
        let generation = self.player_config.generation.wrapping_add(1);
        self.player_config = PlayerConfig::from_settings(&self.config.settings.playback);
        self.player_config.generation = generation;
        // The old processes go with their handles.
        self.decks.set(0, None);
        self.decks.set(1, None);
        self.voice = None;
        self.save_config();
        self.toast_info("Restarting mpv…".into());
        stop
    }
}

fn digits(text: &str) -> u32 {
    text.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semitone_steps_move_the_speed_by_twelfth_roots_of_two() {
        assert!((step_speed(1.0, 12.0) - 2.0).abs() < 1e-5);
        assert!((step_speed(1.0, -12.0) - 0.5).abs() < 1e-5);
        assert!((step_speed(1.0, 1.0) - 1.059_463).abs() < 1e-4);
        // Back to where it was; clamped to the speed range.
        assert!((step_speed(step_speed(1.0, 1.0), -1.0) - 1.0).abs() < 1e-5);
        assert_eq!(step_speed(2.0, 1.0), 2.0);
        assert!((speed_to_pitch(1.0)).abs() < 1e-6);
    }

    #[test]
    fn device_choices_list_the_default_first_and_keep_a_missing_selection() {
        let devices = vec![
            ("auto".to_string(), "Autoselect device".to_string()),
            ("alsa/hw:0".to_string(), "Speakers".to_string()),
        ];
        let c = device_choices(&devices, "auto");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].label, "System default");
        assert_eq!(c[1].id, "alsa/hw:0");
        let c = device_choices(&devices, "alsa/unplugged");
        assert_eq!(c.len(), 3);
        assert!(c[2].label.contains("not found"));
        assert_eq!(device_label(&devices, "alsa/hw:0"), "Speakers");
        assert_eq!(device_label(&devices, "auto"), "System default");
    }
}
