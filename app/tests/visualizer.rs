//! The visualizer options modal, presets, and the waveform seek bar.

use iced_test::simulator::Simulator;
use listen::app::overlay::Modal;
use listen::app::viz::{MODAL, SliderSetting, VizMsg};
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS};
use listen_analysis::waveform::Waveform;
use listen_core::settings::{ChannelLayout, SliderType, WaveformAlign};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Vulkan instances race when several `Simulator`s live at once.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn boot(args: Args) -> Listen {
    Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..args }).0
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn run_events(s: &mut Listen, at: (f32, f32), events: impl IntoIterator<Item = iced::Event>) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.point_at(at);
        let _ = ui.simulate(events);
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn modal_open(s: &Listen) -> bool {
    matches!(s.overlay.modal, Some(Modal::Custom(k)) if k == MODAL)
}

#[test]
fn the_settings_page_opens_the_options_modal() {
    let mut s = boot(Args { route: Some("settings".into()), ..Args::default() });
    // A tall window shows the whole page.
    s.window_size = iced::Size::new(1600.0, 4096.0);
    let _ = s.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(
        listen_core::settings::SettingsTab::Playback,
    )));
    assert!(has_text(&s, "Visualizer settings"));
    let sent = click_text(&mut s, "Configure…");
    assert!(sent.iter().any(|m| matches!(m, Message::Viz(VizMsg::Open))), "{sent:?}");
    assert!(modal_open(&s));
    for text in ["Presets", "Select Preset", "Mode", "Gradient", "FFT Size", "Reflex Ratio", "Close"] {
        assert!(has_text(&s, text), "the modal shows {text:?}");
    }
}

#[test]
fn modal_buttons_save_and_load_presets_and_reset() {
    let mut s = boot(Args::default());
    let _ = s.update(Message::Viz(VizMsg::Open));
    assert!(modal_open(&s));
    let _ = s.update(Message::Viz(VizMsg::Set("mode", json!(6))));
    let _ = s.update(Message::Viz(VizMsg::Set("ledBars", json!(true))));
    let _ = s.update(Message::Viz(VizMsg::PresetName("My LEDs".into())));
    let sent = click_text(&mut s, "Save as Preset");
    assert!(sent.iter().any(|m| matches!(m, Message::Viz(VizMsg::PresetSave))));
    assert_eq!(s.config.settings.visualizer.presets.len(), 1);
    assert_eq!(s.viz.preset.as_deref(), Some("My LEDs"));
    // Changing things and loading the preset brings them back.
    let _ = s.update(Message::Viz(VizMsg::Set("mode", json!(2))));
    let _ = s.update(Message::Viz(VizMsg::Set("ledBars", json!(false))));
    let _ = s.update(Message::Viz(VizMsg::PresetPick("My LEDs".into())));
    assert_eq!((s.config.settings.visualizer.mode, s.config.settings.visualizer.led_bars), (6, true));
    // A built-in one, then back to the defaults.
    let _ = s.update(Message::Viz(VizMsg::PresetPick("Preset 4".into())));
    assert_eq!(s.config.settings.visualizer.channel_layout, ChannelLayout::DualCombined);
    let sent = click_text(&mut s, "Reset to default");
    assert!(sent.iter().any(|m| matches!(m, Message::Viz(VizMsg::Reset))));
    let v = &s.config.settings.visualizer;
    assert_eq!((v.mode, v.channel_layout, v.led_bars), (10, ChannelLayout::Single, false));
    assert_eq!(v.presets.len(), 1, "the saved presets survive a reset");
    // Built-in names can't be overwritten; delete removes a saved preset.
    let _ = s.update(Message::Viz(VizMsg::PresetName("Preset 1".into())));
    let _ = s.update(Message::Viz(VizMsg::PresetSave));
    assert_eq!(s.config.settings.visualizer.presets.len(), 1);
    let _ = s.update(Message::Viz(VizMsg::PresetPick("My LEDs".into())));
    let _ = s.update(Message::Viz(VizMsg::PresetDelete));
    assert!(s.config.settings.visualizer.presets.is_empty());
}

#[test]
fn pasted_configurations_apply_or_report() {
    let mut s = boot(Args::default());
    let _ = s.update(Message::Viz(VizMsg::Pasted(Some(
        r#"{"mode":4,"channelLayout":"dual-vertical","maxFPS":24}"#.into(),
    ))));
    let v = &s.config.settings.visualizer;
    assert_eq!((v.mode, v.channel_layout, v.max_fps), (4, ChannelLayout::DualVertical, 24));
    let before = v.clone();
    let _ = s.update(Message::Viz(VizMsg::Pasted(Some("{not json".into()))));
    let _ = s.update(Message::Viz(VizMsg::Pasted(None)));
    assert_eq!(s.config.settings.visualizer, before, "bad pastes change nothing");
}

#[test]
fn custom_gradients_can_be_edited() {
    let mut s = boot(Args::default());
    let _ = s.update(Message::Viz(VizMsg::GradientAdd));
    let _ = s.update(Message::Viz(VizMsg::GradientName(0, "Fire".into())));
    let _ = s.update(Message::Viz(VizMsg::StopColor(0, 1, "#ff8800".into())));
    let _ = s.update(Message::Viz(VizMsg::StopAdd(0)));
    let _ = s.update(Message::Viz(VizMsg::GradientHorizontal(0, true)));
    let _ = s.update(Message::Viz(VizMsg::Set("gradient", json!("Fire"))));
    let v = &s.config.settings.visualizer;
    assert_eq!(v.custom_gradients[0].name, "Fire");
    assert_eq!((v.custom_gradients[0].color_stops.len(), v.custom_gradients[0].dir.as_str()), (3, "h"));
    assert_eq!(v.gradient, "Fire");
    // Renaming follows the selection; removing falls back to prism.
    let _ = s.update(Message::Viz(VizMsg::GradientName(0, "Flame".into())));
    assert_eq!(s.config.settings.visualizer.gradient, "Flame");
    let _ = s.update(Message::Viz(VizMsg::GradientRemove(0)));
    assert_eq!(s.config.settings.visualizer.gradient, "prism");
    // The last colour can't be removed.
    let _ = s.update(Message::Viz(VizMsg::GradientAdd));
    let _ = s.update(Message::Viz(VizMsg::StopRemove(0, 0)));
    let _ = s.update(Message::Viz(VizMsg::StopRemove(0, 0)));
    assert_eq!(s.config.settings.visualizer.custom_gradients[0].color_stops.len(), 1);
}

fn wave() -> Arc<Waveform> {
    let peaks = (0..1000).map(|i| (60.0 + 190.0 * ((i as f32 * 0.07).sin().abs())) as u8).collect();
    Arc::new(Waveform { peaks, duration_ms: 200_000 })
}

fn waveform_state() -> Listen {
    let mut s = boot(Args::default());
    s.window_size = iced::Size::new(1200.0, 500.0);
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(SliderType::Waveform))));
    s.viz.waveform.data = Some(wave());
    let _ = s.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(Duration::from_secs(50))),
    ));
    s
}

#[test]
fn the_seek_bar_is_a_waveform_only_when_chosen_and_loaded() {
    let mut s = waveform_state();
    let (pos, dur) = (Duration::from_secs(50), Duration::from_secs(200));
    assert!(listen::ui::waveform::seek_bar(&s, pos, dur).is_some());
    s.viz.waveform.data = None;
    assert!(listen::ui::waveform::seek_bar(&s, pos, dur).is_none(), "still loading: the plain slider");
    s.viz.waveform.data = Some(wave());
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(SliderType::Slider))));
    assert!(listen::ui::waveform::seek_bar(&s, pos, dur).is_none(), "the setting is off");
}

#[test]
fn changing_the_slider_type_follows_the_song() {
    let mut s = boot(Args::default());
    assert!(s.viz.waveform.key.is_none(), "nothing computed for the plain slider");
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(SliderType::Waveform))));
    assert!(s.viz.waveform.key.is_some(), "the playing song is tracked");
    assert!(s.viz.waveform.data.is_none() && s.viz.waveform.no_input, "demo songs have no audio to read");
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::Align(WaveformAlign::Bottom))));
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::BarWidth(500))));
    let pb = &s.config.settings.player_bar;
    assert_eq!((pb.waveform_align, pb.waveform_bar_width), (WaveformAlign::Bottom, 20), "clamped");
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(SliderType::Slider))));
    assert!(s.viz.waveform.key.is_none() && s.viz.waveform.data.is_none());
}

#[test]
fn a_late_waveform_is_dropped_for_another_song() {
    let mut s = boot(Args::default());
    let _ = s.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(SliderType::Waveform))));
    let key = s.viz.waveform.key.clone().unwrap();
    let _ = s.update(Message::Viz(VizMsg::WaveformReady("some-other-song".into(), Some(wave()))));
    assert!(s.viz.waveform.data.is_none());
    let _ = s.update(Message::Viz(VizMsg::WaveformReady(key, Some(wave()))));
    assert!(s.viz.waveform.data.is_some());
}

#[test]
fn clicking_the_waveform_seeks() {
    use iced::mouse::{Button, Event as Mouse};
    let mut s = waveform_state();
    let (w, h) = (s.window_size.width, s.window_size.height);
    // Find the bar by probing heights above the bottom edge, at the window's centre.
    let press = [iced::Event::Mouse(Mouse::ButtonPressed(Button::Left))];
    let mut found = None;
    for y in (0..80).step_by(3).map(|d| h - 8.0 - d as f32) {
        let sent = run_events(&mut s, (w / 2.0, y), press.clone());
        if let Some(Message::SeekPreview(secs)) = sent.iter().find(|m| matches!(m, Message::SeekPreview(_))) {
            found = Some((y, *secs));
            break;
        }
    }
    let (y, middle) = found.expect("a click on the bar seeks");
    assert!(middle > 40.0 && middle < 160.0, "mid-bar is mid-song, got {middle}");
    // Release sends the seek.
    let _ = run_events(&mut s, (w / 2.0, y), [iced::Event::Mouse(Mouse::ButtonReleased(Button::Left))]);
    // Further left is earlier.
    let _ = s.update(Message::SeekRelease);
    let mut earlier = None;
    for dx in [40.0, 70.0, 100.0, 130.0] {
        let sent = run_events(&mut s, (w / 2.0 - dx, y), press.clone());
        earlier = sent.iter().find_map(|m| if let Message::SeekPreview(x) = m { Some(*x) } else { None });
        let _ = s.update(Message::SeekRelease);
        if earlier.is_some() {
            break;
        }
    }
    assert!(earlier.is_some_and(|e| e < middle), "{earlier:?} vs {middle}");
}
