//! Appearance: accent, primary shade, custom themes + overrides, font, zoom,
//! backgrounds and cover options.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::appearance::AppearanceMsg as A;
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};
use listen_core::settings::{FontKind, ImagePlaceholder};
use listen_core::theme::Rgb;
use listen_core::theme::custom::{self, Loaded};
use std::sync::Arc;

/// The UI font is process-wide; every test here boots Listen, which sets it.
/// Run them one at a time so a parallel boot can't reset the font mid-test.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn boot() -> Listen {
    let (s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
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
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 3000.0), s.view());
    ui.find(text).is_ok()
}

fn send(s: &mut Listen, m: A) {
    let _ = s.update(Message::Appearance(m));
}

static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn themes_dir(files: &[(&str, &str)]) -> Loaded {
    let dir = std::env::temp_dir().join(format!(
        "listen-appearance-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let loaded = custom::load_dir(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    loaded
}

#[test]
fn zoom_steps_by_five_and_stays_in_range() {
    let _serial = serial();
    let mut s = boot();
    assert!((s.ui_scale(iced::window::Id::unique()) - 1.0).abs() < 1e-6);
    send(&mut s, A::ZoomIn);
    assert_eq!(s.config.settings.appearance.zoom, 105);
    send(&mut s, A::ZoomOut);
    send(&mut s, A::ZoomOut);
    assert_eq!(s.config.settings.appearance.zoom, 95);
    send(&mut s, A::ZoomSet(10_000));
    assert_eq!(s.config.settings.appearance.zoom, 300);
    send(&mut s, A::ZoomIn);
    assert_eq!(s.config.settings.appearance.zoom, 300);
    send(&mut s, A::ZoomSet(1));
    assert_eq!(s.config.settings.appearance.zoom, 50);
    send(&mut s, A::ZoomOut);
    assert_eq!(s.config.settings.appearance.zoom, 50);
    send(&mut s, A::ZoomSet(125));
    assert!((s.ui_scale(iced::window::Id::unique()) - 1.25).abs() < 1e-6);
    // The slider only previews until it is released.
    send(&mut s, A::ZoomDraft(200));
    assert_eq!(s.config.settings.appearance.zoom, 125);
    assert_eq!(s.appearance.zoom_draft, Some(200));
    send(&mut s, A::ZoomCommit);
    assert_eq!(s.config.settings.appearance.zoom, 200);
    assert_eq!(s.appearance.zoom_draft, None);
    send(&mut s, A::ZoomReset);
    assert_eq!(s.config.settings.appearance.zoom, 100);
}

#[test]
fn zoom_changes_the_compact_breakpoint_through_the_window_size() {
    let _serial = serial();
    // iced reports the logical size (physical / scale); the layout follows it.
    let mut s = boot();
    send(&mut s, A::ZoomSet(200));
    let _ = s.update(Message::WindowResized(iced::Size::new(700.0, 600.0)));
    assert_eq!(s.layout_mode(), listen::ui::LayoutMode::Compact);
}

#[test]
fn accent_swatch_and_text_change_the_primary_colour() {
    let _serial = serial();
    let mut s = boot();
    s.config.settings.appearance.use_theme_accent = false;
    let before = s.tokens.primary;
    send(&mut s, A::AccentSwatch(Rgb(240, 170, 22)));
    assert_ne!(s.tokens.primary, before);
    assert_eq!(s.config.settings.appearance.accent, Some(Rgb(240, 170, 22)));
    assert_eq!(s.accent_input, "rgb(240, 170, 22)");
    send(&mut s, A::AccentText("#1db954".into()));
    assert_eq!(s.config.settings.appearance.accent, Some(Rgb(29, 185, 84)));
    // A half-typed colour keeps the last good accent but stays in the field.
    send(&mut s, A::AccentText("#1db".into()));
    assert_eq!(s.accent_input, "#1db");
    send(&mut s, A::AccentText("#1d".into()));
    assert_eq!(s.config.settings.appearance.accent, Some(Rgb(0x11, 0xdd, 0xbb)));
}

#[test]
fn primary_shade_picks_the_shade() {
    let _serial = serial();
    let mut s = boot();
    s.config.settings.appearance.use_theme_accent = false;
    send(&mut s, A::AccentSwatch(Rgb(53, 116, 252)));
    let theme_default = s.tokens.primary;
    send(&mut s, A::UsePrimaryShade(false));
    send(&mut s, A::PrimaryShade(0));
    let lightest = s.tokens.primary;
    send(&mut s, A::PrimaryShade(9));
    let darkest = s.tokens.primary;
    assert_ne!(lightest, darkest);
    assert!(lightest.r + lightest.g + lightest.b > darkest.r + darkest.g + darkest.b);
    assert_eq!(s.tokens.primary_shade, s.tokens.primary, "the override applies to links too");
    send(&mut s, A::UsePrimaryShade(true));
    assert_eq!(s.tokens.primary, theme_default);
}

#[test]
fn custom_theme_and_override_file_apply() {
    let _serial = serial();
    let loaded = themes_dir(&[
        (
            "sunset.json",
            r##"{"extends":"tokyoNight","colors":{"background":"#201010","primary":"#ff8800"}}"##,
        ),
        (
            "overrides.toml",
            "[colors]\nforeground = \"#fafafa\"\n[radius]\nmd = 11\n[font_size]\nmd = 15\n[spacing]\nlg = 20\n",
        ),
    ]);
    let mut s = boot();
    s.config.settings.appearance.use_theme_accent = true;
    send(&mut s, A::ThemesChanged(Arc::new(loaded.clone())));
    // Not selected yet: the built-in theme is untouched, but overrides apply.
    assert_eq!(s.tokens.radius_md, 11.0);
    assert_eq!(s.tokens.font(listen::style::Scale::Md), 15.0);
    assert_eq!(s.tokens.space(listen::style::Scale::Lg), 20.0);
    assert_eq!(s.tokens.space(listen::style::Scale::Sm), 8.0, "unset values keep the default");
    assert_eq!(s.tokens.foreground, iced::Color::from_rgb8(0xfa, 0xfa, 0xfa));
    s.config.settings.appearance.follow_desktop_theme = false;
    s.config.settings.appearance.follow_system_theme = false;
    s.config.settings.appearance.theme = "sunset".into();
    let _ = s.update(Message::Appearance(A::ThemesChanged(Arc::new(Loaded::default()))));
    assert_eq!(s.tokens.radius_md, 5.0, "the override file is gone");
    send(&mut s, A::ThemesChanged(Arc::new(loaded)));
    assert_eq!(s.tokens.background, iced::Color::from_rgb8(0x20, 0x10, 0x10));
    assert_eq!(s.tokens.radius_sm, 3.0);
    // The settings page lists it.
    let _ = s.update(Message::Navigate(Route::Settings));
    assert!(has_text(&s, "Custom themes folder"));
}

#[test]
fn theme_errors_and_warnings_are_listed() {
    let _serial = serial();
    let loaded =
        themes_dir(&[("broken.json", "{ nope"), ("odd.json", r##"{"colors":{"primary":"banana"}}"##)]);
    let error = format!("broken.json: {}", loaded.errors[0].message);
    let mut s = boot();
    send(&mut s, A::ThemesChanged(Arc::new(loaded)));
    let _ = s.update(Message::Navigate(Route::Settings));
    assert!(has_text(&s, &error), "error line");
    assert!(has_text(&s, "odd: invalid colour for \"primary\" dropped: \"banana\""), "warning line");
}

#[test]
fn font_choice_round_trips_into_the_ui_font() {
    let _serial = serial();
    let mut s = boot();
    send(&mut s, A::FontFamily("Poppins".into()));
    assert_eq!(s.config.settings.appearance.font, "Poppins");
    assert_eq!(listen::appearance::current_font(), listen::appearance::family("Poppins"));
    send(&mut s, A::FontKind(FontKind::System));
    send(&mut s, A::FontFamily("DejaVu Sans".into()));
    assert_eq!(listen::appearance::current_font(), listen::appearance::family("DejaVu Sans"));
    // A bad file is reported, not applied.
    send(&mut s, A::FontPathInput("/definitely/not/here.ttf".into()));
    send(&mut s, A::FontPathApply);
    assert!(s.appearance.font_error.is_some());
    assert_eq!(s.config.settings.appearance.font, "DejaVu Sans");
    // A real file becomes the custom font.
    let path = std::env::temp_dir().join(format!("listen-font-test-{}.ttf", std::process::id()));
    std::fs::write(&path, include_bytes!("../../assets/fonts/Poppins-Medium.ttf")).unwrap();
    send(&mut s, A::FontPathInput(path.to_string_lossy().into()));
    send(&mut s, A::FontPathApply);
    std::fs::remove_file(&path).unwrap();
    assert!(s.appearance.font_error.is_none());
    assert_eq!(s.config.settings.appearance.font_kind, FontKind::Custom);
    assert_eq!(s.config.settings.appearance.font, "Poppins");
    // The settings survive a config round trip.
    let text = toml::to_string(&s.config.settings.appearance).unwrap();
    let back: listen_core::settings::Appearance = toml::from_str(&text).unwrap();
    assert_eq!(back, s.config.settings.appearance);
    send(&mut s, A::FontKind(FontKind::Builtin));
    send(&mut s, A::FontFamily("Inter".into()));
    assert_eq!(listen::appearance::current_font(), INTER);
}

#[test]
fn background_blur_and_cover_options_are_stored() {
    let _serial = serial();
    let mut s = boot();
    send(&mut s, A::AlbumBlur(9.0));
    assert_eq!(s.config.settings.appearance.album_background_blur, 6.0);
    send(&mut s, A::ArtistBlur(0.5));
    assert_eq!(s.config.settings.appearance.artist_background_blur, 0.5);
    send(&mut s, A::AlbumBackground(false));
    assert!(!s.background_enabled(listen::app::appearance::BlurKind::Album));
    send(&mut s, A::ArtistBackground(false));
    assert!(!s.background_enabled(listen::app::appearance::BlurKind::Artist));
    send(&mut s, A::NativeAspect(true));
    send(&mut s, A::BlurExplicit(true));
    send(&mut s, A::Placeholder(ImagePlaceholder::Off));
    let a = &s.config.settings.appearance;
    assert!(a.native_cover_aspect && a.blur_explicit_images);
    assert_eq!(a.image_placeholder, ImagePlaceholder::Off);
}

#[test]
fn explicit_covers_get_a_blurred_stand_in() {
    let _serial = serial();
    let mut s = boot();
    let png = {
        let mut img = image::RgbaImage::new(64, 64);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba([if x < 32 { 255 } else { 0 }, 0, 200, 255]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    assert!(listen::images::blur_cover(&png).is_some());
    assert!(listen::images::blur_cover(b"junk").is_none());
    send(&mut s, A::BlurExplicit(true));
    let handle = listen::images::blur_cover(&png);
    send(&mut s, A::ExplicitReady("k".into(), handle));
    assert!(s.images.explicit("k").is_some());
    assert!(s.images.explicit("other").is_none());
}

#[test]
fn settings_page_shows_the_new_rows() {
    let _serial = serial();
    let mut s = boot();
    let _ = s.update(Message::Navigate(Route::Settings));
    for text in [
        "Accent colour",
        "Primary shade",
        "Custom themes folder",
        "Font type",
        "Zoom",
        "Album background image",
        "Artist background image",
        "Blur explicit images",
        "Native cover aspect ratio",
        "Image placeholder",
    ] {
        assert!(has_text(&s, text), "{text}");
    }
}

#[test]
fn clicking_the_zoom_plus_button_sends_zoom_in() {
    let _serial = serial();
    let mut s = boot();
    let _ = s.update(Message::Navigate(Route::Settings));
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 3000.0), s.view());
        // The "+" button next to the zoom slider.
        let target = ui.find("+").expect("plus button");
        ui.point_at(target.visible_bounds().expect("visible").center());
        let _ = ui.simulate([
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)),
        ]);
        ui.into_messages().collect()
    };
    assert!(messages.iter().any(|m| matches!(m, Message::Appearance(A::ZoomIn))), "{messages:?}");
}

/// The override file's spacing and font sizes reach every screen (not just `Tokens`),
/// and dropping the file brings the defaults back.
#[test]
fn override_spacing_and_font_sizes_apply_to_the_screens() {
    use listen::style::metrics;
    let _serial = serial();
    let mut s = boot();
    let _ = s.update(Message::Navigate(Route::Settings));
    assert_eq!((metrics::font_md(), metrics::space_lg(), metrics::radius_md()), (14.0, 16.0, 5.0));
    let loaded = themes_dir(&[(
        "overrides.toml",
        "[font_size]\nmd = 20\nsm = 17\n[spacing]\nmd = 30\nlg = 40\n[radius]\nmd = 14\n",
    )]);
    send(&mut s, A::ThemesChanged(Arc::new(loaded)));
    assert_eq!((metrics::font_md(), metrics::font_sm()), (20.0, 17.0));
    assert_eq!((metrics::space_md(), metrics::space_lg(), metrics::radius_md()), (30.0, 40.0, 14.0));
    assert_eq!(metrics::space_sm(), 8.0, "unset values keep the default");
    {
        let theme = s.theme();
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 1100.0), s.view());
        let snapshot = ui.snapshot(&theme).expect("snapshot renders");
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("theme-override-spacing-font");
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            if entry.file_name().to_string_lossy().starts_with("theme-override-spacing-font-") {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        assert!(snapshot.matches_image(&path).expect("png written"));
    }

    send(&mut s, A::ThemesChanged(Arc::new(Loaded::default())));
    assert_eq!((metrics::font_md(), metrics::space_lg(), metrics::radius_md()), (14.0, 16.0, 5.0));
}
