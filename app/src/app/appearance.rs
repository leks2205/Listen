//! Appearance: accent and primary shade, custom themes + the override file,
//! font, zoom, header backgrounds and cover options.
//!
//! Zoom messages for hotkeys: send `Message::Appearance(AppearanceMsg::ZoomIn)`,
//! `ZoomOut` or `ZoomReset` (Feishin steps 5 %, kept within 50-300 %).

use super::{Listen, Message};
use crate::images;
use iced::widget::image::Handle;
use iced::{Subscription, Task};
use listen_core::model::ImageRef;
use listen_core::settings::{self, FontKind, ImagePlaceholder, ZOOM_STEP};
use listen_core::theme::Rgb;
use listen_core::theme::custom::{self, Loaded};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Which header the blurred artwork is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlurKind {
    Album,
    Artist,
}

#[derive(Debug, Clone)]
pub enum AppearanceMsg {
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Slider drag: shown, not applied yet.
    ZoomDraft(u16),
    /// Slider released / typed value: applied.
    ZoomSet(u16),
    ZoomCommit,
    /// The accent text field (applied once it parses).
    AccentText(String),
    AccentSwatch(Rgb),
    UsePrimaryShade(bool),
    PrimaryShade(u8),
    FontKind(FontKind),
    FontFamily(String),
    FontPathInput(String),
    FontPathApply,
    FontLoaded(Result<(), String>),
    SystemFonts(Vec<String>),
    AlbumBackground(bool),
    ArtistBackground(bool),
    /// Slider drag (0-6): applied to the setting, images re-blurred on release.
    AlbumBlur(f32),
    ArtistBlur(f32),
    BlurCommit,
    BlurExplicit(bool),
    ExternalLinks(bool),
    ExplicitReady(images::Key, Option<Handle>),
    NativeAspect(bool),
    Placeholder(ImagePlaceholder),
    /// The themes folder changed on disk.
    ThemesChanged(Arc<Loaded>),
    OpenThemesFolder,
}

/// Runtime state of the appearance settings.
#[derive(Debug, Default)]
pub struct AppearanceState {
    /// Custom themes and the override file, as last read from the folder.
    pub loaded: Arc<Loaded>,
    pub themes_dir: Option<PathBuf>,
    pub font_error: Option<String>,
    pub font_path_input: String,
    /// Installed font families (listed when "System" is chosen).
    pub system_fonts: Vec<String>,
    /// Zoom value while the slider is dragged.
    pub zoom_draft: Option<u16>,
}

impl AppearanceState {
    /// Reads the themes folder (`None`: demo/tests never touch the real config).
    pub fn boot(config_dir: Option<PathBuf>, font_path: &str) -> Self {
        let themes_dir = config_dir.map(|d| custom::themes_dir(&d));
        let loaded = themes_dir.as_deref().map(custom::load_dir).unwrap_or_default();
        Self {
            loaded: Arc::new(loaded),
            themes_dir,
            font_path_input: font_path.to_string(),
            ..Self::default()
        }
    }
}

/// Polls the themes folder; a change must hold still for one tick before it
/// is reloaded (debounce), so a `git pull` or an unzip reloads once.
pub fn themes_subscription(dir: PathBuf) -> Subscription<Message> {
    Subscription::run_with(dir, |dir| {
        let dir = dir.clone();
        iced::stream::channel(2, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
            use iced::futures::SinkExt;
            let mut loaded_stamp = None;
            let mut pending = None;
            loop {
                let d = dir.clone();
                let stamp = tokio::task::spawn_blocking(move || custom::stamp(&d)).await.unwrap_or_default();
                if loaded_stamp.as_ref() != Some(&stamp) {
                    if pending.as_ref() == Some(&stamp) {
                        let d = dir.clone();
                        if let Ok(loaded) = tokio::task::spawn_blocking(move || custom::load_dir(&d)).await {
                            loaded_stamp = Some(stamp);
                            pending = None;
                            if out
                                .send(Message::Appearance(AppearanceMsg::ThemesChanged(Arc::new(loaded))))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    } else {
                        pending = Some(stamp);
                    }
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
    })
}

impl Listen {
    /// Start-up work: the chosen font (a custom file has to be loaded) and
    /// the system font list when it is needed.
    pub(super) fn appearance_boot(&mut self) -> Task<Message> {
        let a = self.config.settings.appearance.clone();
        crate::appearance::set_font(crate::appearance::font_for(&a));
        let mut tasks = Vec::new();
        match a.font_kind {
            FontKind::Custom => tasks.push(self.load_font_file(&a.font_path)),
            FontKind::System => tasks.push(self.list_system_fonts()),
            FontKind::Builtin => {}
        }
        Task::batch(tasks)
    }

    fn list_system_fonts(&self) -> Task<Message> {
        if !self.appearance.system_fonts.is_empty() {
            return Task::none();
        }
        Task::perform(
            async {
                tokio::task::spawn_blocking(crate::appearance::system_families).await.unwrap_or_default()
            },
            |f| Message::Appearance(AppearanceMsg::SystemFonts(f)),
        )
    }

    /// Reads, checks and registers a font file; its family becomes the UI font.
    fn load_font_file(&mut self, path: &str) -> Task<Message> {
        match crate::appearance::read_font_file(path) {
            Ok((bytes, family)) => {
                let a = &mut self.config.settings.appearance;
                a.font_kind = FontKind::Custom;
                a.font_path = path.trim().trim_matches('"').to_string();
                a.font = family;
                crate::appearance::set_font(crate::appearance::font_for(a));
                self.appearance.font_error = None;
                iced::font::load(bytes)
                    .map(|r| Message::Appearance(AppearanceMsg::FontLoaded(r.map_err(|e| format!("{e:?}")))))
            }
            Err(e) => {
                self.appearance.font_error = Some(e);
                Task::none()
            }
        }
    }

    /// Cache key of the blurred header art of `image`.
    pub fn blur_key(&self, image: &ImageRef, kind: BlurKind) -> images::Key {
        let key = self.image_key(image);
        match kind {
            BlurKind::Album => key,
            BlurKind::Artist => format!("{key}#artist"),
        }
    }

    pub fn background_enabled(&self, kind: BlurKind) -> bool {
        let a = &self.config.settings.appearance;
        match kind {
            BlurKind::Album => a.album_background,
            BlurKind::Artist => a.artist_background,
        }
    }

    /// Blur radius (sigma in pixels of the 192x48 header strip) for a 0-6 setting.
    pub fn blur_sigma(&self, kind: BlurKind) -> f32 {
        let a = &self.config.settings.appearance;
        let blur = match kind {
            BlurKind::Album => a.album_background_blur,
            BlurKind::Artist => a.artist_background_blur,
        };
        blur.clamp(0.0, 6.0) * 1.5
    }

    /// Starts a background job producing the blurred header version of an image.
    pub fn request_blur_for(&mut self, image: Option<&ImageRef>, kind: BlurKind) -> Task<Message> {
        if !self.background_enabled(kind) {
            return Task::none();
        }
        let (Some(image), Some(source)) = (image, &self.source) else { return Task::none() };
        let key = self.image_key(image);
        let blur_key = self.blur_key(image, kind);
        if !self.images.should_blur(&blur_key) {
            return Task::none();
        }
        let Some(url) = source.image_url(image, super::ART_SIZE) else { return Task::none() };
        let (disk, http) = (self.images.disk_path(&key), self.http.clone());
        let sigma = self.blur_sigma(kind);
        Task::perform(
            async move {
                let bytes = images::fetch(http, url, disk).await?;
                tokio::task::spawn_blocking(move || images::blur(&bytes, sigma)).await.ok().flatten()
            },
            move |h| Message::BlurLoaded(blur_key.clone(), h),
        )
    }

    /// Blurs a freshly loaded cover when explicit covers are hidden.
    pub(super) fn explicit_blur_job(&mut self, key: &str, bytes: Option<&Arc<Vec<u8>>>) -> Task<Message> {
        let (true, Some(bytes)) = (self.config.settings.appearance.blur_explicit_images, bytes) else {
            return Task::none();
        };
        self.explicit_job(key.to_string(), bytes.to_vec())
    }

    fn explicit_job(&mut self, key: images::Key, bytes: Vec<u8>) -> Task<Message> {
        if !self.images.should_blur_explicit(&key) {
            return Task::none();
        }
        Task::perform(
            async move {
                let k = key.clone();
                (k, tokio::task::spawn_blocking(move || images::blur_cover(&bytes)).await.ok().flatten())
            },
            |(key, handle)| Message::Appearance(AppearanceMsg::ExplicitReady(key, handle)),
        )
    }

    /// Re-blurs the header art of the open album / artist.
    fn reblur_headers(&mut self) -> Task<Message> {
        self.images.clear_blurred();
        let album = self.library.album.as_ref().and_then(|a| a.image.clone());
        let artist = self.library.artist.as_ref().and_then(|d| d.artist.image.clone());
        Task::batch([
            self.request_blur_for(album.as_ref(), BlurKind::Album),
            self.request_blur_for(artist.as_ref(), BlurKind::Artist),
        ])
    }

    pub(super) fn on_appearance(&mut self, message: AppearanceMsg) -> Task<Message> {
        use AppearanceMsg as M;
        let mut task = Task::none();
        let mut theme_changed = false;
        let a = &mut self.config.settings.appearance;
        match message {
            M::ZoomIn => a.zoom = settings::step_zoom(a.zoom_clamped(), i32::from(ZOOM_STEP)),
            M::ZoomOut => a.zoom = settings::step_zoom(a.zoom_clamped(), -i32::from(ZOOM_STEP)),
            M::ZoomReset => a.zoom = 100,
            M::ZoomSet(z) => {
                a.zoom = z.clamp(settings::ZOOM_MIN, settings::ZOOM_MAX);
                self.appearance.zoom_draft = None;
            }
            M::ZoomDraft(z) => {
                self.appearance.zoom_draft = Some(z.clamp(settings::ZOOM_MIN, settings::ZOOM_MAX));
                return Task::none();
            }
            M::ZoomCommit => {
                if let Some(z) = self.appearance.zoom_draft.take() {
                    a.zoom = z;
                }
            }
            M::AccentText(text) => {
                if let Some(rgb) = custom::parse_color(&serde_json::Value::String(text.clone())) {
                    a.accent = Some(rgb);
                    theme_changed = true;
                }
                self.accent_input = text;
            }
            M::AccentSwatch(rgb) => {
                a.accent = Some(rgb);
                self.accent_input = rgb.css();
                theme_changed = true;
            }
            M::UsePrimaryShade(v) => {
                a.use_theme_primary_shade = v;
                theme_changed = true;
            }
            M::PrimaryShade(n) => {
                a.primary_shade = n.min(9);
                theme_changed = true;
            }
            M::FontKind(kind) => {
                a.font_kind = kind;
                match kind {
                    FontKind::Builtin => {
                        if !settings::BUILTIN_FONTS.contains(&a.font.as_str()) {
                            a.font = settings::BUILTIN_FONTS[0].into();
                        }
                        crate::appearance::set_font(crate::appearance::font_for(a));
                    }
                    FontKind::System => {
                        crate::appearance::set_font(crate::appearance::font_for(a));
                        task = self.list_system_fonts();
                    }
                    FontKind::Custom => {
                        let path = a.font_path.clone();
                        if !path.is_empty() {
                            task = self.load_font_file(&path);
                        }
                    }
                }
                self.appearance.font_error = None;
            }
            M::FontFamily(name) => {
                a.font = name;
                crate::appearance::set_font(crate::appearance::font_for(a));
                self.appearance.font_error = None;
            }
            M::FontPathInput(text) => {
                self.appearance.font_path_input = text;
                return Task::none();
            }
            M::FontPathApply => {
                let path = self.appearance.font_path_input.clone();
                task = self.load_font_file(&path);
                if self.appearance.font_error.is_some() {
                    return Task::none();
                }
            }
            M::FontLoaded(result) => {
                if let Err(e) = result {
                    self.appearance.font_error = Some(format!("The font could not be loaded: {e}"));
                }
                return Task::none();
            }
            M::SystemFonts(fonts) => {
                self.appearance.system_fonts = fonts;
                return Task::none();
            }
            M::AlbumBackground(v) => {
                a.album_background = v;
                task = self.reblur_headers();
            }
            M::ArtistBackground(v) => {
                a.artist_background = v;
                task = self.reblur_headers();
            }
            M::AlbumBlur(v) => a.album_background_blur = v.clamp(0.0, 6.0),
            M::ArtistBlur(v) => a.artist_background_blur = v.clamp(0.0, 6.0),
            M::BlurCommit => task = self.reblur_headers(),
            M::ExternalLinks(v) => a.external_links = v,
            M::BlurExplicit(v) => {
                a.blur_explicit_images = v;
                if v {
                    task = self.blur_cached_covers();
                }
            }
            M::ExplicitReady(key, handle) => {
                self.images.explicit_loaded(key, handle);
                return Task::none();
            }
            M::NativeAspect(v) => a.native_cover_aspect = v,
            M::Placeholder(p) => a.image_placeholder = p,
            M::ThemesChanged(loaded) => {
                if *loaded == *self.appearance.loaded {
                    return Task::none();
                }
                tracing::info!(
                    "themes folder reloaded: {} themes, {} errors, {} warnings",
                    loaded.themes.len(),
                    loaded.errors.len(),
                    loaded.warning_count()
                );
                self.appearance.loaded = loaded;
                self.apply_theme();
                return Task::none();
            }
            M::OpenThemesFolder => {
                if let Some(dir) = &self.appearance.themes_dir {
                    let _ = std::fs::create_dir_all(dir);
                    open_folder(dir);
                }
                return Task::none();
            }
        }
        if theme_changed {
            self.apply_theme();
        }
        self.save_config();
        task
    }

    /// Blurred copies for the covers already in memory (setting just turned on).
    fn blur_cached_covers(&mut self) -> Task<Message> {
        let jobs: Vec<(images::Key, Vec<u8>)> = self.images.cached_bytes();
        Task::batch(jobs.into_iter().map(|(k, b)| self.explicit_job(k, b)).collect::<Vec<_>>())
    }
}

fn open_folder(dir: &std::path::Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        tracing::warn!("opening {}: {e}", dir.display());
    }
}

/// A stable tint for a cover whose image is still loading.
pub fn placeholder_tint(key: &str, dark: bool) -> iced::Color {
    // FNV-1a over the key picks a hue; saturation and lightness stay muted.
    let h = key.bytes().fold(0xcbf29ce484222325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    let hue = (h % 360) as f32;
    let (s, l) = if dark { (0.30, 0.24) } else { (0.35, 0.80) };
    hsl(hue, s, l)
}

fn hsl(h: f32, s: f32, l: f32) -> iced::Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    iced::Color::from_rgb(r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_tint_is_stable_and_muted() {
        let a = placeholder_tint("srv/abc@300", true);
        assert_eq!(a, placeholder_tint("srv/abc@300", true));
        assert_ne!(a, placeholder_tint("srv/abd@300", true));
        assert!(a.r.max(a.g).max(a.b) < 0.5, "dark tint stays dark");
        assert!(placeholder_tint("x", false).r.min(placeholder_tint("x", false).g) > 0.5);
    }
}
