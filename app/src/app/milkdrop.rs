//! The Milkdrop visualizer's state and messages: the preset list, the frame
//! on screen, the settings (Feishin's butterchurn options) and the actions
//! (next preset, pick a preset, choose the preset folder).

use super::*;
use crate::milkdrop::presets::{self, Preset};
use iced::widget::image::Handle;
use listen_core::settings::VisualizerKind;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum MilkdropMsg {
    /// A finished picture from the render thread.
    Frame(Handle),
    /// The preset on screen changed (cycling, next, pick).
    Preset(String),
    /// A preset that wouldn't load (it is skipped from now on).
    Failed(String, String),
    /// The folder has no presets.
    NoPresets,
    /// Milkdrop can't run.
    Error(String),
    /// Read the preset folder again.
    Rescan,
    Scanned(PathBuf, Arc<Vec<Preset>>),
    /// The next preset (button).
    Next,
    Pick(String),
    /// Typing the folder; saved with `FolderSave`.
    FolderInput(String),
    FolderSave,
    /// Text typed in the preset search.
    Query(String),
    /// Open the folder picker.
    PickFolder,
    FolderPicked(Option<PathBuf>),
}

/// A change made in the Milkdrop settings.
#[derive(Debug, Clone)]
pub enum MilkdropSetting {
    Kind(VisualizerKind),
    Cycle(bool),
    CycleTime(u32),
    BlendTime(f32),
    Randomize(bool),
    IncludeAll(bool),
    MaxFps(u32),
    Opacity(f32),
    /// Never pick the preset on screen again.
    IgnoreCurrent,
    ClearIgnored,
    /// Add / remove the preset on screen from the selection.
    ToggleSelected,
    ClearSelected,
}

#[derive(Default)]
pub struct MilkdropState {
    /// The last picture; kept while paused.
    pub frame: Option<Handle>,
    /// The preset on screen.
    pub preset: Option<String>,
    pub presets: Arc<Vec<Preset>>,
    /// The folder `presets` was read from; `None` before the first scan.
    pub scanned: Option<PathBuf>,
    pub scanning: bool,
    /// The folder has no presets.
    pub empty: bool,
    pub error: Option<String>,
    /// The last preset that failed to load, shown as a hint.
    pub failed: Option<String>,
    /// The folder being typed (settings).
    pub folder_input: Option<String>,
    /// The preset search text (settings).
    pub query: String,
}

impl Listen {
    /// Milkdrop is the chosen visualizer and can run here.
    pub fn milkdrop_active(&self) -> bool {
        self.config.settings.milkdrop.kind == VisualizerKind::Milkdrop
            && crate::milkdrop::availability().is_ok()
    }

    /// The folder presets are read from.
    pub fn milkdrop_folder(&self) -> Option<PathBuf> {
        presets::folder(
            &self.config.settings.milkdrop.preset_dir,
            self.paths.as_ref().map(|p| p.data_dir.as_path()),
        )
    }

    /// Reads the preset folder in the background.
    pub fn milkdrop_scan(&mut self) -> Task<Message> {
        let Some(dir) = self.milkdrop_folder() else { return Task::none() };
        self.milk.scanning = true;
        Task::perform(
            async move {
                let d = dir.clone();
                let found = tokio::task::spawn_blocking(move || presets::scan(&d)).await.unwrap_or_default();
                (dir, found)
            },
            |(dir, found)| Message::Milkdrop(MilkdropMsg::Scanned(dir, Arc::new(found))),
        )
    }

    /// Scans on first use (called when Milkdrop is chosen or at boot).
    pub fn milkdrop_boot(&mut self) -> Task<Message> {
        if self.config.settings.milkdrop.kind == VisualizerKind::Milkdrop && self.milk.scanned.is_none() {
            self.milkdrop_scan()
        } else {
            Task::none()
        }
    }

    /// What the render thread works from; called when the subscription is built.
    #[cfg(target_os = "linux")]
    pub fn milkdrop_sync(&self) {
        if let Some(root) = self.milkdrop_folder() {
            crate::milkdrop::set_config(crate::milkdrop::Config {
                root,
                presets: self.milk.presets.clone(),
                settings: self.config.settings.milkdrop.clone(),
            });
        }
    }

    pub(super) fn on_milkdrop(&mut self, msg: MilkdropMsg) -> Task<Message> {
        match msg {
            MilkdropMsg::Frame(h) => {
                self.milk.error = None;
                self.milk.empty = false;
                self.milk.frame = Some(h);
            }
            MilkdropMsg::Preset(name) => {
                self.milk.empty = false;
                if self.config.settings.milkdrop.current_preset.as_deref() != Some(name.as_str()) {
                    self.config.settings.milkdrop.current_preset = Some(name.clone());
                    self.save_config();
                }
                self.milk.preset = Some(name);
            }
            MilkdropMsg::Failed(name, why) => {
                tracing::debug!("milkdrop preset {name}: {why}");
                self.milk.failed = Some(name);
            }
            MilkdropMsg::NoPresets => self.milk.empty = true,
            MilkdropMsg::Error(e) => self.milk.error = Some(e),
            MilkdropMsg::Rescan => return self.milkdrop_scan(),
            MilkdropMsg::Scanned(dir, found) => {
                self.milk.scanning = false;
                // A folder changed meanwhile: that scan is on its way.
                if self.milkdrop_folder().as_deref() == Some(dir.as_path()) {
                    self.milk.empty = found.is_empty();
                    self.milk.presets = found;
                    self.milk.scanned = Some(dir);
                }
            }
            MilkdropMsg::Next => {
                #[cfg(target_os = "linux")]
                crate::milkdrop::request_next();
            }
            MilkdropMsg::Pick(name) => {
                self.milk.preset = Some(name.clone());
                self.config.settings.milkdrop.current_preset = Some(name.clone());
                self.save_config();
                #[cfg(target_os = "linux")]
                crate::milkdrop::request_pick(name);
            }
            MilkdropMsg::FolderInput(text) => self.milk.folder_input = Some(text),
            MilkdropMsg::FolderSave => {
                if let Some(text) = self.milk.folder_input.take() {
                    return self.milkdrop_set_folder(text.trim().to_string());
                }
            }
            MilkdropMsg::Query(q) => self.milk.query = q,
            MilkdropMsg::PickFolder => {
                let start = self.milkdrop_folder();
                return Task::perform(
                    async move {
                        crate::dialog::open_folder(crate::dialog::Pick {
                            title: "Milkdrop presets folder",
                            start,
                            filter_name: "",
                            patterns: &[],
                        })
                        .await
                    },
                    |p| Message::Milkdrop(MilkdropMsg::FolderPicked(p)),
                );
            }
            MilkdropMsg::FolderPicked(Some(path)) => {
                return self.milkdrop_set_folder(path.to_string_lossy().into_owned());
            }
            MilkdropMsg::FolderPicked(None) => {}
        }
        Task::none()
    }

    fn milkdrop_set_folder(&mut self, text: String) -> Task<Message> {
        self.config.settings.milkdrop.preset_dir = text;
        self.milk.folder_input = None;
        self.milk.scanned = None;
        self.save_config();
        self.milkdrop_scan()
    }

    pub(super) fn milkdrop_setting(&mut self, change: MilkdropSetting) -> Task<Message> {
        let current = self.milk.preset.clone();
        let m = &mut self.config.settings.milkdrop;
        let mut task = Task::none();
        match change {
            MilkdropSetting::Kind(k) => {
                m.kind = k;
                task = self.milkdrop_boot();
            }
            MilkdropSetting::Cycle(v) => m.cycle_presets = v,
            MilkdropSetting::CycleTime(v) => m.cycle_time = v.clamp(1, 300),
            MilkdropSetting::BlendTime(v) => m.blend_time = (v * 10.0).round() / 10.0,
            MilkdropSetting::Randomize(v) => m.randomize = v,
            MilkdropSetting::IncludeAll(v) => m.include_all = v,
            MilkdropSetting::MaxFps(v) => m.max_fps = v.min(144),
            MilkdropSetting::Opacity(v) => m.opacity = v.clamp(0.0, 1.0),
            MilkdropSetting::IgnoreCurrent => {
                if let Some(c) = current
                    && !m.ignored.contains(&c)
                {
                    m.ignored.push(c);
                    #[cfg(target_os = "linux")]
                    crate::milkdrop::request_next();
                }
            }
            MilkdropSetting::ClearIgnored => m.ignored.clear(),
            MilkdropSetting::ToggleSelected => {
                if let Some(c) = current {
                    if let Some(i) = m.selected.iter().position(|s| *s == c) {
                        m.selected.remove(i);
                    } else {
                        m.selected.push(c);
                    }
                }
            }
            MilkdropSetting::ClearSelected => m.selected.clear(),
        }
        self.save_config();
        task
    }
}
