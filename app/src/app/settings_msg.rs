//! The settings page's messages and what each one changes.

use super::*;

/// A change made on the settings page.
#[derive(Debug, Clone)]
pub enum Setting {
    QueueVisualizer(bool),
    Milkdrop(super::milkdrop::MilkdropSetting),
    Artist(super::artist_page::ArtistSetting),
    QueryBuilder(super::smart_playlist::QueryTagSetting),
    Sharing(super::item::share::SharingSetting),
    FollowDesktop(bool),
    /// Touchpad scroll speed, percent (100 = like Chromium).
    ScrollSpeed(u16),
    /// The UI language code ("" = system); takes effect after a restart.
    Language(String),
    FollowSystem(bool),
    Theme(String),
    ThemeDark(String),
    ThemeLight(String),
    UseThemeAccent(bool),
    Accent(String),
    CustomTitleBar(bool),
    Window(super::window_state::WindowSetting),
    Integrations(super::integrations::IntegrationSetting),
    Gapless(String),
    ReplayGain(listen_core::settings::ReplayGain),
    StreamQuality(listen_core::settings::StreamQuality),
    Scrobble(bool),
    HistoryEnabled(bool),
    HistoryRetention(listen_core::settings::HistoryRetention),
    HistoryPrivate(bool),
    TrackFade(f32),
    FadeOnPause(bool),
    PreferOwnServers(bool),
    EqEnabled(bool),
    EqBand(usize, f32),
    EqPreamp(f32),
    AutoDj(bool),
    AutoDjCount(u32),
    Analysis(bool),
    AnalysisLibrary(bool),
    AnalysisOnBattery(bool),
    /// Typing the shared folder; saved with `AnalysisDirSave`.
    AnalysisDirInput(String),
    AnalysisDirSave,
    /// Choose the analysis folder in a dialog.
    AnalysisDirPick,
    AnalysisDirPicked(Option<std::path::PathBuf>),
    /// Analyse YouTube Music songs too (asks for a folder first when none is set).
    AnalysisYtm(bool),
    Mixing(bool),
    MixFlow(bool),
    MixMaxTempo(f32),
    MixLongBars(u8),
    MixTempoRamps(bool),
    MixBassSwap(bool),
    MixManualQueue(bool),
    LyricsFetch(bool),
    Lyrics(super::lyrics_state::LyricsSetting),
    Tray(bool),
    ExitToTray(bool),
    Discord(bool),
    ImportPath(String),
    ImportFeishin,
    AiDj(bool),
    AiDjPersona(String),
    AiDjTtsModel(String),
    AiDjSetMax(u32),
    AiDjTalkEvery(u32),
    AiDjMaxVoiced(u32),
    AiDjVoiceGain(f32),
    AiDjLocation(String),
    AiDjPronunciation(String),
    AiDjNotes(String),
    /// Language code; empty = the app's language.
    AiDjLanguage(String),
    AiDjKeyInput(String),
    /// Store the typed key (empty = remove it) and test it.
    AiDjSaveKey,
    AiDjPreview,
    RipAuto(bool),
    RipEject(bool),
    RipSound(bool),
    RipReplayGain(bool),
    RipScan(bool),
    RipPickerTimeout(u32),
    /// Typing in the destination field; `RipSaveDestination` stores it.
    RipDestination(String),
    RipSaveDestination,
    RipOffset(String),
    RipSaveOffset,
    /// Run `whipper offset find`.
    RipVerifyOffset,
}

impl Listen {
    pub(super) fn change_setting(&mut self, change: Setting) -> Task<Message> {
        let s = &mut self.config.settings;
        let mut player_changed = false;
        match change {
            Setting::Language(code) => {
                let changed = s.appearance.language != code;
                s.appearance.language = code;
                if changed {
                    self.toast_restart_required("Restart the application to apply the new language");
                }
            }
            Setting::ScrollSpeed(percent) => s.appearance.scroll_speed = f32::from(percent) / 100.0,
            Setting::QueueVisualizer(v) => s.visualizer.in_queue_panel = v,
            Setting::Milkdrop(change) => return self.milkdrop_setting(change),
            Setting::Artist(change) => change.apply(&mut s.artist),
            Setting::QueryBuilder(change) => change.apply(&mut s.query_builder),
            Setting::Sharing(change) => change.apply(&mut s.sharing),
            Setting::FollowDesktop(v) => {
                s.appearance.follow_desktop_theme = v;
                self.desktop_theme = v
                    .then(|| {
                        listen_core::theme::desktop::config_dir()
                            .and_then(|d| listen_core::theme::desktop::read(&d))
                    })
                    .flatten();
            }
            Setting::FollowSystem(v) => s.appearance.follow_system_theme = v,
            Setting::Theme(id) => s.appearance.theme = id,
            Setting::ThemeDark(id) => s.appearance.theme_dark = id,
            Setting::ThemeLight(id) => s.appearance.theme_light = id,
            Setting::UseThemeAccent(v) => s.appearance.use_theme_accent = v,
            Setting::Accent(text) => {
                if let Some(rgb) = listen_core::theme::Rgb::parse(&text) {
                    s.appearance.accent = Some(rgb);
                }
                self.accent_input = text;
            }
            Setting::CustomTitleBar(v) => {
                let style = if v {
                    listen_core::settings::WindowBarStyle::Windows
                } else {
                    listen_core::settings::WindowBarStyle::Native
                };
                return self.window_setting(super::window_state::WindowSetting::BarStyle(style));
            }
            Setting::Window(change) => return self.window_setting(change),
            Setting::Integrations(change) => return self.integration_setting(change),
            Setting::Gapless(v) => {
                s.playback.gapless = v;
                player_changed = true;
            }
            Setting::ReplayGain(v) => {
                s.playback.replay_gain = v;
                player_changed = true;
            }
            Setting::StreamQuality(v) => {
                s.playback.stream_quality = v;
                for source in self.sources.values().chain(self.source.iter()) {
                    source.set_stream_quality(v);
                }
                self.toast_info("The new quality applies to the songs that start from now on.".into());
            }
            Setting::Scrobble(v) => s.playback.scrobble = v,
            Setting::HistoryEnabled(v) => s.history.enabled = v,
            Setting::HistoryRetention(r) => self.set_history_retention(r),
            Setting::HistoryPrivate(v) => {
                return Task::done(Message::Integrations(super::integrations::IntegrationsMsg::SetPrivate(
                    v,
                )));
            }
            Setting::TrackFade(v) => s.playback.track_fade_secs = v.clamp(0.0, 10.0),
            Setting::FadeOnPause(v) => s.playback.fade_on_pause = v,
            Setting::PreferOwnServers(v) => {
                s.playback.prefer_own_servers = v;
                self.substitutes.set_enabled(v);
            }
            Setting::EqEnabled(v) => {
                s.playback.equalizer_enabled = v;
                player_changed = true;
            }
            Setting::EqBand(i, gain) => {
                if let Some(band) = s.playback.equalizer.get_mut(i) {
                    band.gain = gain;
                }
                player_changed = true;
            }
            Setting::EqPreamp(v) => {
                s.playback.equalizer_preamp = v;
                player_changed = true;
            }
            Setting::AutoDj(v) => {
                s.auto_dj.enabled = v;
                player_changed = true; // the decks' filter chain follows
            }
            Setting::AutoDjCount(v) => s.auto_dj.item_count = v,
            Setting::Analysis(v) => s.analysis.enabled = v,
            Setting::AnalysisLibrary(v) => s.analysis.analyze_library = v,
            Setting::AnalysisOnBattery(v) => s.analysis.on_battery = v,
            Setting::AnalysisDirInput(text) => {
                self.analysis_dir_input = Some(text);
                return Task::none();
            }
            Setting::AnalysisDirSave => {
                if let Some(dir) = self.analysis_dir_input.take() {
                    s.analysis.shared_dir = dir.trim().to_string();
                    if s.analysis.shared_dir.is_empty() {
                        s.analysis.youtube_music = false;
                    }
                }
            }
            Setting::AnalysisDirPick => {
                let start = Some(super::shellexpand_home(s.analysis.shared_dir.trim()))
                    .filter(|p| !p.as_os_str().is_empty());
                return Task::perform(
                    async move {
                        crate::dialog::open_folder(crate::dialog::Pick {
                            title: "Folder for analysis data",
                            start,
                            filter_name: "",
                            patterns: &[],
                        })
                        .await
                    },
                    |p| Message::Setting(Setting::AnalysisDirPicked(p)),
                );
            }
            Setting::AnalysisDirPicked(None) => return Task::none(),
            Setting::AnalysisDirPicked(Some(dir)) => {
                s.analysis.shared_dir = dir.display().to_string();
                self.analysis_dir_input = None;
                // Picked while switching YouTube Music analysis on: finish that.
                if self.pending_ytm_analysis {
                    s.analysis.youtube_music = true;
                    self.pending_ytm_analysis = false;
                }
            }
            Setting::AnalysisYtm(on) => {
                if on && s.analysis.shared_dir.trim().is_empty() {
                    self.pending_ytm_analysis = true;
                    return self.update(Message::Setting(Setting::AnalysisDirPick));
                }
                s.analysis.youtube_music = on;
            }
            Setting::Mixing(v) => {
                s.mixing.enabled = v;
                player_changed = true;
            }
            Setting::MixFlow(v) => s.mixing.flow_select = v,
            Setting::MixMaxTempo(v) => s.mixing.max_tempo_pct = v.clamp(2.0, 8.0),
            Setting::MixLongBars(v) => s.mixing.long_blend_bars = if v >= 16 { 16 } else { 8 },
            Setting::MixTempoRamps(v) => s.mixing.tempo_ramps = v,
            Setting::MixBassSwap(v) => s.mixing.bass_swap = v,
            Setting::MixManualQueue(v) => s.mixing.mix_manual_queue = v,
            Setting::LyricsFetch(v) => s.lyrics.fetch = v,
            Setting::Lyrics(change) => change.apply(&mut s.lyrics),
            Setting::Tray(v) => s.window.tray = v,
            Setting::ExitToTray(v) => s.window.exit_to_tray = v,
            Setting::Discord(v) => {
                s.window.discord_rpc = v;
                self.integ.discord_dirty();
            }
            Setting::ImportPath(p) => {
                self.import_path = p;
                return Task::none();
            }
            Setting::ImportFeishin => {
                let path = shellexpand_home(self.import_path.trim());
                match std::fs::read_to_string(&path) {
                    Ok(json) => match listen_core::settings::import_feishin(&self.config.settings, &json) {
                        Ok(imported) => {
                            self.config.settings = imported;
                            self.toast_success("Feishin settings imported.".into());
                            player_changed = true;
                        }
                        Err(e) => self.toast_error(format!("Not a Feishin settings file: {e}")),
                    },
                    Err(e) => self.toast_error(format!("Could not read {}: {e}", path.display())),
                }
            }
            other @ (Setting::AiDj(_)
            | Setting::AiDjPersona(_)
            | Setting::AiDjTtsModel(_)
            | Setting::AiDjSetMax(_)
            | Setting::AiDjTalkEvery(_)
            | Setting::AiDjMaxVoiced(_)
            | Setting::AiDjVoiceGain(_)
            | Setting::AiDjLocation(_)
            | Setting::AiDjPronunciation(_)
            | Setting::AiDjNotes(_)
            | Setting::AiDjLanguage(_)
            | Setting::AiDjKeyInput(_)
            | Setting::AiDjSaveKey
            | Setting::AiDjPreview) => return self.dj_setting(other),
            other @ (Setting::RipAuto(_)
            | Setting::RipEject(_)
            | Setting::RipSound(_)
            | Setting::RipReplayGain(_)
            | Setting::RipScan(_)
            | Setting::RipPickerTimeout(_)
            | Setting::RipDestination(_)
            | Setting::RipSaveDestination
            | Setting::RipOffset(_)
            | Setting::RipSaveOffset
            | Setting::RipVerifyOffset) => return self.rip_setting(other),
        }
        self.apply_theme();
        self.save_config();
        if player_changed { self.apply_audio() } else { Task::none() }
    }
}
