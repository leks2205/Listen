//! State and messages of the full-screen player and the player-bar extras
//! around it: panel tabs, the dynamic background, the full-window visualizer,
//! the "Related" tab's hook, idle auto-open, ratings and the settings.
//! Rendering is in `ui::fullscreen`.

use super::sleep_timer::{SleepMsg, SleepUi};
use super::{FullscreenTab, Listen, Message};
use iced::widget::image::Handle;
use iced::{Color, Task};
use listen_core::model::{Song, SourceKind};
use listen_core::settings::{ItemAlignment, RightControl, TitleDisplay};
use std::time::{Duration, Instant};

/// The cover-derived background of the player.
#[derive(Debug, Clone)]
pub struct Backdrop {
    /// Cache key: the image and the blur it was made with.
    pub key: String,
    /// The dominant colour of the cover.
    pub color: Color,
    /// The blurred cover (`dynamic_is_image`).
    pub image: Option<Handle>,
}

#[derive(Debug)]
pub struct FullscreenState {
    /// The visualizer fills the whole window (Feishin `visualizerExpanded`).
    pub visualizer_expanded: bool,
    /// Shrinking the visualizer goes back to the full-screen player.
    pub visualizer_return: bool,
    /// Similar songs of `related_for` (filled by the radio / similar feature).
    pub related: Vec<Song>,
    /// Id of the song `related` belongs to.
    pub related_for: Option<String>,
    pub related_loading: bool,
    pub backdrop: Option<Backdrop>,
    /// Key of the backdrop job in flight.
    backdrop_pending: Option<String>,
    /// Last mouse / key activity, for the auto-open timeout.
    pub last_input: Instant,
    pub sleep_ui: SleepUi,
    pub sleep_tick: Option<Instant>,
    /// Start of the title marquee, and the song it runs for.
    marquee: Option<(String, Instant)>,
}

impl Default for FullscreenState {
    fn default() -> Self {
        Self {
            visualizer_expanded: false,
            visualizer_return: false,
            related: Vec::new(),
            related_for: None,
            related_loading: false,
            backdrop: None,
            backdrop_pending: None,
            last_input: Instant::now(),
            sleep_ui: SleepUi::default(),
            sleep_tick: None,
            marquee: None,
        }
    }
}

/// Modal key of the gear's settings.
pub const SETTINGS_MODAL: &str = "fullscreen-settings";

/// One change of the full-screen / player-bar settings.
#[derive(Debug, Clone)]
pub enum FsSetting {
    DynamicBackground(bool),
    DynamicIsImage(bool),
    ImageBlur(f32),
    Opacity(u8),
    CoverSize(u8),
    Alignment(ItemAlignment),
    TitleDisplay(TitleDisplay),
    TitleLines(u8),
    AspectRatio(bool),
    AutoOpenMinutes(u32),
    /// A badge id on / off.
    Badge(String, bool),
    OpenDrawer(bool),
    /// The expand button on the bar's cover (off: the sidebar image goes too).
    ExpandArt(bool),
    ShowRemaining(bool),
    RightVisible(usize, bool),
    RightMove(usize, i8),
}

#[derive(Debug, Clone)]
pub enum FullscreenMsg {
    /// A panel tab button: opens it, or closes the panel when it is the open one.
    TogglePanel(FullscreenTab),
    /// The gear: the settings dialog.
    OpenSettings,
    Setting(FsSetting),
    /// The visualizer fills the window / goes back.
    ExpandVisualizer,
    ShrinkVisualizer,
    /// A cover backdrop is ready (`None`: the image could not be read).
    Backdrop(String, Option<(Color, Option<Handle>)>),
    /// Hook for the similar-songs feature: the related songs of a song.
    SetRelated(String, Vec<Song>),
    RelatedLoading(bool),
    /// Play the related list from this row.
    PlayRelated(usize),
    /// Mouse or key activity (auto-open timeout).
    Activity,
    IdleCheck,
    /// Right duration: total / remaining.
    ToggleRemaining,
    /// A star of the bar: sets the rating, the same star again clears it.
    Rate(u8),
    Sleep(SleepMsg),
    /// Frame of the one-line title's marquee.
    Marquee,
}

impl Listen {
    /// How far the full-screen title overflows its width (0: no marquee).
    pub fn title_overflow(&self) -> f32 {
        let f = &self.config.settings.fullscreen;
        if !self.fullscreen || f.title_display != TitleDisplay::Scroll {
            return 0.0;
        }
        let Some(item) = self.queue.current() else { return 0.0 };
        let open = self.fullscreen_tab != super::FullscreenTab::Hidden;
        let width = crate::ui::fullscreen::metadata::title_width(self.window_size, f.cover_size, open);
        crate::ui::fullscreen::metadata::overflow_px(&item.song.title, width)
    }

    pub(super) fn on_fullscreen(&mut self, msg: FullscreenMsg) -> Task<Message> {
        match msg {
            FullscreenMsg::TogglePanel(tab) => {
                self.fullscreen_tab = if self.fullscreen_tab == tab { FullscreenTab::Hidden } else { tab };
                return self.panel_shown();
            }
            FullscreenMsg::OpenSettings => self.open_modal(SETTINGS_MODAL),
            FullscreenMsg::Setting(s) => return self.fs_setting(s),
            FullscreenMsg::ExpandVisualizer => {
                self.fs.visualizer_return = self.fullscreen;
                self.fs.visualizer_expanded = true;
                self.fullscreen = false;
            }
            FullscreenMsg::ShrinkVisualizer => {
                self.fs.visualizer_expanded = false;
                self.fullscreen = std::mem::take(&mut self.fs.visualizer_return);
                if !self.visualizer_wanted() {
                    self.spectrum = None;
                }
            }
            FullscreenMsg::Backdrop(key, result) => {
                if self.fs.backdrop_pending.as_deref() == Some(key.as_str()) {
                    self.fs.backdrop_pending = None;
                }
                if let Some((color, image)) = result {
                    self.fs.backdrop = Some(Backdrop { key, color, image });
                }
            }
            FullscreenMsg::SetRelated(song, songs) => {
                self.fs.related_for = Some(song);
                self.fs.related = songs;
                self.fs.related_loading = false;
            }
            FullscreenMsg::RelatedLoading(on) => self.fs.related_loading = on,
            FullscreenMsg::PlayRelated(i) => {
                let songs = self.fs.related.clone();
                if i < songs.len() {
                    return self.play_songs(songs, i);
                }
            }
            FullscreenMsg::Marquee => {
                let overflow = self.title_overflow();
                let Some(item) = self.queue.current() else { return Task::none() };
                let id = item.song.id.clone();
                if self.fs.marquee.as_ref().is_none_or(|(s, _)| *s != id) {
                    self.fs.marquee = Some((id, Instant::now()));
                }
                let t = self.fs.marquee.as_ref().map_or(0.0, |(_, at)| at.elapsed().as_secs_f32());
                let x = crate::ui::fullscreen::metadata::marquee_pos(t, overflow);
                return iced::widget::operation::snap_to(
                    iced::widget::Id::new(crate::ui::fullscreen::metadata::TITLE_ID),
                    iced::widget::scrollable::RelativeOffset { x, y: 0.0 },
                );
            }
            FullscreenMsg::Activity => self.fs.last_input = Instant::now(),
            FullscreenMsg::IdleCheck => {
                let minutes = self.config.settings.fullscreen.auto_open_minutes;
                if idle_should_open(
                    minutes,
                    self.fs.last_input.elapsed(),
                    self.playback.playing,
                    self.fullscreen || self.fs.visualizer_expanded,
                    self.queue.current().is_some(),
                ) {
                    self.fs.last_input = Instant::now();
                    return self.handle(Message::ToggleFullscreen);
                }
            }
            FullscreenMsg::ToggleRemaining => {
                let b = &mut self.config.settings.player_bar;
                b.show_time_remaining = !b.show_time_remaining;
                self.save_config();
            }
            FullscreenMsg::Rate(stars) => {
                let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
                let stars = if song.rating == Some(stars) { 0 } else { stars };
                return self.rate_song(&song, stars);
            }
            FullscreenMsg::Sleep(m) => return self.on_sleep(m),
        }
        Task::none()
    }

    /// Whether the song's source can rate it (stars in the bar). Never YouTube Music.
    pub fn can_rate_current(&self) -> bool {
        use listen_source::Capabilities;
        let Some(item) = self.queue.current() else { return false };
        self.show_ratings()
            && item.song.source != SourceKind::YouTubeMusic
            && self
                .sources
                .get(&item.song.server_id)
                .is_some_and(|s| s.capabilities().contains(Capabilities::RATINGS))
    }

    /// The panel changed: the lyrics need loading and the queue list starts at the top.
    fn panel_shown(&mut self) -> Task<Message> {
        self.queue_scroll.offset = 0.0;
        if self.fullscreen_tab != FullscreenTab::Visualizer && !self.queue_visualizer_visible() {
            self.spectrum = None;
        }
        Task::batch([self.load_lyrics(), self.scroll_lyrics(), self.related_track_changed()])
    }

    /// The full-screen player was opened or closed.
    pub(super) fn fullscreen_toggled(&mut self) -> Task<Message> {
        if self.fs.visualizer_expanded {
            // The cover button closes everything (Feishin: left-controls).
            self.fs.visualizer_expanded = false;
            self.fs.visualizer_return = false;
            self.fullscreen = false;
        } else {
            self.fullscreen = !self.fullscreen;
        }
        self.fs.last_input = Instant::now();
        self.queue_scroll.offset = 0.0;
        Task::batch([
            self.load_lyrics(),
            self.scroll_lyrics(),
            self.fs_backdrop(),
            self.related_track_changed(),
        ])
    }

    /// Escape: the visualizer shrinks, the sleep popover closes, the player closes.
    /// `None` when there is nothing to close.
    pub(super) fn fullscreen_escape(&mut self) -> Option<Task<Message>> {
        if self.fs.visualizer_expanded {
            return Some(self.on_fullscreen(FullscreenMsg::ShrinkVisualizer));
        }
        if self.fs.sleep_ui.open {
            return Some(self.on_sleep(SleepMsg::Close));
        }
        if self.fullscreen {
            return Some(self.fullscreen_toggled());
        }
        None
    }

    /// The spectrum is wanted by the full-screen panel or the full-window overlay.
    pub fn visualizer_wanted(&self) -> bool {
        self.fs.visualizer_expanded
            || (self.fullscreen && self.fullscreen_tab == FullscreenTab::Visualizer)
            || self.queue_visualizer_visible()
    }

    fn fs_setting(&mut self, setting: FsSetting) -> Task<Message> {
        let mut backdrop = false;
        {
            let f = &mut self.config.settings.fullscreen;
            let b = &mut self.config.settings.player_bar;
            match setting {
                FsSetting::DynamicBackground(v) => {
                    f.dynamic_background = v;
                    backdrop = true;
                }
                FsSetting::DynamicIsImage(v) => {
                    f.dynamic_is_image = v;
                    backdrop = true;
                }
                FsSetting::ImageBlur(v) => {
                    f.image_blur = v.clamp(0.0, 6.0).round();
                    backdrop = true;
                }
                FsSetting::Opacity(v) => f.opacity = v.min(100),
                FsSetting::CoverSize(v) => f.cover_size = v.clamp(55, 100),
                FsSetting::Alignment(v) => f.alignment = v,
                FsSetting::TitleDisplay(v) => f.title_display = v,
                FsSetting::TitleLines(v) => f.title_lines = v.clamp(1, 10),
                FsSetting::AspectRatio(v) => f.use_image_aspect_ratio = v,
                FsSetting::AutoOpenMinutes(v) => f.auto_open_minutes = v.min(120),
                FsSetting::Badge(id, on) => {
                    b.info_items.retain(|i| *i != id);
                    if on {
                        b.info_items.push(id);
                    }
                }
                FsSetting::OpenDrawer(v) => b.open_drawer = v,
                FsSetting::ExpandArt(v) => {
                    self.config.settings.sidebar.show_image = v;
                    if !v {
                        self.config.settings.sidebar.cover_expanded = false;
                    }
                }
                FsSetting::ShowRemaining(v) => b.show_time_remaining = v,
                FsSetting::RightVisible(i, v) => {
                    if let Some(c) = b.right_controls.get_mut(i) {
                        c.visible = v;
                    }
                }
                FsSetting::RightMove(i, dir) => {
                    if b.right_controls.len() < listen_core::settings::RIGHT_CONTROLS.len() {
                        // A saved list from before a button existed: complete it first.
                        for (id, _) in listen_core::settings::RIGHT_CONTROLS {
                            if !b.right_controls.iter().any(|c| c.id == *id) {
                                b.right_controls.push(RightControl { id: (*id).into(), visible: true });
                            }
                        }
                    }
                    let j = i.checked_add_signed(isize::from(dir)).filter(|j| *j < b.right_controls.len());
                    if let (Some(j), true) = (j, i < b.right_controls.len()) {
                        b.right_controls.swap(i, j);
                    }
                }
            }
        }
        self.save_config();
        if backdrop { self.fs_backdrop() } else { Task::none() }
    }

    /// Makes the backdrop of the playing song's cover (colour + blurred image).
    pub(super) fn fs_backdrop(&mut self) -> Task<Message> {
        let s = self.config.settings.fullscreen.clone();
        if !s.dynamic_background {
            return Task::none();
        }
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        if self.demo {
            let (r, g, b) = crate::demo::color_for_album(song.album_id.as_deref());
            self.fs.backdrop = Some(Backdrop {
                key: format!("demo-{r}-{g}-{b}"),
                color: Color::from_rgb8(r, g, b),
                image: None,
            });
            return Task::none();
        }
        let (Some(image), Some(source)) = (song.image.clone(), self.sources.get(&song.server_id).cloned())
        else {
            self.fs.backdrop = None;
            return Task::none();
        };
        let blur = if s.dynamic_is_image { s.image_blur } else { -1.0 };
        let key = format!("{}|{blur}", self.image_key_on(&song.server_id, &image));
        if self.fs.backdrop.as_ref().is_some_and(|b| b.key == key)
            || self.fs.backdrop_pending.as_deref() == Some(key.as_str())
        {
            return Task::none();
        }
        let Some(url) = source.image_url(&image, super::ART_SIZE) else { return Task::none() };
        self.fs.backdrop_pending = Some(key.clone());
        let disk = self.images.disk_path(&self.image_key_on(&song.server_id, &image));
        let http = self.http.clone();
        Task::perform(
            async move {
                let bytes = crate::images::fetch(http, url, disk).await?;
                tokio::task::spawn_blocking(move || make_backdrop(&bytes, blur)).await.ok().flatten()
            },
            move |r| Message::Fullscreen(FullscreenMsg::Backdrop(key.clone(), r)),
        )
    }

    /// The subscriptions of this feature: input activity and the idle check.
    pub fn fullscreen_subscriptions(&self) -> Vec<iced::Subscription<Message>> {
        let mut subs = Vec::new();
        if self.config.settings.fullscreen.auto_open_minutes > 0 && !self.fullscreen {
            subs.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(
                    iced::mouse::Event::CursorMoved { .. }
                    | iced::mouse::Event::ButtonPressed(_)
                    | iced::mouse::Event::WheelScrolled { .. },
                )
                | iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { .. }) => {
                    Some(Message::Fullscreen(FullscreenMsg::Activity))
                }
                _ => None,
            }));
            subs.push(
                iced::time::every(Duration::from_secs(5))
                    .map(|_| Message::Fullscreen(FullscreenMsg::IdleCheck)),
            );
        }
        if self.title_overflow() > 0.0 {
            subs.push(
                iced::time::every(Duration::from_millis(33))
                    .map(|_| Message::Fullscreen(FullscreenMsg::Marquee)),
            );
        }
        if self.sleep_ticking() {
            subs.push(
                iced::time::every(Duration::from_secs(1))
                    .map(|_| Message::Fullscreen(FullscreenMsg::Sleep(SleepMsg::Tick))),
            );
        }
        subs
    }
}

/// Whether the idle timeout has run out: enabled, quiet long enough, a song
/// playing and the player not open yet.
pub fn idle_should_open(
    minutes: u32,
    idle: Duration,
    playing: bool,
    already_open: bool,
    has_track: bool,
) -> bool {
    minutes > 0
        && playing
        && has_track
        && !already_open
        && idle >= Duration::from_secs(u64::from(minutes) * 60)
}

/// The dominant colour of a cover, and (for `blur >= 0`, in rem) its blurred copy.
fn make_backdrop(bytes: &[u8], blur: f32) -> Option<(Color, Option<Handle>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let color = dominant_color(&img.resize_exact(24, 24, image::imageops::FilterType::Triangle).to_rgba8());
    let handle = (blur >= 0.0).then(|| {
        let small = img.resize_to_fill(96, 96, image::imageops::FilterType::Triangle).to_rgba8();
        let blurred = image::imageops::blur(&small, (blur * 1.5).max(0.5));
        let (w, h) = blurred.dimensions();
        Handle::from_rgba(w, h, blurred.into_raw())
    });
    Some((color, handle))
}

/// The most common colour: pixels bucketed to 4 bits a channel, the fullest bucket's mean.
fn dominant_color(img: &image::RgbaImage) -> Color {
    use std::collections::HashMap;
    let mut buckets: HashMap<(u8, u8, u8), (u32, [u32; 3])> = HashMap::new();
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        if a < 128 {
            continue;
        }
        let e = buckets.entry((r >> 4, g >> 4, b >> 4)).or_default();
        e.0 += 1;
        e.1[0] += u32::from(r);
        e.1[1] += u32::from(g);
        e.1[2] += u32::from(b);
    }
    // Ties broken by the bucket id so the result is stable.
    let best = buckets.iter().max_by_key(|(k, (n, _))| (*n, **k));
    match best {
        Some((_, (n, sum))) => Color::from_rgb8((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8),
        None => Color::BLACK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_opens_only_when_quiet_playing_and_closed() {
        let m = Duration::from_secs(60);
        assert!(idle_should_open(1, m, true, false, true));
        assert!(!idle_should_open(0, m * 100, true, false, true), "0 = off");
        assert!(!idle_should_open(1, m / 2, true, false, true));
        assert!(!idle_should_open(1, m, false, false, true), "paused");
        assert!(!idle_should_open(1, m, true, true, true), "already open");
        assert!(!idle_should_open(1, m, true, false, false), "nothing queued");
    }

    #[test]
    fn dominant_color_picks_the_fullest_bucket() {
        let mut img = image::RgbaImage::from_pixel(10, 10, image::Rgba([200, 20, 20, 255]));
        for x in 0..3 {
            img.put_pixel(x, 0, image::Rgba([0, 0, 255, 255]));
        }
        let c = dominant_color(&img);
        assert!(c.r > 0.7 && c.b < 0.1, "{c:?}");
    }
}
