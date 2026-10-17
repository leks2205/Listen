//! Player bar items.

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerBar {
    /// Extra metadata shown under the title (`codec`, `release_year`, …).
    pub info_items: Vec<String>,
    pub skip_buttons: bool,
    pub skip_backward_secs: u32,
    pub skip_forward_secs: u32,
    pub volume_wheel_step: u8,
    pub volume_width: u16,
    pub button_size: u16,
    /// A click on the bar's empty area opens the full-screen player (Feishin `playerbarOpenDrawer`).
    pub open_drawer: bool,
    /// The right duration shows the time left ("-m:ss") instead of the total.
    pub show_time_remaining: bool,
    /// The right-hand buttons: order and visibility.
    pub right_controls: Vec<RightControl>,
    /// Plain slider, or the song's waveform as the seek bar (Feishin's `playerbarSlider`).
    pub slider_type: SliderType,
    pub waveform_align: WaveformAlign,
    pub waveform_bar_width: u32,
    pub waveform_gap: u32,
    pub waveform_radius: u32,
    /// Scale the waveform up to the full height of the bar.
    pub waveform_stretch: bool,
    /// Seconds to wait before the waveform of a new song is computed.
    pub waveform_delay_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SliderType {
    #[default]
    Slider,
    Waveform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaveformAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

/// One button of the bar's right column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightControl {
    /// One of [`RIGHT_CONTROLS`].
    pub id: String,
    pub visible: bool,
}

/// Button ids with their names, in the default order.
pub const RIGHT_CONTROLS: &[(&str, &str)] = &[
    ("sleep_timer", "Sleep timer"),
    ("player_config", "Player config"),
    ("visualizer", "Visualizer"),
    ("lyrics", "Lyrics"),
    ("favorite", "Favorite"),
    ("queue", "Queue"),
    ("volume", "Volume"),
];

impl RightControl {
    pub fn defaults() -> Vec<RightControl> {
        RIGHT_CONTROLS.iter().map(|(id, _)| RightControl { id: (*id).into(), visible: true }).collect()
    }
}

impl PlayerBar {
    /// The visible right-hand buttons in order: unknown ids dropped, ones missing
    /// from a saved list (added later) appended.
    pub fn right_controls_ordered(&self) -> Vec<&str> {
        let known = |id: &str| RIGHT_CONTROLS.iter().any(|(k, _)| *k == id);
        let mut out: Vec<&str> =
            self.right_controls.iter().filter(|c| c.visible && known(&c.id)).map(|c| c.id.as_str()).collect();
        for (id, _) in RIGHT_CONTROLS {
            if !self.right_controls.iter().any(|c| c.id == *id) {
                out.push(id);
            }
        }
        out
    }
}

impl Default for PlayerBar {
    fn default() -> Self {
        Self {
            info_items: vec!["codec".into(), "release_year".into()],
            skip_buttons: true,
            skip_backward_secs: 5,
            skip_forward_secs: 10,
            volume_wheel_step: 5,
            volume_width: 70,
            button_size: 15,
            open_drawer: false,
            show_time_remaining: false,
            right_controls: RightControl::defaults(),
            slider_type: SliderType::Slider,
            waveform_align: WaveformAlign::Center,
            waveform_bar_width: 2,
            waveform_gap: 1,
            waveform_radius: 4,
            waveform_stretch: false,
            waveform_delay_secs: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_controls_keep_order_and_add_new_ones() {
        let mut b = PlayerBar::default();
        assert_eq!(b.right_controls_ordered().len(), RIGHT_CONTROLS.len());
        b.right_controls = vec![
            RightControl { id: "queue".into(), visible: true },
            RightControl { id: "lyrics".into(), visible: false },
            RightControl { id: "gone".into(), visible: true },
        ];
        let o = b.right_controls_ordered();
        assert_eq!(o[0], "queue");
        assert!(!o.contains(&"lyrics") && !o.contains(&"gone"));
        // The ones the saved list lacks come after.
        assert!(o.contains(&"volume") && o.contains(&"sleep_timer"));
    }

    #[test]
    fn old_config_without_the_new_fields_loads() {
        let b: PlayerBar = toml::from_str("button_size = 20").unwrap();
        assert!(!b.open_drawer && b.right_controls.len() == RIGHT_CONTROLS.len());
    }

    #[test]
    fn waveform_settings_serde_and_old_configs() {
        let old: PlayerBar = toml::from_str("skip_buttons = false\n").unwrap();
        assert_eq!(old.slider_type, SliderType::Slider);
        assert_eq!((old.waveform_bar_width, old.waveform_gap, old.waveform_radius), (2, 1, 4));
        let pb = PlayerBar {
            slider_type: SliderType::Waveform,
            waveform_align: WaveformAlign::Bottom,
            waveform_stretch: true,
            ..PlayerBar::default()
        };
        let text = toml::to_string(&pb).unwrap();
        assert!(text.contains("slider_type = \"waveform\"") && text.contains("waveform_align = \"bottom\""));
        assert_eq!(toml::from_str::<PlayerBar>(&text).unwrap(), pb);
    }

    #[test]
    fn feishin_import_reads_slider_and_visualizer() {
        let json = r#"{"general":{"playerbarSlider":{"type":"waveform","barAlign":"top","barWidth":3,
            "barGap":2,"barRadius":1,"stretched":true,"loadingDelay":5}},
            "visualizer":{"audiomotionanalyzer":{"mode":4,"ledBars":true,"maxFPS":30,
            "presets":[{"id":"x","name":"Preset 1","value":{"mode":1}},{"id":"y","name":"Mine","value":{"mode":6}}]}}}"#;
        let s = crate::settings::import_feishin(&Settings::default(), json).unwrap();
        let pb = &s.player_bar;
        assert_eq!(pb.slider_type, SliderType::Waveform);
        assert_eq!(pb.waveform_align, WaveformAlign::Top);
        assert_eq!((pb.waveform_bar_width, pb.waveform_gap, pb.waveform_radius), (3, 2, 1));
        assert!(pb.waveform_stretch);
        assert_eq!(pb.waveform_delay_secs, 5);
        assert_eq!((s.visualizer.mode, s.visualizer.led_bars, s.visualizer.max_fps), (4, true, 30));
        // Feishin's built-in preset is not copied, a user preset is.
        assert_eq!(s.visualizer.presets.len(), 1);
        assert_eq!(s.visualizer.presets[0].name, "Mine");
    }
}
