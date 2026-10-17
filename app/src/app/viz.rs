//! Visualizer settings (the audioMotion options modal, presets, custom
//! gradients, copy / paste) and the player bar's waveform: the data of the
//! playing song, loaded from the cache or computed from its audio.

use super::*;
use listen_analysis::Cancel;
use listen_analysis::waveform::{Waveform, WaveformCache};
use listen_core::settings::{CustomGradient, GradientStop, SliderType, Visualizer, WaveformAlign};
use std::sync::Arc;

/// Key of the visualizer settings modal.
pub const MODAL: &str = "visualizer-settings";

/// Fields of the player bar slider settings.
#[derive(Debug, Clone, PartialEq)]
pub enum SliderSetting {
    Type(SliderType),
    Align(WaveformAlign),
    BarWidth(u32),
    Gap(u32),
    Radius(u32),
    Stretch(bool),
    Delay(u32),
}

#[derive(Debug, Clone)]
pub enum VizMsg {
    /// Open the options modal.
    Open,
    /// One option, by its Feishin (camelCase) name.
    Set(&'static str, serde_json::Value),
    /// Back to Feishin's defaults (keeps the saved presets).
    Reset,
    PresetPick(String),
    PresetName(String),
    PresetSave,
    PresetDelete,
    CopyConfig,
    PasteConfig,
    Pasted(Option<String>),
    GradientAdd,
    GradientRemove(usize),
    GradientName(usize, String),
    GradientHorizontal(usize, bool),
    StopColor(usize, usize, String),
    StopAdd(usize),
    StopRemove(usize, usize),
    GradientCopy(usize),
    GradientPaste,
    GradientPasted(Option<String>),
    Slider(SliderSetting),
    WaveformReady(String, Option<Arc<Waveform>>),
}

/// What the waveform seek bar shows.
#[derive(Default)]
pub struct WaveformState {
    /// The song the data is (or will be) for.
    pub key: Option<String>,
    pub data: Option<Arc<Waveform>>,
    /// The song has no audio to read yet (YouTube Music before a copy exists).
    pub no_input: bool,
    cancel: Cancel,
}

#[derive(Default)]
pub struct VizState {
    pub waveform: WaveformState,
    /// The name typed for “save as preset”.
    pub preset_name: String,
    /// The preset picked in the list.
    pub preset: Option<String>,
}

impl Listen {
    fn waveform_cache(&self) -> Option<WaveformCache> {
        self.paths.as_ref().map(|p| WaveformCache::new(p.cache_dir.join("waveforms")))
    }

    /// Points the waveform at the playing song: from the cache, else computed after the configured delay
    /// (not for a YouTube Music song without a local copy).
    pub fn waveform_sync(&mut self) -> Task<Message> {
        let pb = &self.config.settings.player_bar;
        let wanted = pb.slider_type == SliderType::Waveform;
        let song = self.queue.current().map(|i| i.song.clone());
        let key = song.as_ref().map(|s| format!("{}-{}", s.server_id.0, listen_analysis::record_key(s)));
        let state = &mut self.viz.waveform;
        if !wanted || key.is_none() {
            state.cancel.cancel();
            *state = WaveformState::default();
            return Task::none();
        }
        if state.key == key {
            return Task::none();
        }
        state.cancel.cancel();
        state.cancel = Cancel::default();
        state.key = key.clone();
        state.data = None;
        state.no_input = false;
        let (Some(song), Some(key)) = (song, key) else { return Task::none() };
        let cache = self.waveform_cache();
        if let Some(wf) = cache.as_ref().and_then(|c| c.get(&key)) {
            self.viz.waveform.data = Some(Arc::new(wf));
            return Task::none();
        }
        let Some(input) = self.sources.get(&song.server_id).and_then(|s| s.waveform_input(&song)) else {
            self.viz.waveform.no_input = true;
            return Task::none();
        };
        let delay = std::time::Duration::from_secs(pb.waveform_delay_secs.into());
        let cancel = self.viz.waveform.cancel.clone();
        let done = key.clone();
        Task::perform(
            async move {
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = cancel.cancelled() => return None,
                }
                let wf = listen_analysis::waveform::compute(&input.as_mpv_arg(), &cancel).await;
                match wf {
                    Ok(wf) => {
                        if let Some(cache) = cache
                            && let Err(e) = cache.put(&key, &wf)
                        {
                            tracing::warn!("waveform cache: {e}");
                        }
                        Some(Arc::new(wf))
                    }
                    Err(e) => {
                        tracing::debug!("waveform of “{}”: {e}", song.title);
                        None
                    }
                }
            },
            move |wf| Message::Viz(VizMsg::WaveformReady(done.clone(), wf)),
        )
    }

    /// A song was analysed (its audio is now on disk): computes a waveform that was waiting for input.
    pub fn waveform_analysed(&mut self, song_id: &str) -> Task<Message> {
        let current = self.queue.current().is_some_and(|i| i.song.id == song_id);
        if current && self.viz.waveform.no_input {
            self.viz.waveform.key = None;
            return self.waveform_sync();
        }
        Task::none()
    }

    fn viz_edit(&mut self, f: impl FnOnce(&mut Visualizer)) {
        f(&mut self.config.settings.visualizer);
        self.save_config();
    }

    pub(super) fn on_viz(&mut self, msg: VizMsg) -> Task<Message> {
        match msg {
            VizMsg::Open => self.open_modal(MODAL),
            VizMsg::Set(key, value) => {
                let json = serde_json::json!({ key: value }).to_string();
                self.viz_edit(|v| {
                    let _ = v.apply_json(&json);
                });
            }
            VizMsg::Reset => self.viz_edit(|v| {
                let keep = Visualizer {
                    presets: std::mem::take(&mut v.presets),
                    in_queue_panel: v.in_queue_panel,
                    custom_gradients: std::mem::take(&mut v.custom_gradients),
                    ..Visualizer::default()
                };
                *v = keep;
            }),
            VizMsg::PresetPick(name) => {
                let result = self.config.settings.visualizer.load_preset(&name);
                match result {
                    Ok(()) => {
                        self.viz.preset_name = name.clone();
                        self.viz.preset = Some(name);
                        self.save_config();
                    }
                    Err(e) => self.toast_error(e),
                }
            }
            VizMsg::PresetName(name) => self.viz.preset_name = name,
            VizMsg::PresetSave => {
                let name = self.viz.preset_name.clone();
                if builtin_name(&name) {
                    self.toast_warning("The built-in presets can't be replaced; pick another name.".into());
                } else if self.config.settings.visualizer.save_preset(&name) {
                    self.viz.preset = Some(name.trim().to_string());
                    self.save_config();
                    self.toast_success("Preset saved.".into());
                } else {
                    self.toast_warning("Enter a name for the preset.".into());
                }
            }
            VizMsg::PresetDelete => {
                if let Some(name) = self.viz.preset.take()
                    && self.config.settings.visualizer.delete_preset(&name)
                {
                    self.save_config();
                }
            }
            VizMsg::CopyConfig => {
                let text = serde_json::to_string_pretty(&self.config.settings.visualizer.config_json())
                    .unwrap_or_default();
                self.toast_info("Configuration copied to the clipboard.".into());
                return iced::clipboard::write(text);
            }
            VizMsg::PasteConfig => {
                return iced::clipboard::read().map(|t| Message::Viz(VizMsg::Pasted(t)));
            }
            VizMsg::Pasted(text) => {
                let Some(text) = text else {
                    self.toast_error("The clipboard has no text.".into());
                    return Task::none();
                };
                match self.config.settings.visualizer.apply_json(&text) {
                    Ok(()) => {
                        self.save_config();
                        self.toast_success("Configuration applied.".into());
                    }
                    Err(e) => self.toast_error(format!("Failed to apply the configuration: {e}")),
                }
            }
            VizMsg::GradientAdd => self.viz_edit(|v| {
                let mut n = v.custom_gradients.len() + 1;
                while v.custom_gradient(&format!("Custom {n}")).is_some() {
                    n += 1;
                }
                v.custom_gradients
                    .push(CustomGradient { name: format!("Custom {n}"), ..CustomGradient::default() });
            }),
            VizMsg::GradientRemove(i) => self.viz_edit(|v| {
                if i < v.custom_gradients.len() {
                    let gone = v.custom_gradients.remove(i).name;
                    // Anything that used it falls back to prism.
                    if v.gradient == gone {
                        v.gradient = "prism".into();
                    }
                    for slot in [&mut v.gradient_left, &mut v.gradient_right] {
                        if slot.as_deref() == Some(gone.as_str()) {
                            *slot = None;
                        }
                    }
                }
            }),
            VizMsg::GradientName(i, name) => self.viz_edit(|v| {
                let taken = v.custom_gradients.iter().enumerate().any(|(j, g)| j != i && g.name == name);
                if let Some(g) = v.custom_gradients.get_mut(i)
                    && !name.is_empty()
                    && !taken
                {
                    let old = std::mem::replace(&mut g.name, name.clone());
                    if v.gradient == old {
                        v.gradient = name.clone();
                    }
                    for slot in [&mut v.gradient_left, &mut v.gradient_right] {
                        if slot.as_deref() == Some(old.as_str()) {
                            *slot = Some(name.clone());
                        }
                    }
                }
            }),
            VizMsg::GradientHorizontal(i, horizontal) => self.viz_edit(|v| {
                if let Some(g) = v.custom_gradients.get_mut(i) {
                    g.dir = if horizontal { "h" } else { "v" }.into();
                }
            }),
            VizMsg::StopColor(i, j, color) => self.viz_edit(|v| {
                if let Some(s) = v.custom_gradients.get_mut(i).and_then(|g| g.color_stops.get_mut(j)) {
                    s.color = color;
                }
            }),
            VizMsg::StopAdd(i) => self.viz_edit(|v| {
                if let Some(g) = v.custom_gradients.get_mut(i) {
                    g.color_stops.push(GradientStop { color: "#ffffff".into(), pos: None, level: None });
                }
            }),
            VizMsg::StopRemove(i, j) => self.viz_edit(|v| {
                if let Some(g) = v.custom_gradients.get_mut(i)
                    && g.color_stops.len() > 1
                    && j < g.color_stops.len()
                {
                    g.color_stops.remove(j);
                }
            }),
            VizMsg::GradientCopy(i) => {
                let Some(g) = self.config.settings.visualizer.custom_gradients.get(i) else {
                    return Task::none();
                };
                let text = serde_json::to_string_pretty(g).unwrap_or_default();
                self.toast_info("Gradient copied to the clipboard.".into());
                return iced::clipboard::write(text);
            }
            VizMsg::GradientPaste => {
                return iced::clipboard::read().map(|t| Message::Viz(VizMsg::GradientPasted(t)));
            }
            VizMsg::GradientPasted(text) => match text.as_deref().map(parse_gradient) {
                Some(Ok(mut g)) => {
                    let v = &mut self.config.settings.visualizer;
                    let base = g.name.clone();
                    let mut n = 1;
                    while v.custom_gradient(&g.name).is_some() {
                        n += 1;
                        g.name = format!("{base} {n}");
                    }
                    v.custom_gradients.push(g);
                    self.save_config();
                }
                Some(Err(e)) => self.toast_error(format!("Not a gradient: {e}")),
                None => self.toast_error("The clipboard has no text.".into()),
            },
            VizMsg::Slider(change) => {
                let pb = &mut self.config.settings.player_bar;
                match change {
                    SliderSetting::Type(t) => pb.slider_type = t,
                    SliderSetting::Align(a) => pb.waveform_align = a,
                    SliderSetting::BarWidth(v) => pb.waveform_bar_width = v.clamp(1, 20),
                    SliderSetting::Gap(v) => pb.waveform_gap = v.min(20),
                    SliderSetting::Radius(v) => pb.waveform_radius = v.min(20),
                    SliderSetting::Stretch(v) => pb.waveform_stretch = v,
                    SliderSetting::Delay(v) => pb.waveform_delay_secs = v.min(60),
                }
                self.save_config();
                return self.waveform_sync();
            }
            VizMsg::WaveformReady(key, data) => {
                if self.viz.waveform.key.as_deref() == Some(key.as_str()) {
                    self.viz.waveform.data = data;
                }
            }
        }
        Task::none()
    }
}

fn builtin_name(name: &str) -> bool {
    listen_core::settings::builtin_presets().iter().any(|p| p.name == name.trim())
}

/// A gradient from JSON: `{"name", "dir", "colorStops": [{"color", "pos"?, "level"?}]}`
/// (Feishin's format), or just a list of colour stops.
pub fn parse_gradient(text: &str) -> Result<CustomGradient, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut g: CustomGradient = if value.is_array() {
        CustomGradient {
            color_stops: serde_json::from_value(value).map_err(|e| e.to_string())?,
            ..CustomGradient::default()
        }
    } else {
        serde_json::from_value(value).map_err(|e| e.to_string())?
    };
    if g.color_stops.is_empty() {
        return Err("no colour stops".into());
    }
    if g.name.trim().is_empty() {
        g.name = "Pasted".into();
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradients_paste_from_feishin_json() {
        let g = parse_gradient(
            r##"{"name":"Fire","dir":"h","colorStops":[{"color":"#f00","pos":0},{"color":"#ff0","level":0.5,"levelEnabled":true},{"color":"yellow"}]}"##,
        )
        .unwrap();
        assert_eq!((g.name.as_str(), g.dir.as_str(), g.color_stops.len()), ("Fire", "h", 3));
        assert_eq!(g.color_stops[1].level, Some(0.5));
        let list = parse_gradient(r##"[{"color":"#000"},{"color":"#fff"}]"##).unwrap();
        assert_eq!(list.color_stops.len(), 2);
        assert!(parse_gradient("[]").is_err() && parse_gradient("nope").is_err());
        assert!(parse_gradient(r#"{"colorStops":[]}"#).is_err());
    }
}
