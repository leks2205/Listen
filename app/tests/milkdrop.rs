//! Milkdrop: library loader, preset folder, and real frames where libprojectM 4 and OpenGL exist.

use listen::milkdrop::{self, ffi, presets};

/// The render thread's requests are global: tests that make or watch them take turns.
static REQUESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/milkdrop")
}

#[test]
fn missing_library_is_reported_not_fatal() {
    let err = ffi::Api::load_from(&["/nonexistent/libprojectM-4.so.4", "libnot-projectm.so"])
        .err()
        .expect("must not load");
    assert!(err.contains("libprojectM 4 not found"), "{err}");
    assert!(ffi::Api::load_from::<&str>(&[]).is_err());
    // A library that exists but isn't projectM: the missing symbol is named.
    let libc = ["libc.so.6", "libc.dylib", "msvcrt.dll"];
    let wrong = ffi::Api::load_from(&libc).err().expect("libc is not projectM");
    assert!(wrong.contains("projectm_"), "{wrong}");
    // The hint shown in the settings when it is missing.
    if milkdrop::api().is_err() {
        assert_eq!(milkdrop::availability().unwrap_err(), "Milkdrop needs libprojectM 4");
        assert!(milkdrop::load_error().is_some());
    }
}

#[test]
fn fixture_folder_is_discovered_in_order() {
    let found = presets::scan(&fixtures());
    let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["a-red", "b-green", "sub/c-blue"]);
}

#[cfg(target_os = "linux")]
#[test]
fn real_library_renders_frames_and_cycles() {
    use listen::milkdrop::{Config, Out, engine, request_next, set_config};
    let _turn = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    engine::clear_requests();
    // EGL and a live Vulkan instance (the Simulators below) must not set up at the same time.
    let _gpu = gpu();
    use listen_core::settings::MilkdropSettings;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    if milkdrop::availability().is_err() {
        eprintln!("skipped: libprojectM 4 is not installed");
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Out>(8);
    let eng = engine::Engine::start(tx);
    set_config(Config {
        root: fixtures(),
        presets: Arc::new(presets::scan(&fixtures())),
        settings: MilkdropSettings {
            randomize: false,
            cycle_presets: false,
            blend_time: 0.0,
            ignored: vec![],
            ..MilkdropSettings::default()
        },
    });
    // A 440 Hz tone, stereo, 48 kHz.
    let tone = |n: usize| -> Vec<f32> {
        (0..n).flat_map(|i| [(i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.8; 2]).collect()
    };
    let (mut frames, mut presets_seen, mut lit) = (0, Vec::new(), false);
    let mut last: Option<Vec<u8>> = None;
    let mut failed: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && frames < 28 {
        eng.feed(tone(576));
        match rx.try_recv() {
            Ok(Out::Error(e)) => {
                eprintln!("skipped: {e}");
                return;
            }
            Ok(Out::Frame(rgba)) => {
                assert_eq!(rgba.len(), (engine::WIDTH * engine::HEIGHT * 4) as usize);
                frames += 1;
                last = Some(rgba.clone());
                lit |= rgba.as_chunks::<4>().0.iter().any(|p| p[0] > 60 || p[1] > 60 || p[2] > 60);
                if [4, 12, 20].contains(&frames) {
                    request_next();
                }
            }
            Ok(Out::Preset(p)) => {
                eprintln!("preset {p}");
                presets_seen.push(p)
            }
            Ok(Out::Failed(n, e)) => {
                eprintln!("failed {n}: {e}");
                failed.push(n)
            }
            Ok(Out::NoPresets) => eprintln!("no presets"),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    if let Some(dir) = std::env::var_os("LISTEN_MILKDROP_DUMP")
        && let Some(f) = &last
    {
        let img = image::RgbaImage::from_raw(engine::WIDTH as u32, engine::HEIGHT as u32, f.clone()).unwrap();
        img.save(PathBuf::from(dir).join("milkdrop-frame.png")).unwrap();
    }
    assert!(frames >= 28, "only {frames} frames");
    assert!(lit, "every frame was black");
    assert!(presets_seen.len() >= 2, "no preset change: {presets_seen:?}");
    // In order and wrapping (other tests may queue a "next" too, so no exact count).
    let order = ["a-red", "b-green", "sub/c-blue"];
    assert!(presets_seen.len() >= 4, "{presets_seen:?}");
    assert_eq!(presets_seen[0], "a-red");
    for w in presets_seen.windows(2) {
        let i = order.iter().position(|n| *n == w[0]).unwrap();
        assert_eq!(w[1], order[(i + 1) % 3], "{presets_seen:?}");
    }
    assert!(failed.is_empty(), "{failed:?}");
    eng.shutdown();

    // A preset projectM can't open is an error, not a crash or a silent no-op.
    let gl = listen::milkdrop::gl::Headless::new(64, 64).expect("EGL worked a moment ago");
    let mut pm = ffi::ProjectM::new(milkdrop::api().unwrap(), 64, 64, 30).expect("projectM");
    assert!(pm.load_preset(&fixtures().join("a-red.milk"), false).is_ok());
    assert!(pm.load_preset(&fixtures().join("does-not-exist.milk"), false).is_err());
    drop(pm);
    drop(gl);
}

// ---- the settings and the view ---------------------------------------------------

use iced_test::simulator::Simulator;
use listen::app::milkdrop::{MilkdropMsg, MilkdropSetting};
use listen::app::{INTER, Listen, Message, Setting};
use listen::{Args, FONTS};
use listen_core::settings::{SettingsTab, VisualizerKind};
use std::sync::Arc;

/// Vulkan instances race when several `Simulator`s live at once.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn boot() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.config.settings.settings_tab = SettingsTab::Playback;
    s.config.settings.milkdrop.preset_dir = fixtures().to_string_lossy().into_owned();
    s
}

fn view_has(s: &Listen, height: f32, text: &str) -> bool {
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings, iced::Size::new(1400.0, height), s.view());
    ui.find(text).is_ok()
}

fn scanned(s: &mut Listen) {
    let found = Arc::new(presets::scan(&fixtures()));
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Scanned(fixtures(), found)));
}

#[test]
fn settings_show_the_hint_or_the_options() {
    let mut s = boot();
    let _ = s.update(Message::Navigate(listen::app::Route::Settings));
    assert!(view_has(&s, 6000.0, "Milkdrop"), "the section is there either way");
    if milkdrop::availability().is_err() {
        assert!(view_has(&s, 6000.0, "Milkdrop needs libprojectM 4"));
        assert!(!view_has(&s, 6000.0, "Cycle time"), "no options without the library");
    } else {
        scanned(&mut s);
        let count = format!("3 presets in {}", fixtures().display());
        for t in ["Preset folder", "Cycle time", "Blend time", "Include all presets", "Max FPS", &count] {
            assert!(view_has(&s, 6000.0, t), "{t}");
        }
    }
}

#[test]
fn settings_changes_are_clamped_and_saved_to_the_state() {
    let _turn = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = boot();
    let set = |s: &mut Listen, c| {
        let _ = s.update(Message::Setting(Setting::Milkdrop(c)));
    };
    set(&mut s, MilkdropSetting::CycleTime(9999));
    set(&mut s, MilkdropSetting::BlendTime(2.46));
    set(&mut s, MilkdropSetting::MaxFps(500));
    set(&mut s, MilkdropSetting::Opacity(3.0));
    set(&mut s, MilkdropSetting::Randomize(false));
    set(&mut s, MilkdropSetting::IncludeAll(false));
    let m = &s.config.settings.milkdrop;
    assert_eq!((m.cycle_time, m.blend_time, m.max_fps, m.opacity), (300, 2.5, 144, 1.0));
    assert!(!m.randomize && !m.include_all);
    assert_eq!(m.fps(), 60, "capped like the spectrum");
    // The preset on screen can be selected and ignored.
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Preset("a-red".into())));
    set(&mut s, MilkdropSetting::ToggleSelected);
    assert_eq!(s.config.settings.milkdrop.selected, ["a-red"]);
    set(&mut s, MilkdropSetting::ToggleSelected);
    assert!(s.config.settings.milkdrop.selected.is_empty());
    set(&mut s, MilkdropSetting::IgnoreCurrent);
    set(&mut s, MilkdropSetting::IgnoreCurrent);
    assert_eq!(s.config.settings.milkdrop.ignored, ["a-red"], "once");
    set(&mut s, MilkdropSetting::ClearIgnored);
    assert!(s.config.settings.milkdrop.ignored.is_empty());
    assert_eq!(s.config.settings.milkdrop.current_preset.as_deref(), Some("a-red"), "remembered");
    #[cfg(target_os = "linux")]
    listen::milkdrop::engine::clear_requests(); // ignoring asked for the next preset
}

#[test]
fn old_settings_files_load_with_the_defaults() {
    let s: listen_core::settings::Settings = toml::from_str("[visualizer]\nmode = 3\n").unwrap();
    assert_eq!(s.milkdrop.kind, VisualizerKind::Spectrum);
    assert!(s.milkdrop.cycle_presets && s.milkdrop.randomize && s.milkdrop.include_all);
    assert_eq!((s.milkdrop.cycle_time, s.milkdrop.blend_time), (30, 2.5));
    let m: listen_core::settings::Settings =
        toml::from_str("[milkdrop]\nkind = \"milkdrop\"\ncycle_time = 12\n").unwrap();
    assert_eq!((m.milkdrop.kind, m.milkdrop.cycle_time), (VisualizerKind::Milkdrop, 12));
}

#[test]
fn milkdrop_is_only_active_where_it_can_run() {
    let mut s = boot();
    assert!(!s.milkdrop_active(), "spectrum is the default");
    let _ = s.update(Message::Setting(Setting::Milkdrop(MilkdropSetting::Kind(VisualizerKind::Milkdrop))));
    assert_eq!(s.config.settings.milkdrop.kind, VisualizerKind::Milkdrop);
    assert_eq!(s.milkdrop_active(), milkdrop::availability().is_ok());
}

#[test]
fn next_button_and_search_pick_send_messages() {
    if milkdrop::availability().is_err() {
        return;
    }
    let _turn = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = boot();
    let _ = s.update(Message::Navigate(listen::app::Route::Settings));
    scanned(&mut s);
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Query("blue".into())));
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings, iced::Size::new(1400.0, 6000.0), s.view());
        ui.click("sub/c-blue").expect("the search lists the match");
        ui.into_messages().collect()
    };
    assert!(
        messages.iter().any(|m| matches!(m, Message::Milkdrop(MilkdropMsg::Pick(n)) if n == "sub/c-blue"))
    );
    for m in messages {
        let _ = s.update(m);
    }
    assert_eq!(s.milk.preset.as_deref(), Some("sub/c-blue"));
    #[cfg(target_os = "linux")]
    listen::milkdrop::engine::clear_requests(); // the pick asked the render thread
}

#[test]
fn the_picture_shows_in_the_fullscreen_visualizer_with_the_preset_name() {
    if milkdrop::availability().is_err() {
        return;
    }
    let mut s = boot();
    let _ = s.update(Message::Setting(Setting::Milkdrop(MilkdropSetting::Kind(VisualizerKind::Milkdrop))));
    let _ = s.update(Message::OpenFullscreenTab(listen::app::FullscreenTab::Visualizer));
    // No picture yet: why.
    scanned(&mut s);
    assert!(view_has(&s, 900.0, "Starting Milkdrop…"));
    let frame = iced::widget::image::Handle::from_rgba(2, 2, vec![30u8; 16]);
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Frame(frame)));
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Preset("b-green".into())));
    assert!(view_has(&s, 900.0, "b-green"));
    assert!(!view_has(&s, 900.0, "Starting Milkdrop…"));
    // An empty folder says so.
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Error("no OpenGL here".into())));
    s.milk.frame = None;
    assert!(view_has(&s, 900.0, "no OpenGL here"));
}

#[test]
fn an_empty_preset_folder_is_explained() {
    if milkdrop::availability().is_err() {
        return;
    }
    let mut s = boot();
    let _ = s.update(Message::Setting(Setting::Milkdrop(MilkdropSetting::Kind(VisualizerKind::Milkdrop))));
    let _ = s.update(Message::OpenFullscreenTab(listen::app::FullscreenTab::Visualizer));
    let _ = s.update(Message::Milkdrop(MilkdropMsg::Scanned(fixtures(), Arc::new(Vec::new()))));
    assert!(s.milk.empty);
    let why = listen::ui::milkdrop::status(&s);
    assert!(why.starts_with("No Milkdrop presets (.milk) in "), "{why}");
    assert!(view_has(&s, 900.0, &why));
}
