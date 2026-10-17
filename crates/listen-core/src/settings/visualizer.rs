//! The audio visualizer: audioMotion-analyzer's options, presets and custom
//! gradients. Names and ranges are Feishin's, so a configuration copied from
//! Feishin (camelCase JSON) can be pasted here and the other way round.

use super::*;

/// How the channels are laid out (audioMotion's `channelLayout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelLayout {
    #[default]
    Single,
    /// Both channels in the same area, drawn over each other.
    DualCombined,
    /// Left on the left half, right on the right half.
    DualHorizontal,
    /// Left above, right below.
    DualVertical,
}

/// What decides a bar's colour (audioMotion's `colorMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorMode {
    /// The gradient across the whole canvas.
    #[default]
    Gradient,
    /// One colour per bar, along the gradient.
    BarIndex,
    /// One colour per bar from its level.
    BarLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrequencyScale {
    #[default]
    Log,
    Linear,
    Bark,
    Mel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WeightingFilter {
    #[default]
    #[serde(rename = "")]
    None,
    A,
    B,
    C,
    D,
    /// ITU-R 468.
    Z,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradientStop {
    /// `#rrggbb` (or `#rgb`).
    pub color: String,
    pub pos: Option<f32>,
    pub level: Option<f32>,
}

/// A custom gradient (Feishin's `customGradients` entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomGradient {
    pub name: String,
    /// `h` = horizontal; anything else vertical.
    pub dir: String,
    pub color_stops: Vec<GradientStop>,
}

impl Default for CustomGradient {
    fn default() -> Self {
        Self {
            name: "Custom".into(),
            dir: "v".into(),
            color_stops: vec![
                GradientStop { color: "#ff0000".into(), pos: None, level: None },
                GradientStop { color: "#0000ff".into(), pos: None, level: None },
            ],
        }
    }
}

/// A saved set of options; `value` is the Feishin-style JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualizerPreset {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Visualizer {
    /// audioMotion mode: 0 = discrete frequencies, 1–8 = octave bands, 10 = line/area graph.
    pub mode: u8,
    pub gradient: String,
    pub gradient_left: Option<String>,
    pub gradient_right: Option<String>,
    pub custom_gradients: Vec<CustomGradient>,
    pub color_mode: ColorMode,
    pub split_gradient: bool,
    pub channel_layout: ChannelLayout,
    pub fft_size: u32,
    pub min_freq: f32,
    pub max_freq: f32,
    pub min_db: f32,
    pub max_db: f32,
    pub smoothing: f32,
    pub frequency_scale: FrequencyScale,
    pub weighting_filter: WeightingFilter,
    pub linear_amplitude: bool,
    pub linear_boost: f32,
    /// Input gain.
    pub volume: f32,
    pub peak_line: bool,
    pub show_peaks: bool,
    pub fade_peaks: bool,
    /// Fall speed of peaks (audioMotion `gravity`).
    pub gravity: f32,
    pub peak_hold_time: u32,
    pub peak_fade_time: u32,
    pub line_width: f32,
    pub fill_alpha: f32,
    /// Space between bars: 0..1 of the bar width, or pixels when ≥ 1.
    pub bar_space: f32,
    pub alpha_bars: bool,
    pub led_bars: bool,
    pub lumi_bars: bool,
    pub outline_bars: bool,
    pub round_bars: bool,
    pub true_leds: bool,
    pub lo_res: bool,
    pub ansi_bands: bool,
    /// −1 = mirror to the left, 1 = to the right, 0 = off.
    pub mirror: i8,
    pub radial: bool,
    pub radial_invert: bool,
    pub radius: f32,
    /// Revolutions per minute.
    pub spin_speed: f32,
    pub reflex_ratio: f32,
    pub reflex_alpha: f32,
    pub reflex_bright: f32,
    pub reflex_fit: bool,
    pub show_scale_x: bool,
    pub show_scale_y: bool,
    pub note_labels: bool,
    pub show_fps: bool,
    /// 0 = the default 30 fps.
    #[serde(rename = "max_fps")]
    pub max_fps: u32,
    pub opacity: f32,
    /// Presets saved by the user (the four of Feishin are built in).
    pub presets: Vec<VisualizerPreset>,
    /// Show the visualizer at the bottom of the queue panel (Feishin's side panel).
    pub in_queue_panel: bool,
}

impl Default for Visualizer {
    fn default() -> Self {
        Self {
            mode: 10,
            gradient: "prism".into(),
            gradient_left: None,
            gradient_right: None,
            custom_gradients: Vec::new(),
            color_mode: ColorMode::Gradient,
            split_gradient: false,
            channel_layout: ChannelLayout::Single,
            fft_size: 16384,
            min_freq: 20.0,
            max_freq: 22050.0,
            min_db: -85.0,
            max_db: -25.0,
            smoothing: 0.6,
            frequency_scale: FrequencyScale::Log,
            weighting_filter: WeightingFilter::None,
            linear_amplitude: false,
            linear_boost: 4.0,
            volume: 1.0,
            peak_line: true,
            show_peaks: false,
            fade_peaks: true,
            gravity: 11.0,
            peak_hold_time: 500,
            peak_fade_time: 900,
            line_width: 1.9,
            fill_alpha: 0.0,
            bar_space: 0.7,
            alpha_bars: false,
            led_bars: false,
            lumi_bars: false,
            outline_bars: false,
            round_bars: false,
            true_leds: false,
            lo_res: false,
            ansi_bands: false,
            mirror: 0,
            radial: false,
            radial_invert: false,
            radius: 0.7,
            spin_speed: 0.0,
            reflex_ratio: 0.5,
            reflex_alpha: 0.1,
            reflex_bright: 1.0,
            reflex_fit: false,
            show_scale_x: false,
            show_scale_y: false,
            note_labels: false,
            show_fps: false,
            max_fps: 0,
            opacity: 1.0,
            presets: Vec::new(),
            in_queue_panel: true,
        }
    }
}

/// Feishin's four built-in presets: `(name, JSON)`.
pub fn builtin_presets() -> Vec<VisualizerPreset> {
    #[derive(Deserialize)]
    struct Entry {
        name: String,
        value: serde_json::Value,
    }
    serde_json::from_str::<Vec<Entry>>(include_str!("visualizer_presets.json"))
        .unwrap_or_default()
        .into_iter()
        .map(|e| VisualizerPreset { name: e.name, value: e.value.to_string() })
        .collect()
}

/// Settings that are not part of a preset or a copied configuration.
const NOT_CONFIG: [&str; 2] = ["presets", "in_queue_panel"];

/// `fft_size` → `fftSize`; `max_fps` → `maxFPS`; `min_db` → `minDecibels`, ….
fn camel(snake: &str) -> String {
    match snake {
        "max_fps" => return "maxFPS".into(),
        "show_fps" => return "showFPS".into(),
        "min_db" => return "minDecibels".into(),
        "max_db" => return "maxDecibels".into(),
        _ => {}
    }
    let mut out = String::new();
    let mut up = false;
    for c in snake.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn snake(camel: &str) -> String {
    match camel {
        "maxFPS" => return "max_fps".into(),
        "showFPS" => return "show_fps".into(),
        "minDecibels" => return "min_db".into(),
        "maxDecibels" => return "max_db".into(),
        _ => {}
    }
    let mut out = String::new();
    for c in camel.chars() {
        if c.is_uppercase() {
            out.push('_');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Visualizer {
    /// The built-in presets followed by the user's.
    pub fn all_presets(&self) -> Vec<VisualizerPreset> {
        let mut all = builtin_presets();
        all.extend(self.presets.iter().cloned());
        all
    }

    /// The options as Feishin's JSON (camelCase), including the custom
    /// gradients, without presets.
    pub fn config_json(&self) -> serde_json::Value {
        let serde_json::Value::Object(map) = serde_json::to_value(self).unwrap_or_default() else {
            return serde_json::Value::Null;
        };
        map.into_iter()
            .filter(|(k, v)| !NOT_CONFIG.contains(&k.as_str()) && !v.is_null())
            .map(|(k, v)| (camel(&k), v))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    /// Applies a Feishin-style JSON object (any subset of the options, as
    /// from [`Self::config_json`] or Feishin's “Copy configuration”).
    /// Unknown keys are ignored; on an error nothing changes.
    pub fn apply_json(&mut self, json: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(incoming) = value else {
            return Err("not a JSON object".into());
        };
        let mut merged = match serde_json::to_value(&*self) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => return Err("cannot read the current options".into()),
        };
        for (k, v) in incoming {
            let key = snake(&k);
            if !NOT_CONFIG.contains(&key.as_str()) && !v.is_null() && merged.contains_key(&key) {
                merged.insert(key, v);
            }
        }
        let mut next: Visualizer = serde_json::from_value(serde_json::Value::Object(merged))
            .map_err(|e| format!("invalid value: {e}"))?;
        next.sanitize();
        *self = next;
        Ok(())
    }

    /// Puts out-of-range values back into audioMotion's ranges.
    pub fn sanitize(&mut self) {
        if !(self.mode <= 8 || self.mode == 10) {
            self.mode = 10;
        }
        self.fft_size = self.fft_size.clamp(1024, 32768).next_power_of_two();
        self.min_freq = self.min_freq.clamp(1.0, 22049.0);
        self.max_freq = self.max_freq.clamp(self.min_freq + 1.0, 22050.0);
        self.min_db = self.min_db.clamp(-120.0, -10.0);
        self.max_db = self.max_db.clamp(self.min_db + 1.0, 0.0);
        self.smoothing = self.smoothing.clamp(0.0, 0.99);
        self.linear_boost = self.linear_boost.clamp(1.0, 5.0);
        self.volume = self.volume.clamp(0.0, 4.0);
        self.fill_alpha = self.fill_alpha.clamp(0.0, 1.0);
        self.line_width = self.line_width.clamp(0.0, 10.0);
        self.mirror = self.mirror.clamp(-1, 1);
        self.radius = self.radius.clamp(0.1, 1.0);
        self.reflex_ratio = self.reflex_ratio.clamp(0.0, 0.9);
        self.reflex_alpha = self.reflex_alpha.clamp(0.0, 1.0);
        self.reflex_bright = self.reflex_bright.clamp(0.0, 2.0);
        self.opacity = self.opacity.clamp(0.0, 1.0);
        self.max_fps = self.max_fps.min(240);
        self.gravity = self.gravity.clamp(0.1, 100.0);
    }

    /// Saves the current options as preset `name`; an existing one of that
    /// name is replaced. Returns false for an empty name.
    pub fn save_preset(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let value = self.config_json().to_string();
        match self.presets.iter_mut().find(|p| p.name == name) {
            Some(p) => p.value = value,
            None => self.presets.push(VisualizerPreset { name: name.to_string(), value }),
        }
        true
    }

    /// Applies preset `name` (user presets win over the built-in ones).
    pub fn load_preset(&mut self, name: &str) -> Result<(), String> {
        let preset = self
            .presets
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .or_else(|| builtin_presets().into_iter().find(|p| p.name == name))
            .ok_or_else(|| format!("no preset “{name}”"))?;
        self.apply_json(&preset.value)
    }

    /// Deletes the user preset `name`.
    pub fn delete_preset(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| p.name != name);
        self.presets.len() != before
    }

    /// The custom gradient called `name`.
    pub fn custom_gradient(&self, name: &str) -> Option<&CustomGradient> {
        self.custom_gradients.iter().find(|g| g.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_configs_without_the_new_fields_load() {
        let v: Visualizer = toml::from_str("mode = 4\ngradient = \"rainbow\"\nsmoothing = 0.3\n").unwrap();
        assert_eq!((v.mode, v.gradient.as_str(), v.smoothing), (4, "rainbow", 0.3));
        assert_eq!(v.bar_space, 0.7);
        assert_eq!(v.channel_layout, ChannelLayout::Single);
        assert!(v.in_queue_panel);
    }

    #[test]
    fn serde_round_trip_with_presets_and_gradients() {
        let mut v = Visualizer {
            mode: 3,
            led_bars: true,
            channel_layout: ChannelLayout::DualVertical,
            frequency_scale: FrequencyScale::Mel,
            weighting_filter: WeightingFilter::A,
            color_mode: ColorMode::BarLevel,
            gradient_left: Some("rainbow".into()),
            ..Visualizer::default()
        };
        v.custom_gradients.push(CustomGradient {
            name: "sunset".into(),
            dir: "h".into(),
            color_stops: vec![
                GradientStop { color: "#f00".into(), pos: Some(0.0), level: None },
                GradientStop { color: "#00f".into(), pos: None, level: Some(0.4) },
            ],
        });
        assert!(v.save_preset("mine"));
        let text = toml::to_string(&v).unwrap();
        let back: Visualizer = toml::from_str(&text).unwrap();
        assert_eq!(back, v);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<Visualizer>(&json).unwrap(), v);
    }

    #[test]
    fn config_json_uses_feishins_names() {
        let json = Visualizer::default().config_json();
        for key in [
            "minDecibels",
            "maxDecibels",
            "maxFPS",
            "showFPS",
            "fftSize",
            "weightingFilter",
            "frequencyScale",
            "peakLine",
            "reflexRatio",
            "channelLayout",
            "colorMode",
            "customGradients",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert!(json.get("presets").is_none() && json.get("inQueuePanel").is_none());
        assert_eq!(json["channelLayout"], "single");
        assert_eq!(json["weightingFilter"], "");
    }

    #[test]
    fn feishin_configs_apply_and_round_trip() {
        let feishin = r#"{"alphaBars":true,"barSpace":0,"channelLayout":"dual-horizontal","colorMode":"bar-index",
            "fftSize":4096,"frequencyScale":"bark","gradient":"steelblue","gradientLeft":"rainbow","ledBars":true,
            "maxDecibels":-30,"minDecibels":-90,"maxFPS":60,"mode":6,"weightingFilter":"D","mirror":-1,
            "somethingNew":1,"presets":[]}"#;
        let mut v = Visualizer::default();
        v.apply_json(feishin).unwrap();
        assert_eq!(v.mode, 6);
        assert!(v.alpha_bars && v.led_bars);
        assert_eq!(v.channel_layout, ChannelLayout::DualHorizontal);
        assert_eq!(v.color_mode, ColorMode::BarIndex);
        assert_eq!((v.fft_size, v.max_fps, v.mirror), (4096, 60, -1));
        assert_eq!((v.min_db, v.max_db), (-90.0, -30.0));
        assert_eq!(v.frequency_scale, FrequencyScale::Bark);
        assert_eq!(v.weighting_filter, WeightingFilter::D);
        assert_eq!(v.gradient_left.as_deref(), Some("rainbow"));
        // Copy → paste gives the same options.
        let mut other = Visualizer::default();
        other.apply_json(&v.config_json().to_string()).unwrap();
        assert_eq!(other, v);
    }

    #[test]
    fn bad_input_changes_nothing() {
        let mut v = Visualizer::default();
        assert!(v.apply_json("not json").is_err());
        assert!(v.apply_json("[1]").is_err());
        assert!(v.apply_json(r#"{"mode":"loud"}"#).is_err());
        assert_eq!(v, Visualizer::default());
        v.apply_json(r#"{"mode":9,"smoothing":5,"fftSize":3000,"volume":-2}"#).unwrap();
        assert_eq!(v.mode, 10, "mode 9 doesn't exist");
        assert_eq!((v.smoothing, v.fft_size, v.volume), (0.99, 4096, 0.0));
    }

    #[test]
    fn presets_save_load_and_delete() {
        let builtin = builtin_presets();
        assert_eq!(
            builtin.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["Preset 1", "Preset 2", "Preset 3", "Preset 4"]
        );
        let mut v = Visualizer::default();
        v.load_preset("Preset 3").unwrap();
        assert_eq!((v.mode, v.fft_size, v.reflex_ratio), (8, 4096, 0.0));
        assert_eq!(v.gradient_left.as_deref(), Some("rainbow"));
        v.load_preset("Preset 4").unwrap();
        assert_eq!(v.channel_layout, ChannelLayout::DualCombined);
        assert_eq!(v.weighting_filter, WeightingFilter::D);
        // Saving keeps the options; changing and loading restores them.
        v.mode = 5;
        assert!(v.save_preset("  five  "));
        assert!(!v.save_preset("   "));
        v.mode = 1;
        v.load_preset("five").unwrap();
        assert_eq!(v.mode, 5);
        // Saving under the same name replaces.
        v.mode = 2;
        v.save_preset("five");
        assert_eq!(v.presets.len(), 1);
        assert_eq!(v.all_presets().len(), 5);
        assert!(v.delete_preset("five") && !v.delete_preset("five"));
        assert!(v.load_preset("five").is_err());
    }
}
