//! Audio output and the player-config popover: opening and closing it, the
//! output-device menu of the volume button, the speed rules around DJ
//! transitions, and the Playback tab's audio rows.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::audio::{AudioMsg, Popup};
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        ephemeral: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    s.queue.set(listen::demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

/// Clicks the widget with `text` and feeds the messages back.
fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|_| panic!("{text} is on screen"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn audio(s: &mut Listen, msg: AudioMsg) {
    let _ = s.update(Message::Audio(msg));
}

#[test]
fn popover_opens_and_closes() {
    let mut s = state();
    assert!(!has_text(&s, "Playback speed"));
    audio(&mut s, AudioMsg::TogglePopover);
    assert_eq!(s.audio.popup, Some(Popup::Config));
    for text in ["Audio player", "Audio device", "Playback style", "Playback speed", "Preserve pitch"] {
        assert!(has_text(&s, text), "{text}");
    }
    // The toggle button closes it again, as does the backdrop's message.
    audio(&mut s, AudioMsg::TogglePopover);
    assert_eq!(s.audio.popup, None);
    audio(&mut s, AudioMsg::TogglePopover);
    audio(&mut s, AudioMsg::ClosePopup);
    assert_eq!(s.audio.popup, None);
    assert!(!has_text(&s, "Playback speed"));
}

#[test]
fn crossfade_is_shown_disabled_with_feishins_reason() {
    let mut s = state();
    audio(&mut s, AudioMsg::TogglePopover);
    assert!(has_text(&s, "Crossfade"));
    assert!(has_text(&s, "Gapless"));
    // Clicking the disabled segment does nothing.
    assert!(click_text(&mut s, "Crossfade").is_empty());
    assert_eq!(s.config.settings.playback.gapless, "weak");
}

#[test]
fn volume_right_click_opens_the_device_menu_and_a_pick_switches_the_device() {
    let mut s = state();
    assert_eq!(s.config.settings.playback.audio_device, "auto");
    // The right button goes down at the volume icon: the position first, then the menu.
    let at = iced::Point::new(1400.0, 700.0);
    let _ = s.update(Message::RightClickAt(at));
    audio(&mut s, AudioMsg::OpenDeviceMenu);
    assert_eq!(s.audio.popup, Some(Popup::Devices(at)));
    assert!(!s.audio.devices.is_empty(), "the demo has devices");
    assert!(has_text(&s, "Audio output"));
    assert!(has_text(&s, "System default"));
    assert!(has_text(&s, "WH-1000XM5"));
    click_text(&mut s, "WH-1000XM5");
    assert_eq!(s.config.settings.playback.audio_device, "pipewire/bluez_output.headphones");
    assert_eq!(s.audio.popup, None, "the menu closes after a pick");
    // And back to the system default.
    audio(&mut s, AudioMsg::OpenDeviceMenu);
    click_text(&mut s, "System default");
    assert_eq!(s.config.settings.playback.audio_device, "auto");
}

#[test]
fn right_click_on_the_volume_icon_sends_the_menu_message() {
    let s = state();
    // The volume icon is the left end of the bar's right column; scan the bar for it.
    let size = s.window_size;
    let _gpu = gpu();
    let mut found = false;
    'scan: for x in (1000..1560).step_by(8) {
        let mut ui = Simulator::with_size(settings(), size, s.view());
        ui.point_at((x as f32, size.height - 45.0));
        let _ = ui.simulate([iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Right))]);
        if ui.into_messages().any(|m| matches!(m, Message::Audio(AudioMsg::OpenDeviceMenu))) {
            found = true;
            break 'scan;
        }
    }
    assert!(found, "a right-click on the volume icon opens the device menu");
}

#[test]
fn speed_is_clamped_and_resets() {
    let mut s = state();
    audio(&mut s, AudioMsg::SetSpeed(1.234));
    assert!((s.audio.speed - 1.23).abs() < 1e-6);
    audio(&mut s, AudioMsg::SetSpeed(9.0));
    assert_eq!(s.audio.speed, 2.0);
    audio(&mut s, AudioMsg::SetSpeed(0.1));
    assert_eq!(s.audio.speed, 0.5);
    audio(&mut s, AudioMsg::PitchStep(-12.0));
    assert_eq!(s.audio.speed, 0.5);
    audio(&mut s, AudioMsg::SetSpeed(1.5));
    audio(&mut s, AudioMsg::TogglePopover);
    click_text(&mut s, "1x");
    assert_eq!(s.audio.speed, 1.0);
}

#[test]
fn pitch_buttons_show_only_without_preserve_pitch() {
    let mut s = state();
    audio(&mut s, AudioMsg::TogglePopover);
    assert!(!has_text(&s, "+1st"));
    audio(&mut s, AudioMsg::PreservePitch(false));
    assert!(!s.config.settings.playback.preserve_pitch);
    assert!(has_text(&s, "+1st") && has_text(&s, "-1st"));
    assert!(!has_text(&s, "+10ct"));
    audio(&mut s, AudioMsg::Microtonal(true));
    assert!(has_text(&s, "+10ct") && has_text(&s, "-10ct"));
    click_text(&mut s, "+1st");
    assert!((s.audio.speed - 1.059_463).abs() < 1e-3, "{}", s.audio.speed);
}

#[test]
fn the_speed_control_rests_while_dj_transitions_are_possible() {
    // Demo with audio: the decks exist, so the DJ conditions count.
    let (mut s, _) = Listen::boot(&Args { demo: true, demo_audio: true, ephemeral: true, ..Args::default() });
    s.config.settings.mixing.enabled = false;
    audio(&mut s, AudioMsg::SetSpeed(1.5));
    assert_eq!(s.effective_speed(), 1.5);
    s.config.settings.mixing.enabled = true;
    s.config.settings.auto_dj.enabled = true;
    assert!(s.speed_locked());
    assert_eq!(s.effective_speed(), 1.0, "the decks run at 1.0 for the mix");
    audio(&mut s, AudioMsg::SetSpeed(1.25));
    assert_eq!(s.audio.speed, 1.5, "ignored while locked");
    // Without DJ transitions the remembered speed is back.
    s.config.settings.mixing.enabled = false;
    assert_eq!(s.effective_speed(), 1.5);
}

#[test]
fn playback_tab_rows_change_the_settings() {
    let mut s = state();
    audio(&mut s, AudioMsg::ReplayGainPreamp(3.0));
    audio(&mut s, AudioMsg::ReplayGainFallback(-6.0));
    audio(&mut s, AudioMsg::ReplayGainClip(false));
    audio(&mut s, AudioMsg::Exclusive(true));
    audio(&mut s, AudioMsg::SampleRateInput("4800a0".into()));
    audio(&mut s, AudioMsg::SampleRateSubmit);
    audio(&mut s, AudioMsg::MpvPath("/opt/mpv/bin/mpv".into()));
    let p = &s.config.settings.playback;
    assert_eq!((p.replay_gain_preamp_db, p.replay_gain_fallback_db, p.replay_gain_clip), (3.0, -6.0, false));
    assert!(p.audio_exclusive);
    assert_eq!(p.sample_rate_hz, 48_000);
    assert_eq!(p.mpv_path, "/opt/mpv/bin/mpv");
    // Below 8 kHz is "default".
    audio(&mut s, AudioMsg::SampleRateInput("100".into()));
    audio(&mut s, AudioMsg::SampleRateSubmit);
    assert_eq!(s.config.settings.playback.sample_rate_hz, 0);
}

#[test]
fn equalizer_and_compressor_presets() {
    let mut s = state();
    audio(&mut s, AudioMsg::EqPreset("Rock".into()));
    let p = &s.config.settings.playback;
    assert_eq!(p.equalizer_preset, "Rock");
    assert_eq!(p.equalizer[0].gain, 3.0);
    audio(&mut s, AudioMsg::EqPresetName("Mine".into()));
    audio(&mut s, AudioMsg::EqSavePreset);
    assert!(s.config.settings.playback.custom_eq_presets.contains_key("Mine"));
    audio(&mut s, AudioMsg::EqReset);
    assert!(s.config.settings.playback.equalizer.iter().all(|b| b.gain == 0.0));
    audio(&mut s, AudioMsg::EqPreset("Mine".into()));
    assert_eq!(s.config.settings.playback.equalizer[0].gain, 3.0);
    audio(&mut s, AudioMsg::EqDeletePreset);
    assert!(s.config.settings.playback.custom_eq_presets.is_empty());

    audio(&mut s, AudioMsg::CompPreset("Limiter".into()));
    audio(&mut s, AudioMsg::CompEnabled(true));
    let c = s.config.settings.playback.compressor;
    assert!(c.enabled && c.params.ratio == 20.0 && c.params.threshold == -3.0);
    audio(&mut s, AudioMsg::CompParam(listen::app::audio::CompParam::Ratio, 6.0));
    assert_eq!(s.config.settings.playback.compressor.params.ratio, 6.0);
    assert!(s.config.settings.playback.compressor_preset.is_empty(), "tweaking leaves the preset");
}

#[test]
fn transcoding_section_is_shown_unless_only_youtube_music_is_set_up() {
    let mut s = state();
    assert!(s.transcoding_available(), "demo");
    audio(&mut s, AudioMsg::TranscodeEnabled(true));
    audio(&mut s, AudioMsg::TranscodeFormat("opus".into()));
    audio(&mut s, AudioMsg::TranscodeBitrate("128 kbit".into()));
    audio(&mut s, AudioMsg::TranscodeSampleRate("48000".into()));
    let t = &s.config.settings.playback.transcode;
    assert!(t.enabled && t.format == "opus" && t.bitrate == 128 && t.max_sample_rate == 48_000);
}

#[test]
fn mpv_extra_parameters_are_edited_line_by_line() {
    use iced::widget::text_editor::{Action, Edit};
    let mut s = state();
    let _ = s.update(Message::Audio(AudioMsg::MpvArgs(Action::Edit(Edit::Paste(
        "--audio-channels=stereo\n\ngapless-audio=yes".to_string().into(),
    )))));
    assert_eq!(s.config.settings.playback.mpv_extra_args, ["--audio-channels=stereo", "--gapless-audio=yes"]);
}
