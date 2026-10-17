//! The Milkdrop visualizer (libprojectM) and which visualizer is shown.
//! Mirrors Feishin's `visualizer.butterchurn` settings (`type`, `blendTime`,
//! `cyclePresets`, `cycleTime`, `includeAllPresets`, `selectedPresets`,
//! `ignoredPresets`, `randomizeNextPreset`, `maxFPS`, `opacity`).

use super::*;

/// Feishin's visualizer `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualizerKind {
    /// audioMotion's spectrum analyzer.
    #[default]
    Spectrum,
    /// Milkdrop presets drawn by libprojectM.
    Milkdrop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MilkdropSettings {
    /// Which visualizer the queue panel and the full-screen player show.
    pub kind: VisualizerKind,
    /// Folder with the `.milk` presets; empty = `<data dir>/milkdrop/presets`.
    pub preset_dir: String,
    /// The preset on screen when the visualizer was last used (its name: the
    /// path inside the folder without `.milk`).
    pub current_preset: Option<String>,
    /// Switch to another preset every `cycle_time` seconds.
    pub cycle_presets: bool,
    /// Seconds a preset stays (1-300).
    pub cycle_time: u32,
    /// Seconds a preset takes to blend into the next one (0-10).
    pub blend_time: f32,
    /// The next preset is random (off: the next in the list).
    pub randomize: bool,
    /// Cycle through every preset in the folder (off: only `selected`).
    pub include_all: bool,
    /// The presets cycled through when `include_all` is off.
    pub selected: Vec<String>,
    /// Presets that are never picked.
    pub ignored: Vec<String>,
    /// Frames a second; 0 = 30.
    pub max_fps: u32,
    /// 0-1.
    pub opacity: f32,
}

impl Default for MilkdropSettings {
    fn default() -> Self {
        Self {
            kind: VisualizerKind::Spectrum,
            preset_dir: String::new(),
            current_preset: None,
            cycle_presets: true,
            cycle_time: 30,
            blend_time: 2.5,
            randomize: true,
            include_all: true,
            selected: Vec::new(),
            ignored: Vec::new(),
            max_fps: 0,
            opacity: 1.0,
        }
    }
}

impl MilkdropSettings {
    /// The frame rate to render at.
    pub fn fps(&self) -> u32 {
        if self.max_fps == 0 { 30 } else { self.max_fps.clamp(5, 60) }
    }
}
