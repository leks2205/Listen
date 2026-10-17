//! What a column (table) or card line (grid) shows for each kind of row.
//!
//! [`ListItem`] is implemented for [`Song`], [`Album`], [`Artist`] and [`Playlist`]; the shared
//! cells (row number with hover play button, heart, stars, links, actions) live here.

use super::anchor::anchored;
use crate::app::list::{ListId, ListMsg};
use crate::app::menu::{MenuMsg, MenuTarget};
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, Tokens, metrics};
use crate::ui::{album_cover, char_em, ellipsize_px, icon, label, label_w, song_cover};
use iced::font::Weight;
use iced::widget::{Row, button, container};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::{Album, Artist, ArtistRef, Playlist, Song, SourceKind, format_duration};
use listen_core::settings::{Align, ColumnId, RowSize};

/// Everything a cell needs to know about where it is drawn.
#[derive(Clone)]
pub struct Cx<'a> {
    pub state: &'a Listen,
    pub id: ListId,
    /// Row index in the list (global, not within the page).
    pub index: usize,
    pub size: RowSize,
    /// The pointer is over the row.
    pub hovered: bool,
    pub selected: bool,
    /// Room for the content in px (cell width minus its padding).
    pub width: f32,
    /// A line under a grid card (smaller, muted text) instead of a table cell.
    pub card: bool,
}

impl Cx<'_> {
    fn tokens(&self) -> Tokens {
        self.state.tokens
    }

    /// Cover size in a table row.
    pub fn cover_px(&self) -> f32 {
        self.size.height() - 16.0
    }

    fn font(&self) -> f32 {
        match self.size {
            RowSize::Compact => metrics::font_sm(),
            _ => metrics::font_md(),
        }
    }
}

/// A kind of row a list can show.
pub trait ListItem: Sized {
    /// Plain text of a text column (`None` for columns that are not plain text).
    /// Used for fitting a column to its content.
    fn text(&self, col: ColumnId, index: usize) -> Option<String>;

    /// The content of `col` for this row. `Cx::card` is set for the lines under a grid card.
    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message>;

    /// The cover for the image column and grid cards.
    fn cover<'a>(&self, state: &Listen, size: f32, radius: f32) -> Element<'a, Message>;

    /// The row is the playing song.
    fn is_current(&self, _state: &Listen) -> bool {
        false
    }

    /// Whether the heart is on; `None` when this row cannot be favourited here.
    fn favorite(&self, _state: &Listen) -> Option<bool> {
        None
    }

    /// The rating when the row can be rated here.
    fn rating(&self, _state: &Listen) -> Option<u8> {
        None
    }

    /// The right-click / "…" menu target.
    fn menu_target(&self) -> Option<MenuTarget> {
        None
    }

    /// Songs and albums can be played with the hover controls.
    fn playable(&self) -> bool {
        true
    }

    fn title(&self) -> String;

    /// The album headers of the rows `from..to` when the table has the album
    /// group column (songs only).
    fn album_groups(_state: &Listen, _items: &[Self], _from: usize, _to: usize) -> Vec<super::RowGroup> {
        Vec::new()
    }

    /// The card has an Expand control that opens a panel under its row (albums).
    fn expandable(&self) -> bool {
        false
    }

    /// The panel an expanded card opens under its row.
    fn expanded_panel<'a>(
        &'a self,
        _state: &'a Listen,
        _id: &ListId,
        _index: usize,
    ) -> Option<Element<'a, Message>> {
        None
    }

    /// The header of an album group (songs only).
    fn album_header<'a>(
        &'a self,
        _state: &'a Listen,
        _id: &ListId,
        _meta: &super::album_group::GroupMeta,
    ) -> Option<Element<'a, Message>> {
        None
    }

    /// The detail display type of this kind of row (albums have one); `None`
    /// shows the table instead.
    fn detail<'a>(_body: &super::Body<'a, Self>) -> Option<Element<'a, Message>> {
        None
    }
}

// ---- shared cells -----------------------------------------------------------

fn pad(width: f32) -> f32 {
    (width - 2.0 * metrics::space_sm()).max(10.0)
}

/// Plain text, ellipsized to the cell.
pub fn text_cell<'a>(cx: &Cx<'a>, s: impl AsRef<str>, strong: bool) -> Element<'a, Message> {
    let t = cx.tokens();
    let size = if cx.card && !strong { metrics::font_sm() } else { cx.font() };
    let color = if strong { t.foreground } else { t.muted };
    let weight = if strong { Weight::Medium } else { Weight::Normal };
    label_w(ellipsize_px(s.as_ref(), cx.width, size), size, color, weight).into()
}

/// The number column: the row number, a play button while the pointer is over the
/// row. Right-click on the button offers play now / next / last.
pub fn index_cell<'a>(cx: &Cx<'a>, number: usize, playable: bool, current: bool) -> Element<'a, Message> {
    let t = cx.tokens();
    // Edit mode of a playlist: a grip to drag the row by instead of the number.
    if cx.state.playlist_editing(&cx.id) {
        return iced::widget::mouse_area(
            container(icon(Icon::LuMenu, metrics::font_lg(), t.foreground)).padding(4),
        )
        .interaction(iced::mouse::Interaction::Grab)
        .on_press(Message::PlaylistDragStart(cx.index))
        .into();
    }
    if cx.hovered && playable {
        let behavior = cx.state.play_behavior();
        let play = button(icon(Icon::LuPlay, metrics::font_lg(), t.foreground))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(Message::List(ListMsg::PlayRow(cx.id.clone(), cx.index, behavior)));
        let (id, index) = (cx.id.clone(), cx.index);
        let play = crate::ui::tip::tip(
            cx.state,
            play,
            match behavior {
                listen_core::settings::PlayBehavior::Now => "Play now",
                listen_core::settings::PlayBehavior::Next => "Play next",
                listen_core::settings::PlayBehavior::Last => "Play last",
            },
            Some("listPlayDefault"),
            crate::ui::tip::Position::Right,
        );
        let hover = super::anchor::hover_anchored(
            play,
            move |rect| Message::List(ListMsg::PlayHover(id.clone(), index, rect)),
            Message::List(ListMsg::PlayHoverLeave),
        );
        return iced::widget::mouse_area(hover)
            .on_right_press(Message::List(ListMsg::PlayMenu(cx.id.clone(), cx.index)))
            .into();
    }
    let color = if current { t.primary } else { t.muted };
    label(format!("{number}"), cx.font(), color).into()
}

/// Names that link to their artist pages, each one separately (Feishin
/// `JoinedArtists`). Artists without an id are plain text.
pub fn artist_links<'a>(cx: &Cx<'a>, artists: &'a [ArtistRef], muted: bool) -> Element<'a, Message> {
    let t = cx.tokens();
    let size = if cx.card || muted { metrics::font_sm() } else { cx.font() };
    let color = if muted || cx.card { t.muted } else { t.foreground };
    let budget = cx.width / size;
    let em = |s: &str| s.chars().map(char_em).sum::<f32>();
    let mut out = Row::new().align_y(Alignment::Center);
    let mut used = 0.0;
    for (i, a) in artists.iter().enumerate() {
        let sep = if i > 0 { ", " } else { "" };
        let (sep_w, name_w) = (em(sep), em(&a.name));
        if used + sep_w + name_w > budget {
            let room = (budget - used - sep_w).max(0.0) * size;
            if room >= size * 2.0 {
                if !sep.is_empty() {
                    out = out.push(label(sep, size, color));
                }
                out = out.push(link(
                    t,
                    ellipsize_px(&a.name, room, size),
                    size,
                    color,
                    a.id.as_deref().map(artist_route),
                ));
            } else if used > 0.0 {
                out = out.push(label("…", size, color));
            }
            break;
        }
        if !sep.is_empty() {
            out = out.push(label(sep, size, color));
        }
        out = out.push(link(t, a.name.clone(), size, color, a.id.as_deref().map(artist_route)));
        used += sep_w + name_w;
    }
    out.into()
}

fn artist_route(id: &str) -> Message {
    Message::Navigate(Route::Artist(id.to_string()))
}

/// Text that opens a page when clicked; plain text without a target.
pub fn link<'a>(t: Tokens, s: String, size: f32, color: Color, to: Option<Message>) -> Element<'a, Message> {
    let Some(to) = to else { return label(s, size, color).into() };
    // No colour on the text: the button's text colour (hover) applies.
    let content = crate::ui::text_in(s, crate::ui::font(Weight::Normal))
        .size(size)
        .wrapping(iced::widget::text::Wrapping::None);
    button(content)
        .padding(0)
        .style(move |_, status| iced::widget::button::Style {
            text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                t.foreground
            } else {
                color
            },
            snap: true,
            ..Default::default()
        })
        .on_press(to)
        .into()
}

/// The album name as a link to the album page.
fn album_link<'a>(cx: &Cx<'a>, name: &str, id: Option<&str>) -> Element<'a, Message> {
    let t = cx.tokens();
    let size = if cx.card { metrics::font_sm() } else { cx.font() };
    link(
        t,
        ellipsize_px(name, cx.width, size),
        size,
        t.muted,
        id.filter(|i| !i.is_empty()).map(|i| Message::OpenAlbum(i.to_string())),
    )
}

/// The heart of a row; a click likes / unlikes (YouTube Music: Liked Music).
pub fn favorite_cell<'a>(cx: &Cx<'a>, on: bool) -> Element<'a, Message> {
    if !cx.state.show_favorites() {
        return space_cell();
    }
    let t = cx.tokens();
    let (glyph, color) = if on { (Icon::RiHeartFill, t.primary) } else { (Icon::RiHeartLine, t.muted) };
    crate::ui::tip::tip(
        cx.state,
        button(icon(glyph, metrics::font_lg(), color))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(Message::List(ListMsg::Favorite(cx.id.clone(), cx.index))),
        if on { "Remove from favorites" } else { "Add to favorites" },
        None,
        crate::ui::tip::Position::Bottom,
    )
}

/// Five stars; a click sets the rating, the same star again clears it.
pub fn rating_cell<'a>(cx: &Cx<'a>, rating: u8) -> Element<'a, Message> {
    if !cx.state.show_ratings() {
        return space_cell();
    }
    let t = cx.tokens();
    let mut stars = Row::new().spacing(0).align_y(Alignment::Center);
    for n in 1..=5u8 {
        let on = n <= rating;
        stars = stars.push(crate::ui::tip::tip_raw(
            cx.state,
            button(icon(
                if on { Icon::LuStarFill } else { Icon::LuStar },
                metrics::font_md(),
                if on { t.primary } else { t.muted },
            ))
            .padding(2)
            .style(style::icon_button(t, false))
            .on_press(Message::List(ListMsg::Rate(cx.id.clone(), cx.index, n))),
            &crate::ui::tip::rate_text(n),
            None,
            crate::ui::tip::Position::Bottom,
        ));
    }
    stars.into()
}

/// The "…" button: the row's context menu, opened at the button (whole selection when the
/// row is selected, like a right-click).
pub fn actions_cell<'a>(cx: &Cx<'a>, target: Option<MenuTarget>) -> Element<'a, Message> {
    let t = cx.tokens();
    let Some(target) = target else { return space_cell() };
    let (id, index, selected) = (cx.id.clone(), cx.index, cx.selected);
    anchored(
        crate::ui::tip::tip(
            cx.state,
            button(icon(Icon::LuEllipsis, metrics::font_lg(), t.muted))
                .padding(4)
                .style(style::icon_button(t, false))
                .on_press(Message::Noop),
            "More options",
            None,
            crate::ui::tip::Position::Left,
        ),
        move |bounds| {
            if selected {
                Message::List(ListMsg::ContextAt(id.clone(), index, bounds.position()))
            } else {
                Message::Menu(MenuMsg::OpenAt(target.clone(), bounds.position()))
            }
        },
    )
}

fn space_cell<'a>() -> Element<'a, Message> {
    iced::widget::space().width(0).into()
}

/// Small rounded labels (genres).
fn badges<'a>(cx: &Cx<'a>, names: &[String]) -> Element<'a, Message> {
    let t = cx.tokens();
    let mut out = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    let mut used = 0.0;
    for g in names {
        let w = g.chars().map(char_em).sum::<f32>() * metrics::font_sm() + 16.0;
        if used + w > cx.width {
            break;
        }
        used += w + metrics::space_xs();
        out = out.push(
            container(label(g.clone(), metrics::font_sm(), t.foreground))
                .padding(Padding::from([2.0, 8.0]))
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.surface)),
                    border: iced::border::rounded(999),
                    ..Default::default()
                }),
        );
    }
    out.into()
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// "2024-05-01T12:00:00Z" → "2024-05-01".
fn date_part(s: &str) -> String {
    s.split('T').next().unwrap_or(s).to_string()
}

/// A column's width fitted to its content: the widest of the first rows, in px.
/// `None` for columns without text.
pub fn content_width(data: &crate::app::list::ListData<'_>, col: ColumnId) -> Option<f32> {
    let mut widest: Option<f32> = None;
    for i in 0..data.len().min(300) {
        if let Some(s) = data.text(i, col) {
            let w = s.chars().map(char_em).sum::<f32>() * metrics::font_md();
            widest = Some(widest.map_or(w, |m| m.max(w)));
        }
    }
    let label = col.label().chars().map(char_em).sum::<f32>() * metrics::font_sm();
    widest.map(|w| (w.max(label) + 2.0 * metrics::space_sm() + 12.0).clamp(60.0, 600.0))
}

fn join_refs(a: &[ArtistRef]) -> String {
    a.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn nonempty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn list_text<T: AsRef<str>>(items: &[T]) -> String {
    items.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(", ")
}

// ---- songs -------------------------------------------------------------------

impl ListItem for Song {
    fn text(&self, col: ColumnId, index: usize) -> Option<String> {
        Some(match col {
            ColumnId::RowIndex => (index + 1).to_string(),
            ColumnId::Title | ColumnId::TitleCombined | ColumnId::TitleArtist => self.title.clone(),
            ColumnId::Duration => format_duration(self.duration),
            ColumnId::Album => self.album.clone()?,
            ColumnId::AlbumArtists => join_refs(&self.album_artists),
            ColumnId::Artists => self.artist_line(),
            ColumnId::Genres | ColumnId::GenreBadge => list_text(&self.genres),
            ColumnId::Year => self.year?.to_string(),
            ColumnId::ReleaseYear => {
                self.extra.release_date.as_deref().and_then(super::dates::year_of).or(self.year)?.to_string()
            }
            ColumnId::Composer => nonempty(list_text(&self.extra.composers))?,
            ColumnId::Date => super::dates::partial_date(self.extra.date.as_deref()?),
            ColumnId::ReleaseDate => super::dates::partial_date(self.extra.release_date.as_deref()?),
            ColumnId::LastPlayed => {
                super::dates::relative(self.extra.last_played.as_deref()?, chrono::Utc::now())?
            }
            ColumnId::DiscNumber => self.disc_number?.to_string(),
            ColumnId::TrackNumber => self.track_number?.to_string(),
            ColumnId::BitDepth => format!("{} bit", self.bit_depth?),
            ColumnId::BitRate => format!("{} kbps", self.bit_rate?),
            ColumnId::Codec => self.codec.clone().or_else(|| self.suffix.clone())?.to_uppercase(),
            ColumnId::SampleRate => format!("{:.1} kHz", self.sample_rate? as f32 / 1000.0),
            ColumnId::Comment => self.comment.clone()?,
            ColumnId::Channels => self.channels?.to_string(),
            ColumnId::Bpm => self.bpm?.to_string(),
            ColumnId::DateAdded => date_part(self.created.as_deref()?),
            ColumnId::Path => self.path.clone()?,
            ColumnId::PlayCount => self.play_count?.to_string(),
            ColumnId::Size => human_size(self.size?),
            _ => return None,
        })
    }

    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message> {
        let t = cx.tokens();
        // In the queue the same song can appear twice: the playing row is the one with its uid.
        let current = if cx.id.is_queue() {
            crate::ui::queue::row_is_current(cx.state, cx.index)
        } else {
            self.is_current(cx.state)
        };
        match col {
            ColumnId::RowIndex if cx.id.is_queue() => crate::ui::queue::index_cell(cx, current),
            ColumnId::RowIndex => index_cell(cx, cx.index + 1, true, current),
            ColumnId::Image => self.cover(cx.state, cx.cover_px(), metrics::radius_md()),
            ColumnId::Title => label_w(
                ellipsize_px(&self.title, cx.width, cx.font()),
                cx.font(),
                title_color(t, current),
                Weight::Medium,
            )
            .into(),
            ColumnId::TitleCombined | ColumnId::TitleArtist => {
                let cover = (col == ColumnId::TitleCombined)
                    .then(|| self.cover(cx.state, cx.cover_px(), metrics::radius_md()));
                let text_w = cx.width
                    - cover.as_ref().map_or(0.0, |_| cx.cover_px() + metrics::space_md())
                    - crate::ui::queue::tag_width(cx);
                let title = label_w(
                    ellipsize_px(&self.title, text_w, cx.font()),
                    cx.font(),
                    title_color(t, current),
                    Weight::Medium,
                );
                let inner = Cx { width: text_w, card: false, ..cx.clone() };
                let lines: Element<'a, Message> = if cx.size == RowSize::Compact {
                    // One line: the artists follow the title on compact rows.
                    iced::widget::column![title, artist_links(&inner, &self.artists, true)].into()
                } else {
                    iced::widget::column![title, artist_links(&inner, &self.artists, true)]
                        .spacing(metrics::space_xs())
                        .into()
                };
                let mut r = Row::new().spacing(metrics::space_md()).align_y(Alignment::Center);
                if let Some(c) = cover {
                    r = r.push(c);
                }
                r = r.push(lines);
                if let Some(tag) = crate::ui::queue::tag(cx) {
                    r = r.push(tag);
                }
                r.into()
            }
            ColumnId::Album => match &self.album {
                Some(name) => album_link(cx, name, self.album_id.as_deref()),
                None => space_cell(),
            },
            ColumnId::AlbumArtists => artist_links(cx, &self.album_artists, true),
            ColumnId::Artists => artist_links(cx, &self.artists, true),
            ColumnId::GenreBadge => badges(cx, &self.genres),
            ColumnId::UserFavorite => favorite_cell(cx, self.favorite),
            ColumnId::UserRating => match self.rating(cx.state) {
                Some(r) => rating_cell(cx, r),
                None if cx.state.can_rate(&self.server_id, self.source) => rating_cell(cx, 0),
                None => space_cell(),
            },
            ColumnId::Actions if cx.id.is_queue() => actions_cell(cx, crate::ui::queue::target(cx, self)),
            ColumnId::Actions => actions_cell(cx, self.menu_target()),
            other => match self.text(other, cx.index) {
                Some(s) => text_cell(cx, s, false),
                None => space_cell(),
            },
        }
    }

    fn cover<'a>(&self, state: &Listen, size: f32, radius: f32) -> Element<'a, Message> {
        song_cover(state, self, size, radius)
    }

    fn is_current(&self, state: &Listen) -> bool {
        state.queue.current().is_some_and(|c| c.song.id == self.id && c.song.server_id == self.server_id)
    }

    fn favorite(&self, _state: &Listen) -> Option<bool> {
        Some(self.favorite)
    }

    fn rating(&self, state: &Listen) -> Option<u8> {
        // Ratings exist where the server supports them; YouTube Music has none.
        (self.source != SourceKind::YouTubeMusic && state.can_rate(&self.server_id, self.source))
            .then(|| self.rating.unwrap_or(0))
    }

    fn menu_target(&self) -> Option<MenuTarget> {
        Some(MenuTarget::song(self))
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn album_groups(state: &Listen, items: &[Self], from: usize, to: usize) -> Vec<super::RowGroup> {
        super::album_group::groups(state, items, from, to)
    }

    fn album_header<'a>(
        &'a self,
        state: &'a Listen,
        id: &ListId,
        meta: &super::album_group::GroupMeta,
    ) -> Option<Element<'a, Message>> {
        Some(super::album_group::view(state, id, meta, self))
    }
}

fn title_color(t: Tokens, current: bool) -> Color {
    if current { t.primary } else { t.foreground }
}

// ---- albums ------------------------------------------------------------------

impl ListItem for Album {
    fn text(&self, col: ColumnId, index: usize) -> Option<String> {
        Some(match col {
            ColumnId::RowIndex => (index + 1).to_string(),
            ColumnId::Title => self.name.clone(),
            ColumnId::Duration => format_duration(self.duration),
            ColumnId::AlbumArtists | ColumnId::Artists => self.artist_line(),
            ColumnId::Genres | ColumnId::GenreBadge => list_text(&self.genres),
            ColumnId::ReleaseYear | ColumnId::Year => self.year?.to_string(),
            ColumnId::ReleaseDate => super::dates::partial_date(self.info.release_date.as_deref()?),
            ColumnId::LastPlayed => {
                super::dates::relative(self.info.last_played.as_deref()?, chrono::Utc::now())?
            }
            ColumnId::ReleaseType => self.release_type.clone()?,
            ColumnId::SongCount => self.song_count?.to_string(),
            ColumnId::PlayCount => self.play_count?.to_string(),
            _ => return None,
        })
    }

    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message> {
        let t = cx.tokens();
        match col {
            ColumnId::RowIndex => index_cell(cx, cx.index + 1, true, false),
            ColumnId::Image => self.cover(cx.state, cx.cover_px(), metrics::radius_md()),
            ColumnId::Title => {
                let size = cx.font();
                link(
                    t,
                    ellipsize_px(&self.name, cx.width, size),
                    size,
                    t.foreground,
                    Some(Message::OpenAlbum(self.id.clone())),
                )
            }
            ColumnId::AlbumArtists | ColumnId::Artists => artist_links(cx, &self.album_artists, true),
            ColumnId::GenreBadge => badges(cx, &self.genres),
            ColumnId::UserFavorite => match self.favorite(cx.state) {
                Some(on) => favorite_cell(cx, on),
                None => space_cell(),
            },
            ColumnId::UserRating => match self.rating(cx.state) {
                Some(r) => rating_cell(cx, r),
                None => space_cell(),
            },
            ColumnId::Actions => actions_cell(cx, self.menu_target()),
            other => match self.text(other, cx.index) {
                Some(s) => text_cell(cx, s, false),
                None => space_cell(),
            },
        }
    }

    fn cover<'a>(&self, state: &Listen, size: f32, radius: f32) -> Element<'a, Message> {
        album_cover(state, self, size, radius)
    }

    fn favorite(&self, state: &Listen) -> Option<bool> {
        use listen_source::Capabilities;
        state
            .sources
            .get(&self.server_id)
            .is_some_and(|s| s.capabilities().contains(Capabilities::FAVORITE_ITEMS))
            .then_some(self.favorite)
    }

    fn rating(&self, state: &Listen) -> Option<u8> {
        state.can_rate(&self.server_id, self.source).then(|| self.rating.unwrap_or(0))
    }

    fn menu_target(&self) -> Option<MenuTarget> {
        Some(MenuTarget::album(self))
    }

    fn title(&self) -> String {
        self.name.clone()
    }

    fn detail<'a>(body: &super::Body<'a, Self>) -> Option<Element<'a, Message>> {
        Some(super::detail::view(body))
    }

    fn expandable(&self) -> bool {
        true
    }

    fn expanded_panel<'a>(
        &'a self,
        state: &'a Listen,
        id: &ListId,
        index: usize,
    ) -> Option<Element<'a, Message>> {
        use crate::ui::album_detail::expanded::{Actions, expanded};
        let songs = super::detail::album_tracks(state, self);
        let (play_id, track_id) = (id.clone(), id.clone());
        Some(expanded(
            state,
            self,
            songs,
            Actions {
                close: Message::List(ListMsg::Collapse),
                play: Box::new(move |b| Message::List(ListMsg::PlayRow(play_id.clone(), index, b))),
                track: Box::new(move |n| Message::List(ListMsg::PlayDetailTrack(track_id.clone(), index, n))),
            },
        ))
    }
}

// ---- artists -----------------------------------------------------------------

impl ListItem for Artist {
    fn text(&self, col: ColumnId, index: usize) -> Option<String> {
        Some(match col {
            ColumnId::RowIndex => (index + 1).to_string(),
            ColumnId::Title => self.name.clone(),
            ColumnId::AlbumCount => self.album_count?.to_string(),
            ColumnId::SongCount => self.song_count?.to_string(),
            _ => return None,
        })
    }

    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message> {
        let t = cx.tokens();
        match col {
            ColumnId::RowIndex => index_cell(cx, cx.index + 1, false, false),
            ColumnId::Image => self.cover(cx.state, cx.cover_px(), cx.cover_px() / 2.0),
            ColumnId::Title => link(
                t,
                ellipsize_px(&self.name, cx.width, cx.font()),
                cx.font(),
                t.foreground,
                Some(artist_route(&self.id)),
            ),
            ColumnId::UserFavorite => match self.favorite(cx.state) {
                Some(on) => favorite_cell(cx, on),
                None => space_cell(),
            },
            ColumnId::UserRating => match self.rating(cx.state) {
                Some(r) => rating_cell(cx, r),
                None => space_cell(),
            },
            ColumnId::Actions => actions_cell(cx, self.menu_target()),
            // Under a card the counts say what they count ("12 albums").
            ColumnId::AlbumCount | ColumnId::SongCount if cx.card => {
                let (n, one, many) = match col {
                    ColumnId::AlbumCount => (self.album_count, "album", "albums"),
                    _ => (self.song_count, "song", "songs"),
                };
                match n {
                    Some(n) => text_cell(cx, format!("{n} {}", if n == 1 { one } else { many }), false),
                    None => space_cell(),
                }
            }
            other => match self.text(other, cx.index) {
                Some(s) => text_cell(cx, s, false),
                None => space_cell(),
            },
        }
    }

    fn menu_target(&self) -> Option<MenuTarget> {
        Some(MenuTarget::artist(self))
    }

    fn favorite(&self, state: &Listen) -> Option<bool> {
        (state.demo || state.can_favorite_items(&self.server_id)).then_some(self.favorite)
    }

    fn rating(&self, state: &Listen) -> Option<u8> {
        state.can_rate(&self.server_id, self.source).then(|| self.rating.unwrap_or(0))
    }

    fn cover<'a>(&self, state: &Listen, size: f32, radius: f32) -> Element<'a, Message> {
        if state.demo {
            crate::ui::art((60, 60, 60), size, radius, Icon::LuUser)
        } else {
            crate::ui::cover_on(state, &self.server_id, self.image.as_ref(), size, radius, Icon::LuUser)
        }
    }

    fn playable(&self) -> bool {
        false
    }

    fn title(&self) -> String {
        self.name.clone()
    }
}

// ---- playlists ---------------------------------------------------------------

impl ListItem for Playlist {
    fn text(&self, col: ColumnId, index: usize) -> Option<String> {
        Some(match col {
            ColumnId::RowIndex => (index + 1).to_string(),
            ColumnId::Title => self.name.clone(),
            ColumnId::SongCount => self.song_count?.to_string(),
            ColumnId::Duration => format_duration(self.duration),
            ColumnId::Owner => self.owner.clone()?,
            _ => return None,
        })
    }

    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message> {
        let t = cx.tokens();
        match col {
            ColumnId::RowIndex => index_cell(cx, cx.index + 1, true, false),
            ColumnId::Image => self.cover(cx.state, cx.cover_px(), metrics::radius_md()),
            ColumnId::Title => link(
                t,
                ellipsize_px(&self.name, cx.width, cx.font()),
                cx.font(),
                t.foreground,
                Some(Message::Navigate(Route::Playlist(self.id.clone()))),
            ),
            ColumnId::Actions => actions_cell(cx, self.menu_target()),
            other => match self.text(other, cx.index) {
                Some(s) => text_cell(cx, s, false),
                None => space_cell(),
            },
        }
    }

    fn cover<'a>(&self, state: &Listen, size: f32, radius: f32) -> Element<'a, Message> {
        if state.demo {
            crate::ui::art((60, 60, 60), size, radius, Icon::LuListMusic)
        } else {
            crate::ui::cover_on(state, &self.server_id, self.image.as_ref(), size, radius, Icon::LuListMusic)
        }
    }

    fn menu_target(&self) -> Option<MenuTarget> {
        Some(MenuTarget::playlist(self))
    }

    fn title(&self) -> String {
        self.name.clone()
    }
}

/// The header cell alignment as an iced alignment.
pub fn horizontal(align: Align) -> iced::alignment::Horizontal {
    match align {
        Align::Start => iced::alignment::Horizontal::Left,
        Align::Center => iced::alignment::Horizontal::Center,
        Align::End => iced::alignment::Horizontal::Right,
    }
}

/// A fixed-width cell with its content aligned.
pub fn sized<'a>(content: Element<'a, Message>, width: f32, align: Align) -> Element<'a, Message> {
    container(content)
        .width(Length::Fixed(width))
        .padding(Padding::from([0.0, metrics::space_sm()]))
        .align_x(horizontal(align))
        .clip(true)
        .into()
}

/// The room for the content of a column that is `width` px wide.
pub fn content_px(width: f32) -> f32 {
    pad(width)
}
