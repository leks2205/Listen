//! The Home page (Feishin `features/home`): a feature carousel of random
//! albums, grid carousels per section whose arrows page and load more, and
//! featured genres. The source's own feed (YouTube Music's Quick picks,
//! mixes, ...) is shown first, its shelves page like the others.

use super::{album_cover, cover_on, ellipsize, ellipsize_px, icon, label, label_w, song_cover};
use crate::app::home::{HomeMsg, RowId, Section, SectionRow, can_next, section_list, slide_offsets};
use crate::app::menu::MenuTarget;
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{
    Row, button, checkbox, column, container, mouse_area, responsive, row, scrollable, space, stack,
};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::{Album, Genre, Playlist, Song};
use listen_core::settings::HomeFeatureStyle;
use listen_source::HomeShelf;

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let h = &state.library.home_state;
    let mut page = column![].spacing(32);

    if state.config.settings.home.feature && !h.feature.albums.is_empty() {
        page = page.push(feature(state));
    }
    let mut any = state.config.settings.home.feature && !h.feature.albums.is_empty();

    for (i, shelf) in state.library.home.iter().enumerate() {
        let cards = shelf_cards(shelf, i);
        if cards.is_empty() {
            continue;
        }
        any = true;
        page = page.push(carousel(state, RowId::Feed(i), &shelf.title, cards, h.feed_first(i), true));
    }

    for key in &state.config.settings.home.sections {
        let Some(section) = Section::from_key(key) else { continue };
        if section == Section::Genres {
            if state.supports_route(&Route::Genres) && !h.genres.is_empty() {
                any = true;
                page = page.push(genres(state));
            }
            continue;
        }
        let Some(row) = h.row(section).filter(|r| !r.is_empty()) else { continue };
        any = true;
        page = page.push(carousel(
            state,
            RowId::Section(section),
            section.title(),
            section_cards(row, section),
            row.first,
            row.done,
        ));
    }

    if !any {
        let text = if state.library.home_loaded || !h.rows.is_empty() {
            "Your library looks empty."
        } else {
            "Loading…"
        };
        page = page.push(label(text, metrics::font_md(), t.muted));
    }
    let page = page
        .padding(Padding {
            top: 48.0,
            right: metrics::PAGE_PADDING_X,
            bottom: 80.0,
            left: metrics::PAGE_PADDING_X,
        })
        .max_width(1800);
    scrollable(container(page).center_x(Length::Fill)).style(style::scrollbar(t)).height(Length::Fill).into()
}

// ---- feature carousel -----------------------------------------------------------

/// Albums per row of the "multiple" feature style (Feishin `getItemsPerRow`).
pub fn feature_cards(width: f32) -> usize {
    match width {
        w if w >= 2560.0 => 6,
        w if w >= 1440.0 => 5,
        w if w >= 1024.0 => 4,
        w if w >= 768.0 => 3,
        _ => 2,
    }
}

/// A round arrow button drawn over the carousel.
fn arrow<'a>(state: &Listen, glyph: Icon, to: Message) -> Element<'a, Message> {
    let b = button(icon(glyph, 26.0, Color::WHITE))
        .padding(6)
        .style(|_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Color { a: if hovered { 0.45 } else { 0.25 }, ..Color::BLACK }.into()),
                text_color: Color::WHITE,
                border: iced::border::rounded(999),
                ..Default::default()
            }
        })
        .on_press(to);
    crate::ui::tip::tip_icon(state, b, glyph, crate::ui::tip::Position::Bottom)
}

/// `body` with previous / next arrows on both sides and Shift + wheel stepping.
fn with_arrows<'a>(
    state: &Listen,
    body: Element<'a, Message>,
    step: usize,
    can_step: bool,
) -> Element<'a, Message> {
    let mut out = stack![body];
    if can_step {
        out = out.push(
            container(
                row![
                    arrow(state, Icon::LuChevronLeft, Message::Home(HomeMsg::Feature(-1, step))),
                    space::horizontal(),
                    arrow(state, Icon::LuChevronRight, Message::Home(HomeMsg::Feature(1, step))),
                ]
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([0.0, 8.0]))
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill),
        );
    }
    let mut area = mouse_area(out);
    // Only listen to the wheel with Shift held, so the page still scrolls otherwise.
    if state.modifiers.shift() && can_step {
        area = area.on_scroll(move |d| Message::Home(HomeMsg::FeatureWheel(wheel_amount(d), step)));
    }
    area.into()
}

fn wheel_amount(d: iced::mouse::ScrollDelta) -> f32 {
    match d {
        iced::mouse::ScrollDelta::Lines { x, y } | iced::mouse::ScrollDelta::Pixels { x, y } => {
            if y != 0.0 {
                y
            } else {
                x
            }
        }
    }
}

fn feature(state: &Listen) -> Element<'_, Message> {
    let f = &state.library.home_state.feature;
    match state.config.settings.home.feature_style {
        HomeFeatureStyle::Single => with_arrows(state, feature_slide(state), 1, f.albums.len() > 1),
        HomeFeatureStyle::Multiple => responsive(move |size| {
            let n = feature_cards(size.width);
            let f = &state.library.home_state.feature;
            let gap = metrics::space_md();
            let card_w = ((size.width - gap * (n as f32 - 1.0)) / n as f32).floor();
            let mut cards = Row::new().spacing(gap);
            for k in 0..n.min(f.albums.len()) {
                cards = cards.push(feature_card(state, &f.albums[(f.index + k) % f.albums.len()], card_w));
            }
            with_arrows(state, cards.into(), n, f.albums.len() > n)
        })
        .height(Length::Shrink)
        .into(),
    }
}

/// The single feature: the shown album, sliding in over the one it replaces
/// while an animation runs (Feishin animates the change).
fn feature_slide(state: &Listen) -> Element<'_, Message> {
    let f = &state.library.home_state.feature;
    let current = &f.albums[f.index.min(f.albums.len() - 1)];
    let Some(anim) = f.anim.filter(|a| !a.finished(iced::time::Instant::now()) && a.from < f.albums.len())
    else {
        return feature_single(state, current);
    };
    let leaving = &f.albums[anim.from];
    // The width of the page's column.
    let width = (state.page_width() - 2.0 * metrics::PAGE_PADDING_X).clamp(200.0, 1720.0);
    let (out_x, in_x) = slide_offsets(anim.progress(iced::time::Instant::now()), width, anim.dir);
    // Two windows side by side: the leaving album shows its far end, the entering one its near end.
    // (A scrollable clips; a container does not, and a pin escapes the clip.)
    let (leaving_w, entering_w) = (width - out_x.abs(), width - in_x.abs());
    if anim.dir >= 0 {
        // Next: the leaving album's right part stays on the left, the new one's left part follows.
        row![
            slide_window(state, leaving, width, leaving_w, true),
            slide_window(state, current, width, entering_w, false)
        ]
        .into()
    } else {
        // Previous: the new one's right part comes in on the left.
        row![
            slide_window(state, current, width, entering_w, true),
            slide_window(state, leaving, width, leaving_w, false)
        ]
        .into()
    }
}

/// `shown` px of an album's page `width` px wide: its end or its start.
fn slide_window<'a>(
    state: &'a Listen,
    album: &'a Album,
    width: f32,
    shown: f32,
    from_end: bool,
) -> Element<'a, Message> {
    use iced::widget::scrollable::{Anchor, Direction, Scrollbar};
    let bar = Scrollbar::hidden().anchor(if from_end { Anchor::End } else { Anchor::Start });
    iced::widget::scrollable(container(feature_single(state, album)).width(width))
        .direction(Direction::Horizontal(bar))
        .width(shown.max(0.0))
        .height(FEATURE_H)
        .into()
}

/// Height of the single feature.
const FEATURE_H: f32 = 248.0;

/// The dark gradient over a blurred cover (Feishin: art at .8 under the dominant colour at .7).
fn backdrop<'a>(
    state: &Listen,
    album: &Album,
    content: Element<'a, Message>,
    height: f32,
    padding: f32,
) -> Element<'a, Message> {
    let base = state.tokens.primary;
    let blurred = album.image.as_ref().and_then(|i| state.images.blurred(&state.image_key(i))).cloned();
    let has_art = blurred.is_some();
    let overlay = container(content).padding(padding).width(Length::Fill).height(height).style(move |_| {
        container::Style {
            background: Some(iced::Background::Gradient(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, style::alpha(style::darken(base, 0.45), if has_art { 0.55 } else { 1.0 }))
                    .add_stop(1.0, style::alpha(style::darken(base, 0.8), if has_art { 0.75 } else { 1.0 }))
                    .into(),
            )),
            border: iced::border::rounded(metrics::radius_md()),
            ..Default::default()
        }
    });
    match blurred {
        Some(handle) => stack![
            container(
                iced::widget::image(handle)
                    .width(Length::Fill)
                    .height(height)
                    .content_fit(iced::ContentFit::Fill)
                    .border_radius(metrics::radius_md())
                    .opacity(0.8_f32),
            )
            .width(Length::Fill)
            .height(height)
            .clip(true),
            overlay,
        ]
        .into(),
        None => overlay.into(),
    }
}

fn metadata(album: &Album, genres: usize) -> String {
    album
        .genres
        .iter()
        .take(genres)
        .cloned()
        .chain(album.year.map(|y| y.to_string()))
        .collect::<Vec<_>>()
        .join(" • ")
}

fn feature_single<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let white = Color::WHITE;
    let play = button(
        row![
            icon(Icon::LuPlay, metrics::font_lg(), Color::BLACK),
            label_w(state.play_behavior().button_label(), metrics::font_md(), Color::BLACK, Weight::Semibold)
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([9.0, 24.0]))
    .style(style::wide_play_button(state.tokens))
    .on_press(match state.play_behavior() {
        listen_core::settings::PlayBehavior::Now => Message::PlayAlbum(album.id.clone()),
        other => Message::PlayAlbumWith(Box::new(album.clone()), other),
    });
    // Page width minus the arrows' room, the cover and the gaps.
    let text_w = (state.page_width() - 80.0 - 200.0 - metrics::space_md() * 2.0 - 48.0).max(120.0);
    let content = row![
        super::with_menu(
            button(album_cover(state, album, 200.0, metrics::radius_md()))
                .padding(0)
                .style(style::bare_button(state.tokens))
                .on_press(Message::OpenAlbum(album.id.clone())),
            MenuTarget::album(album),
        ),
        column![
            crate::ui::header_title_fit(&album.name, white, text_w),
            label_w(ellipsize_px(&album.artist_line(), text_w, 16.0), 16.0, white, Weight::Bold),
            label_w(metadata(album, 2), metrics::font_sm(), white, Weight::Semibold),
            play,
        ]
        .spacing(metrics::space_sm())
        .width(text_w),
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::End);
    // Room for the arrows left and right of the content.
    backdrop(state, album, container(content).padding(Padding::from([0.0, 40.0])).into(), 248.0, 24.0)
}

fn feature_card<'a>(state: &'a Listen, album: &'a Album, width: f32) -> Element<'a, Message> {
    let white = Color::WHITE;
    let art = (width - 48.0).clamp(80.0, 220.0);
    let content = column![
        label_w(ellipsize(&album.name, (width / 8.0) as usize), metrics::font_lg(), white, Weight::Bold),
        super::with_menu(
            button(album_cover(state, album, art, metrics::radius_md()))
                .padding(0)
                .style(style::bare_button(state.tokens))
                .on_press(Message::OpenAlbum(album.id.clone())),
            MenuTarget::album(album),
        ),
        label_w(
            ellipsize(&album.artist_line(), (width / 7.5) as usize),
            metrics::font_md(),
            white,
            Weight::Medium
        ),
        label(metadata(album, 2), metrics::font_sm(), white),
    ]
    .spacing(metrics::space_sm())
    .align_x(Alignment::Center)
    .width(Length::Fill);
    container(backdrop(state, album, content.into(), art + 112.0, 12.0)).width(width).into()
}

// ---- grid carousels ----------------------------------------------------------------

/// A card of a carousel.
#[allow(clippy::large_enum_variant)]
enum Card<'a> {
    Album(&'a Album),
    Playlist(&'a Playlist),
    /// A song and what pressing it sends.
    Song(&'a Song, Message),
}

fn shelf_cards(shelf: &HomeShelf, index: usize) -> Vec<Card<'_>> {
    shelf
        .albums
        .iter()
        .map(Card::Album)
        .chain(shelf.playlists.iter().map(Card::Playlist))
        .chain(shelf.songs.iter().enumerate().map(|(i, s)| Card::Song(s, Message::PlayHomeSong(index, i))))
        .collect()
}

fn section_cards(row: &SectionRow, section: Section) -> Vec<Card<'_>> {
    row.albums
        .iter()
        .map(Card::Album)
        .chain(row.playlists.iter().map(Card::Playlist))
        .chain(
            row.songs
                .iter()
                .enumerate()
                .map(|(i, s)| Card::Song(s, Message::Home(HomeMsg::PlaySectionSong(section, i)))),
        )
        .collect()
}

/// Cards per row for the home grid carousels (Feishin breakpoints).
pub fn cards_per_row(width: f32) -> usize {
    match width {
        w if w >= 1440.0 => 8,
        w if w >= 1280.0 => 7,
        w if w >= 1152.0 => 6,
        w if w >= 960.0 => 5,
        w if w >= 720.0 => 4,
        w if w >= 520.0 => 3,
        _ => 2,
    }
}

fn carousel<'a>(
    state: &'a Listen,
    id: RowId,
    title: &'a str,
    cards: Vec<Card<'a>>,
    first: usize,
    done: bool,
) -> Element<'a, Message> {
    responsive(move |size| {
        let t = state.tokens;
        let n = cards_per_row(size.width);
        let gap = metrics::space_md();
        let card_w = ((size.width - gap * (n as f32 - 1.0)) / n as f32).floor();
        let len = cards.len();
        let first = first.min(len.saturating_sub(1));

        let nav = |glyph: Icon, enabled: bool, to: Message| {
            crate::ui::tip::tip_icon(
                state,
                button(icon(glyph, metrics::font_lg(), if enabled { t.foreground } else { t.muted }))
                    .padding(4)
                    .style(style::icon_button(t, false))
                    .on_press_maybe(enabled.then_some(to)),
                glyph,
                crate::ui::tip::Position::Bottom,
            )
        };
        let mut header = row![label_w(title, 24.0, t.foreground, Weight::Bold), space::horizontal()]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center);
        if let RowId::Section(section) = id {
            header = header.push(nav(Icon::LuRefreshCw, true, Message::Home(HomeMsg::Refresh(section))));
        }
        header = header
            .push(nav(Icon::LuChevronLeft, first > 0, Message::Home(HomeMsg::Page(id, -1, n))))
            .push(nav(
                Icon::LuChevronRight,
                can_next(first, n, len, done),
                Message::Home(HomeMsg::Page(id, 1, n)),
            ));

        let card = |art: Element<'a, Message>, on_press: Message, target: MenuTarget, lines: [String; 3]| {
            let [title, sub, extra] = lines;
            column![
                super::with_menu(
                    button(art).padding(0).style(style::bare_button(t)).on_press(on_press),
                    target
                ),
                // Every line is cut to the card and clipped: a long third line
                // must not run under the next card.
                container(
                    column![
                        label_w(
                            ellipsize_px(&title, card_w, metrics::font_md()),
                            metrics::font_md(),
                            t.foreground,
                            Weight::Medium
                        ),
                        label(ellipsize_px(&sub, card_w, metrics::font_sm()), metrics::font_sm(), t.muted),
                        label(ellipsize_px(&extra, card_w, metrics::font_sm()), metrics::font_sm(), t.muted),
                    ]
                    .spacing(metrics::space_xs()),
                )
                .width(card_w)
                .clip(true),
            ]
            .spacing(metrics::space_sm())
            .width(card_w)
        };
        let mut shown = Row::new().spacing(gap);
        for c in cards.iter().skip(first).take(n) {
            shown = shown.push(match c {
                Card::Album(album) => card(
                    album_cover(state, album, card_w, metrics::radius_md()),
                    Message::OpenAlbum(album.id.clone()),
                    MenuTarget::album(album),
                    [
                        album.name.clone(),
                        album.artist_line(),
                        album.year.map(|y| y.to_string()).unwrap_or_default(),
                    ],
                ),
                Card::Playlist(p) => card(
                    cover_on(
                        state,
                        &p.server_id,
                        p.image.as_ref(),
                        card_w,
                        metrics::radius_md(),
                        Icon::LuListMusic,
                    ),
                    Message::Navigate(Route::Playlist(p.id.clone())),
                    MenuTarget::playlist(p),
                    [p.name.clone(), p.owner.clone().unwrap_or_default(), String::new()],
                ),
                Card::Song(song, on_press) => card(
                    song_cover(state, song, card_w, metrics::radius_md()),
                    on_press.clone(),
                    MenuTarget::song(song),
                    [song.title.clone(), song.artist_line(), song.album.clone().unwrap_or_default()],
                ),
            });
        }
        let mut body: Element<'a, Message> = shown.into();
        // Shift + wheel steps through the row; without Shift the page scrolls.
        if state.modifiers.shift() && len > n {
            body = mouse_area(body)
                .on_scroll(move |d| Message::Home(HomeMsg::Wheel(id, wheel_amount(d), n)))
                .into();
        }
        // A drag along the row pages it (Feishin's carousels scroll by dragging).
        if len > n {
            let spacing = card_w + gap;
            body = super::drag_scroll::drag_scroll(body, move |dx| {
                Message::Home(HomeMsg::Drag(id, dx, n, spacing))
            });
        }
        column![header, body].spacing(metrics::space_md()).into()
    })
    .height(Length::Shrink)
    .into()
}

// ---- genres ------------------------------------------------------------------------

/// A stable colour per name and whether it is light (Feishin `stringToColor`).
pub fn genre_color(name: &str) -> (Color, bool) {
    let hash = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    let hue = (hash % 360) as f32;
    let (s, l) = (0.5_f32, 0.38_f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (Color::from_rgb(r + m, g + m, b + m), false)
}

/// Genre tiles shown for a container width, and their columns (Feishin `getGenresToShow`).
pub fn genre_grid(width: f32) -> (usize, usize) {
    match width {
        w if w >= 1500.0 => (18, 6),
        w if w >= 1200.0 => (15, 5),
        w if w >= 900.0 => (12, 4),
        w if w >= 600.0 => (8, 4),
        _ => (6, 2),
    }
}

fn genre_tile<'a>(genre: &'a Genre, width: f32) -> Element<'a, Message> {
    let (color, _) = genre_color(&genre.name);
    let play = button(icon(Icon::LuPlay, metrics::font_md(), Color::BLACK))
        .padding(8)
        .style(|_, _| button::Style {
            background: Some(Color::WHITE.into()),
            border: iced::border::rounded(999),
            ..Default::default()
        })
        .on_press(Message::Home(HomeMsg::PlayGenre(genre.name.clone())));
    let chars = (width / 9.0) as usize;
    button(
        row![
            label_w(
                ellipsize(&genre.name, chars.saturating_sub(5)),
                metrics::font_lg(),
                Color::WHITE,
                Weight::Bold
            ),
            space::horizontal(),
            play,
        ]
        .align_y(Alignment::Center),
    )
    .width(width)
    .height(72)
    .padding(metrics::space_md())
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(style::lighten(color, if hovered { 0.08 } else { 0.0 }).into()),
            text_color: Color::WHITE,
            border: iced::border::rounded(metrics::radius_md()),
            ..Default::default()
        }
    })
    .on_press(Message::Navigate(Route::Genre(genre.name.clone())))
    .into()
}

fn genres(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    responsive(move |size| {
        let (count, columns) = genre_grid(size.width);
        let gap = metrics::space_md();
        let width = ((size.width - gap * (columns as f32 - 1.0)) / columns as f32).floor();
        let header = row![
            label_w("Genres", 24.0, t.foreground, Weight::Bold),
            space::horizontal(),
            button(label_w("View more", metrics::font_sm(), t.foreground, Weight::Medium))
                .padding(Padding::from([4.0, 10.0]))
                .style(style::icon_button(t, false))
                .on_press(Message::Navigate(Route::Genres)),
        ]
        .align_y(Alignment::Center);
        let mut grid = iced::widget::Column::new().spacing(gap);
        let genres: Vec<&Genre> = state.library.home_state.genres.iter().take(count).collect();
        for chunk in genres.chunks(columns) {
            let mut r = Row::new().spacing(gap);
            for g in chunk {
                r = r.push(genre_tile(g, width));
            }
            grid = grid.push(r);
        }
        column![header, grid].spacing(metrics::space_md()).into()
    })
    .height(Length::Shrink)
    .into()
}

// ---- settings ----------------------------------------------------------------------

/// Settings → General → Home: the sections to show, in order.
pub fn section_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    let t = state.tokens;
    let list = section_list(&state.config.settings.home.sections);
    let visible = list.iter().filter(|(_, v)| *v).count();
    let arrow = |glyph: Icon, to: Option<Message>| {
        crate::ui::tip::tip_icon(
            state,
            button(icon(glyph, metrics::font_md(), t.muted))
                .padding(4)
                .style(style::icon_button(t, false))
                .on_press_maybe(to),
            glyph,
            crate::ui::tip::Position::Left,
        )
    };
    list.into_iter()
        .enumerate()
        .map(|(i, (section, on))| {
            row![
                checkbox(on)
                    .label(section.setting_label())
                    .on_toggle(move |v| Message::Home(HomeMsg::SectionVisible(section, v)))
                    .size(18)
                    .text_size(metrics::font_md()),
                space::horizontal(),
                arrow(Icon::LuChevronUp, (on && i > 0).then_some(Message::Home(HomeMsg::SectionMove(i, -1)))),
                arrow(
                    Icon::LuChevronDown,
                    (on && i + 1 < visible).then_some(Message::Home(HomeMsg::SectionMove(i, 1)))
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .padding(Padding::from([0.0, 8.0]))
            .into()
        })
        .collect()
}
