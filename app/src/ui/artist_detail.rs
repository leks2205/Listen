//! Artist page (Feishin `album-artist-detail-*`): header, links, genres, biography,
//! discography by release type, similar artists, top and favourite songs, in the
//! order and visibility of the `artist` settings. Plus the discography, all-tracks,
//! top-songs and favourite-songs sub-pages.

use super::list::ListView;
use super::{album_cover, cover, ellipsize, icon, label, label_w, plural};
use crate::app::artist_page::{
    self, ArtistMsg, ArtistView, FavMode, Grouping, Section, TopMode, bio_text, parse_bio, site_links,
    total_time,
};
use crate::app::list::ListId;
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{
    Column, Row, button, column, container, rich_text, row, scrollable, space, span, text_input,
};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::{Album, Artist, ImageRef};
use listen_core::settings::{ArtistSection, PlayBehavior};
use listen_source::Capabilities;
use std::time::Duration;

/// Cards shown per release-type group before "View more" (Feishin `MAX_SECTION_CARDS`).
const MAX_CARDS: usize = 100;
/// Song rows in the top / favourite sections; "View all" shows the rest.
const SECTION_ROWS: usize = 10;
/// Bio height before "Show more" (Feishin's Spoiler).
const BIO_HEIGHT: f32 = 96.0;
const SIDE: f32 = 32.0;

fn msg(m: ArtistMsg) -> Message {
    Message::Artist(m)
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let Some(detail) = &state.library.artist else {
        return container(label("Loading…", metrics::font_md(), t.muted)).padding(48).into();
    };
    let artist = &detail.artist;
    let mut body = Column::new().spacing(32).push(action_links(state, artist));
    if let Some(genres) = genres(state) {
        body = body.push(genres);
    }
    if let Some(links) = links(state) {
        body = body.push(links);
    }
    for section in state.artist_sections() {
        let element = match section {
            ArtistSection::Biography => biography(state),
            ArtistSection::RecentAlbums => {
                let mut col = Column::new().spacing(32).push(discography(state));
                if !state.artist_page.videos.is_empty() {
                    col = col.push(videos(state));
                }
                Some(col.into())
            }
            ArtistSection::SimilarArtists => similar(state),
            ArtistSection::TopSongs => top_songs(state),
            ArtistSection::FavoriteSongs => favorite_songs(state),
        };
        if let Some(e) = element {
            body = body.push(e);
        }
    }
    scrollable(
        container(column![
            header(state, artist),
            body.padding(Padding { top: 0.0, right: SIDE, bottom: 80.0, left: SIDE })
        ])
        .max_width(1800)
        .center_x(Length::Fill),
    )
    .style(style::scrollbar(t))
    .height(Length::Fill)
    .into()
}

// ---- header ----------------------------------------------------------------

fn pill<'a>(state: &Listen, text: &'static str, on_press: Message) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(text, metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(Padding::from([9.0, 18.0]))
        .style(style::default_button(t))
        .on_press(on_press)
        .into()
}

fn icon_btn<'a>(state: &Listen, glyph: Icon, active: bool, on_press: Message) -> Element<'a, Message> {
    let t = state.tokens;
    let b = button(icon(glyph, metrics::font_xl(), if active { t.primary } else { t.foreground }))
        .padding(8)
        .style(style::icon_button(t, active))
        .on_press(on_press);
    // The heart reads "Remove" while the artist is a favorite.
    if glyph == Icon::LuHeart {
        let name = if active { "Remove from favorites" } else { "Add to favorites" };
        return crate::ui::tip::tip(state, b, name, None, crate::ui::tip::Position::Bottom);
    }
    crate::ui::tip::tip_icon(state, b, glyph, crate::ui::tip::Position::Bottom)
}

/// "4 albums · 52 tracks · 3 hr 20 min" (the counts the server gave, or sums over the albums).
fn facts(artist: &Artist, albums: &[Album]) -> Vec<String> {
    let mut out = vec![plural(artist.album_count.map_or(albums.len(), |n| n as usize), "album")];
    let tracks = artist.song_count.or_else(|| {
        let n: u32 = albums.iter().filter_map(|a| a.song_count).sum();
        (n > 0).then_some(n)
    });
    if let Some(n) = tracks {
        out.push(plural(n as usize, "track"));
    }
    let total: Duration = albums.iter().map(|a| a.duration).sum();
    if total > Duration::ZERO {
        out.push(total_time(total));
    }
    out
}

fn header<'a>(state: &'a Listen, artist: &'a Artist) -> Element<'a, Message> {
    let t = state.tokens;
    let detail = state.library.artist.as_ref();
    let albums = detail.map(|d| d.albums.as_slice()).unwrap_or_default();
    let caps = state.artist_caps();
    let page = &state.artist_page;

    let behavior = state.play_behavior();
    let play = button(
        row![
            icon(Icon::LuPlay, metrics::font_lg(), Color::BLACK),
            label_w(behavior.button_label(), metrics::font_md(), Color::BLACK, Weight::Semibold)
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([9.0, 24.0]))
    .style(style::wide_play_button(t))
    .on_press(msg(ArtistMsg::Play(behavior)));
    // A long press shuffles (the page's Shuffle button is the shuffled Play now).
    let play = if behavior == PlayBehavior::Now {
        super::long_press::long_press(play, msg(ArtistMsg::Shuffle))
    } else {
        play.into()
    };

    let mut actions = Row::new()
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .push(play)
        .push(pill(state, "Shuffle", msg(ArtistMsg::Shuffle)))
        .push(pill(state, "Add next", msg(ArtistMsg::Play(PlayBehavior::Next))))
        .push(pill(state, "Add last", msg(ArtistMsg::Play(PlayBehavior::Last))));
    if caps.intersects(Capabilities::RADIO | Capabilities::SIMILAR_SONGS) {
        actions = actions.push(icon_btn(state, Icon::LuRadio, false, msg(ArtistMsg::Radio)));
    }
    if caps.contains(Capabilities::FAVORITE_ITEMS) {
        actions =
            actions.push(icon_btn(state, Icon::LuHeart, artist.favorite, msg(ArtistMsg::ToggleFavorite)));
    }
    // Feishin rates artists on Navidrome only.
    if caps.contains(Capabilities::RATINGS) && artist.source == listen_core::model::SourceKind::Subsonic {
        let mut stars = Row::new().align_y(Alignment::Center);
        for n in 1..=5u8 {
            let on = artist.rating.is_some_and(|r| r >= n);
            stars = stars.push(crate::ui::tip::tip_raw(
                state,
                button(icon(
                    if on { Icon::LuStarFill } else { Icon::LuStar },
                    metrics::font_lg(),
                    if on { t.primary } else { t.muted },
                ))
                .padding(3)
                .style(style::bare_button(t))
                .on_press(msg(ArtistMsg::Rate(n))),
                &crate::ui::tip::rate_text(n),
                None,
                crate::ui::tip::Position::Bottom,
            ));
        }
        actions = actions.push(stars);
    }
    actions = actions.push(icon_btn(state, Icon::LuEllipsis, page.more_open, msg(ArtistMsg::More)));

    let mut info = Column::new()
        .spacing(metrics::space_sm())
        .push(label_w("ARTIST", metrics::font_md(), t.foreground, Weight::Semibold))
        .push(super::header_title(&artist.name, t.foreground))
        .push(label(facts(artist, albums).join(" · "), metrics::font_md(), t.muted))
        .push(actions);
    if page.more_open {
        info = info.push(
            row![
                pill(state, "Refresh", msg(ArtistMsg::Refresh)),
                pill(state, "Copy name", msg(ArtistMsg::CopyName)),
            ]
            .spacing(metrics::space_sm()),
        );
    }

    let header =
        row![cover(state, artist.image.as_ref(), 250.0, 125.0, Icon::LuUser), info.width(Length::Fill)]
            .spacing(metrics::space_md())
            .align_y(Alignment::End);

    iced::widget::stack![
        super::header_background(
            state,
            artist.image.as_ref(),
            362.0,
            crate::app::appearance::BlurKind::Artist
        ),
        container(header).padding(Padding { top: 80.0, right: SIDE, bottom: 32.0, left: SIDE }),
    ]
    .into()
}

// ---- links, genres ---------------------------------------------------------

fn text_link<'a>(
    state: &Listen,
    text: &'static str,
    glyph: Option<Icon>,
    on_press: Message,
) -> Element<'a, Message> {
    let t = state.tokens;
    let mut r = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    if let Some(g) = glyph {
        r = r.push(icon(g, metrics::font_md(), t.primary));
    }
    button(r.push(label_w(text, metrics::font_md(), t.primary, Weight::Semibold)))
        .padding(0)
        .style(style::bare_button(t))
        .on_press(on_press)
        .into()
}

/// VIEW DISCOGRAPHY / VIEW ALL TRACKS / ARTIST RADIO.
fn action_links<'a>(state: &Listen, artist: &Artist) -> Element<'a, Message> {
    let caps = state.artist_caps();
    let id = artist.id.clone();
    let mut r = Row::new().spacing(metrics::space_lg()).padding(Padding { top: 8.0, ..Padding::ZERO }).push(
        text_link(
            state,
            "VIEW DISCOGRAPHY",
            None,
            Message::Navigate(Route::ArtistList(id.clone(), ArtistView::Discography)),
        ),
    );
    r = r.push(text_link(
        state,
        "VIEW ALL TRACKS",
        None,
        Message::Navigate(Route::ArtistList(id, ArtistView::Tracks)),
    ));
    if caps.intersects(Capabilities::RADIO | Capabilities::SIMILAR_SONGS) {
        r = r.push(text_link(state, "ARTIST RADIO", Some(Icon::LuRadio), msg(ArtistMsg::Radio)));
    }
    r.into()
}

fn small_title<'a>(state: &Listen, text: String) -> Element<'a, Message> {
    label_w(text, metrics::font_sm(), state.tokens.foreground, Weight::Semibold).into()
}

/// The genres: the server's, or the ones the albums have most.
fn genre_names(state: &Listen) -> Vec<String> {
    if let Some(info) = state.artist_page.info.as_ref().filter(|i| !i.genres.is_empty()) {
        return info.genres.clone();
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    let albums = state.library.artist.iter().flat_map(|d| d.albums.iter());
    for g in albums.flat_map(|a| a.genres.iter()) {
        match counts.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(g)) {
            Some((_, c)) => *c += 1,
            None => counts.push((g.clone(), 1)),
        }
    }
    counts.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    counts.into_iter().take(8).map(|(n, _)| n).collect()
}

fn genres(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let names = genre_names(state);
    if names.is_empty() {
        return None;
    }
    let linked = state.artist_caps().contains(Capabilities::GENRES);
    let mut chips = Row::new().spacing(metrics::space_sm());
    let title = if names.len() == 1 { "GENRE" } else { "GENRES" };
    for name in names {
        let b = button(label(name.clone(), metrics::font_md(), t.foreground))
            .padding(Padding::from([5.0, 12.0]))
            .style(style::default_button(t));
        chips = chips.push(if linked { b.on_press(Message::Navigate(Route::Genre(name))) } else { b });
    }
    Some(column![small_title(state, title.into()), chips.wrap()].spacing(metrics::space_xs()).into())
}

fn links(state: &Listen) -> Option<Element<'_, Message>> {
    let artist = &state.library.artist.as_ref()?.artist;
    let info = state.artist_page.info.as_ref();
    let mbid = info.and_then(|i| i.musicbrainz_id.as_deref());
    let lastfm = info.and_then(|i| i.links.lastfm_url.as_deref());
    let sites = site_links(&state.config.settings.artist, &artist.name, mbid, lastfm);
    if sites.is_empty() {
        return None;
    }
    let mut r = Row::new().spacing(metrics::space_sm());
    for s in sites {
        r = r.push(crate::ui::site_icon::site_button(state, s.name, msg(ArtistMsg::Link(s.url))));
    }
    Some(column![small_title(state, "EXTERNAL LINKS".into()), r.wrap()].spacing(metrics::space_xs()).into())
}

// ---- sections --------------------------------------------------------------

/// A section's title row: title, count badge, a rule, and the controls on the right.
fn section_title<'a>(
    state: &Listen,
    title: String,
    count: Option<usize>,
    right: Element<'a, Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let mut r = Row::new().spacing(metrics::space_md()).align_y(Alignment::Center);
    r = r.push(label_w(title, 24.0, t.foreground, Weight::Bold));
    if let Some(n) = count {
        r = r.push(
            container(label(n.to_string(), metrics::font_sm(), t.foreground))
                .padding(Padding::from([2.0, 8.0]))
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.surface)),
                    border: iced::Border { radius: 999.0.into(), ..Default::default() },
                    ..Default::default()
                }),
        );
    }
    r.push(container(space()).width(Length::Fill).height(1).style(move |_| container::Style {
        background: Some(iced::Background::Color(t.border)),
        ..Default::default()
    }))
    .push(right)
    .into()
}

/// Play now / next / last as three small icon buttons.
fn play_icons<'a>(state: &Listen, now: Message, next: Message, last: Message) -> Element<'a, Message> {
    let t = state.tokens;
    let b = |g: Icon, m: Message, name: &str, action: &str| {
        crate::ui::tip::tip(
            state,
            button(icon(g, metrics::font_lg(), t.foreground))
                .padding(5)
                .style(style::icon_button(t, false))
                .on_press(m),
            name,
            Some(action),
            crate::ui::tip::Position::Bottom,
        )
    };
    row![
        b(Icon::LuPlay, now, "Play now", "listPlayNow"),
        b(Icon::LuStepForward, next, "Play next", "listPlayNext"),
        b(Icon::LuPlus, last, "Play last", "listPlayLast")
    ]
    .into()
}

fn toggle<'a>(state: &Listen, text: &'static str, active: bool, on_press: Message) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(text, metrics::font_sm(), if active { t.primary } else { t.foreground }, Weight::Semibold))
        .padding(Padding::from([5.0, 12.0]))
        .style(style::nav_item(t, active))
        .on_press(on_press)
        .into()
}

fn search_box<'a>(state: &Listen, value: &str, on_input: fn(String) -> ArtistMsg) -> Element<'a, Message> {
    text_input(crate::i18n::tr("Search"), value)
        .on_input(move |v| msg(on_input(v)))
        .style(style::input(state.tokens))
        .width(200)
        .into()
}

fn biography(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let detail = state.library.artist.as_ref()?;
    let html = state
        .artist_page
        .info
        .as_ref()
        .and_then(|i| i.biography.clone())
        .or_else(|| detail.artist.biography.clone())?;
    let spans = parse_bio(&html);
    if spans.is_empty() {
        return None;
    }
    let long = bio_text(&spans).chars().count() > 360;
    let font = crate::appearance::current_font();
    let rich: Vec<_> = spans
        .into_iter()
        .map(|s| {
            let sp = span(s.text).size(metrics::font_md()).font(font);
            match s.link {
                Some(url) => sp.color(t.primary).underline(true).link(url),
                None => sp.color(t.muted),
            }
        })
        .collect();
    let text = rich_text(rich).on_link_click(|url: String| msg(ArtistMsg::Link(url)));
    let open = state.artist_page.bio_open;
    let body: Element<'_, Message> =
        if long && !open { container(text).max_height(BIO_HEIGHT).clip(true).into() } else { text.into() };
    let mut col = Column::new()
        .spacing(metrics::space_sm())
        .max_width(1280)
        .push(label_w(format!("About {}", detail.artist.name), 24.0, t.foreground, Weight::Bold))
        .push(body);
    if long {
        col = col.push(text_link(
            state,
            if open { "Show less" } else { "Show more" },
            None,
            msg(ArtistMsg::ToggleBio),
        ));
    }
    Some(col.into())
}

fn album_card<'a>(state: &Listen, album: &Album, card: f32, chars: usize) -> Element<'a, Message> {
    let t = state.tokens;
    column![
        super::with_menu(
            button(album_cover(state, album, card, metrics::radius_md()))
                .padding(0)
                .style(style::bare_button(t))
                .on_press(Message::OpenAlbum(album.id.clone())),
            crate::app::menu::MenuTarget::album(album)
        ),
        label_w(ellipsize(&album.name, chars), metrics::font_md(), t.foreground, Weight::Medium),
        label(album.year.map(|y| y.to_string()).unwrap_or_default(), metrics::font_sm(), t.muted),
    ]
    .spacing(metrics::space_xs())
    .width(card)
    .into()
}

/// Width the cards share: the page minus the side padding.
fn content_width(state: &Listen) -> f32 {
    (state.page_width() - 2.0 * SIDE + 24.0).max(300.0)
}

fn discography(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let page = &state.artist_page;
    let groups = state.artist_groups();
    let no_groups = groups.is_empty();
    let ascending = page.ascending;

    let mut sort = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    for by in artist_page::AlbumSortBy::ALL {
        sort = sort.push(toggle(state, by.label(), page.sort == by, msg(ArtistMsg::AlbumSort(by))));
    }
    let order = icon_btn(
        state,
        if ascending { Icon::LuArrowUp } else { Icon::LuArrowDown },
        false,
        msg(ArtistMsg::AlbumOrder),
    );
    let settings = icon_btn(state, Icon::LuSlidersHorizontal, page.group_menu, msg(ArtistMsg::GroupMenu));
    let controls =
        row![search_box(state, &page.album_search, ArtistMsg::AlbumSearch), sort.wrap(), order, settings]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center);

    let mut col = Column::new().spacing(metrics::space_lg()).push(controls);
    if page.group_menu {
        col = col.push(
            row![
                label("Group by", metrics::font_md(), t.muted),
                toggle(
                    state,
                    "Primary types",
                    page.grouping == Grouping::Primary,
                    msg(ArtistMsg::Grouping(Grouping::Primary))
                ),
                toggle(
                    state,
                    "All types",
                    page.grouping == Grouping::All,
                    msg(ArtistMsg::Grouping(Grouping::All))
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
    }
    let layout = super::albums::grid_layout(content_width(state));
    let chars = (layout.card / 7.5) as usize;
    for group in groups {
        let shown = if page.show_all.contains(&group.key) {
            group.albums.len()
        } else {
            group.albums.len().min(MAX_CARDS)
        };
        let mut grid = Column::new().spacing(16);
        for chunk in group.albums[..shown].chunks(layout.columns) {
            let mut r = Row::new().spacing(16);
            for album in chunk {
                r = r.push(album_card(state, album, layout.card, chars));
            }
            grid = grid.push(r);
        }
        let key = group.key.clone();
        let mut section = Column::new().spacing(metrics::space_md()).push(section_title(
            state,
            group.title.clone(),
            Some(group.albums.len()),
            play_icons(
                state,
                msg(ArtistMsg::PlayGroup(key.clone(), PlayBehavior::Now)),
                msg(ArtistMsg::PlayGroup(key.clone(), PlayBehavior::Next)),
                msg(ArtistMsg::PlayGroup(key.clone(), PlayBehavior::Last)),
            ),
        ));
        section = section.push(grid);
        if shown < group.albums.len() {
            section = section.push(
                container(pill(state, "View more", msg(ArtistMsg::ShowAll(key)))).center_x(Length::Fill),
            );
        }
        col = col.push(section);
    }
    if no_groups {
        col = col.push(label("No releases.", metrics::font_md(), t.muted));
    }
    col.into()
}

/// A carousel of artist cards (Feishin's grid carousel), paged with arrows.
fn similar(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let detail = state.library.artist.as_ref()?;
    struct Card<'a> {
        id: Option<&'a str>,
        name: &'a str,
        image: Option<&'a ImageRef>,
    }
    let info = state.artist_page.info.as_ref();
    let mut cards: Vec<Card<'_>> = match info.filter(|i| !i.similar_artists.is_empty()) {
        Some(i) => i
            .similar_artists
            .iter()
            .map(|a| Card { id: Some(a.id.as_str()), name: &a.name, image: a.image.as_ref() })
            .collect(),
        None => {
            let refs = info
                .map(|i| i.similar.as_slice())
                .filter(|s| !s.is_empty())
                .unwrap_or(&detail.artist.similar);
            refs.iter().map(|r| Card { id: r.id.as_deref(), name: &r.name, image: None }).collect()
        }
    };
    cards.retain(|c| c.id != Some(detail.artist.id.as_str()));
    if cards.is_empty() {
        return None;
    }
    let layout = super::albums::grid_layout(content_width(state));
    let per = layout.columns;
    let pages = cards.len().div_ceil(per);
    let page = state.artist_page.similar_page.min(pages - 1);
    let chars = (layout.card / 7.5) as usize;
    let mut r = Row::new().spacing(16);
    for c in cards.iter().skip(page * per).take(per) {
        let img = cover(state, c.image, layout.card, layout.card / 2.0, Icon::LuUser);
        let top: Element<'_, Message> = match c.id {
            Some(id) => button(img)
                .padding(0)
                .style(style::bare_button(t))
                .on_press(Message::Navigate(Route::Artist(id.to_string())))
                .into(),
            None => img,
        };
        r = r.push(
            column![top, label_w(ellipsize(c.name, chars), metrics::font_md(), t.foreground, Weight::Medium)]
                .spacing(metrics::space_xs())
                .width(layout.card),
        );
    }
    let arrows: Element<'_, Message> = if pages > 1 {
        row![
            crate::ui::tip::tip(
                state,
                button(icon(Icon::LuChevronLeft, metrics::font_lg(), t.foreground))
                    .padding(5)
                    .style(style::icon_button(t, false))
                    .on_press_maybe((page > 0).then(|| msg(ArtistMsg::SimilarPage(-1)))),
                "Previous",
                None,
                crate::ui::tip::Position::Bottom,
            ),
            crate::ui::tip::tip(
                state,
                button(icon(Icon::LuChevronRight, metrics::font_lg(), t.foreground))
                    .padding(5)
                    .style(style::icon_button(t, false))
                    .on_press_maybe((page + 1 < pages).then(|| msg(ArtistMsg::SimilarPage(1)))),
                "Next",
                None,
                crate::ui::tip::Position::Bottom,
            ),
        ]
        .into()
    } else {
        space().into()
    };
    Some(
        column![section_title(state, "Related artists".into(), None, arrows), r]
            .spacing(metrics::space_md())
            .into(),
    )
}

fn song_rows<'a>(
    state: &'a Listen,
    section: Section,
    songs: &'a [listen_core::model::Song],
) -> Element<'a, Message> {
    let mut col = Column::new();
    for (i, song) in songs.iter().enumerate().take(SECTION_ROWS) {
        col = col.push(super::songs::song_row(state, i, song, msg(ArtistMsg::PlayAt(section, i))));
    }
    col.into()
}

fn top_songs(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let id = state.library.artist.as_ref()?.artist.id.clone();
    let page = &state.artist_page;
    let community = state.library.artist.as_ref().is_some_and(|d| !d.top_songs.is_empty());
    if !community && page.personal.is_empty() {
        return None;
    }
    let both = community && !page.personal.is_empty();
    let mut controls = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    controls = controls.push(search_box(state, &page.top_search, ArtistMsg::TopSearch));
    if both || page.top_mode == TopMode::Personal {
        controls = controls
            .push(toggle(
                state,
                "Community",
                page.top_mode == TopMode::Community,
                msg(ArtistMsg::TopMode(TopMode::Community)),
            ))
            .push(toggle(
                state,
                "Personal",
                page.top_mode == TopMode::Personal,
                msg(ArtistMsg::TopMode(TopMode::Personal)),
            ));
    }
    controls = controls
        .push(play_icons(
            state,
            msg(ArtistMsg::PlaySection(Section::Top, PlayBehavior::Now)),
            msg(ArtistMsg::PlaySection(Section::Top, PlayBehavior::Next)),
            msg(ArtistMsg::PlaySection(Section::Top, PlayBehavior::Last)),
        ))
        .push(text_link(
            state,
            "View all",
            None,
            Message::Navigate(Route::ArtistList(id, ArtistView::TopSongs)),
        ));
    let body: Element<'_, Message> = if page.top_view.is_empty() {
        label(
            if page.top_mode == TopMode::Personal && page.personal.is_empty() {
                "Nothing played yet."
            } else {
                "No results."
            },
            metrics::font_md(),
            t.muted,
        )
        .into()
    } else {
        song_rows(state, Section::Top, &page.top_view)
    };
    Some(
        column![section_title(state, "Top songs".into(), Some(page.top_all.len()), controls.into()), body]
            .spacing(metrics::space_md())
            .into(),
    )
}

fn favorite_songs(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let id = state.library.artist.as_ref()?.artist.id.clone();
    let page = &state.artist_page;
    let caps = state.artist_caps();
    let can_rate = caps.contains(Capabilities::RATINGS);
    let have_favorites = page.favorites.as_ref().is_some_and(|f| !f.is_empty());
    let have_rated = page.rated.as_ref().is_some_and(|f| !f.is_empty());
    if !have_favorites && !have_rated {
        return None;
    }
    let mut controls = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    controls = controls.push(search_box(state, &page.fav_search, ArtistMsg::FavSearch));
    if can_rate {
        controls = controls
            .push(toggle(
                state,
                "Favorite",
                page.fav_mode == FavMode::Favorite,
                msg(ArtistMsg::FavMode(FavMode::Favorite)),
            ))
            .push(toggle(
                state,
                "Rating",
                page.fav_mode == FavMode::Rating,
                msg(ArtistMsg::FavMode(FavMode::Rating)),
            ));
    }
    controls = controls
        .push(play_icons(
            state,
            msg(ArtistMsg::PlaySection(Section::Favorite, PlayBehavior::Now)),
            msg(ArtistMsg::PlaySection(Section::Favorite, PlayBehavior::Next)),
            msg(ArtistMsg::PlaySection(Section::Favorite, PlayBehavior::Last)),
        ))
        .push(text_link(
            state,
            "View all",
            None,
            Message::Navigate(Route::ArtistList(id, ArtistView::FavoriteSongs)),
        ));
    let body: Element<'_, Message> = if page.fav_view.is_empty() {
        label("No results.", metrics::font_md(), t.muted).into()
    } else {
        song_rows(state, Section::Favorite, &page.fav_view)
    };
    Some(
        column![
            section_title(state, "Favorite songs".into(), Some(page.fav_all.len()), controls.into()),
            body
        ]
        .spacing(metrics::space_md())
        .into(),
    )
}

/// YouTube Music's Videos shelf.
fn videos(state: &Listen) -> Element<'_, Message> {
    let page = &state.artist_page;
    column![
        section_title(
            state,
            "Videos".into(),
            Some(page.videos.len()),
            play_icons(
                state,
                msg(ArtistMsg::PlaySection(Section::Videos, PlayBehavior::Now)),
                msg(ArtistMsg::PlaySection(Section::Videos, PlayBehavior::Next)),
                msg(ArtistMsg::PlaySection(Section::Videos, PlayBehavior::Last)),
            )
        ),
        song_rows(state, Section::Videos, &page.videos),
    ]
    .spacing(metrics::space_md())
    .into()
}

// ---- sub-pages -------------------------------------------------------------

/// Discography, all tracks, and the "View all" pages of the top and favourite songs.
pub fn sub_page<'a>(state: &'a Listen, id: &str, view: ArtistView) -> Element<'a, Message> {
    let t = state.tokens;
    let name = state.library.artist.as_ref().filter(|d| d.artist.id == id).map(|d| d.artist.name.clone());
    let Some(name) = name else {
        return container(label("Loading…", metrics::font_md(), t.muted)).padding(48).into();
    };
    let back = button(
        row![
            icon(Icon::LuChevronLeft, metrics::font_md(), t.primary),
            label_w(name.clone(), metrics::font_md(), t.primary, Weight::Semibold)
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center),
    )
    .padding(0)
    .style(style::bare_button(t))
    .on_press(Message::Navigate(Route::Artist(id.to_string())));
    let page = &state.artist_page;
    let list: Element<'a, Message> = match view {
        ArtistView::Discography => {
            ListView::new(state, ListId::ArtistAlbums(id.to_string()), &page.discography)
                .title("Discography")
                .subtitle(plural(page.discography.len(), "album"))
                .empty("No releases.")
                .view()
        }
        ArtistView::Tracks => {
            let songs = page.all_songs.as_deref().unwrap_or_default();
            ListView::new(state, ListId::Artist(id.to_string()), songs)
                .title("All tracks")
                .subtitle(plural(songs.len(), "track"))
                .loading(page.all_songs.is_none())
                .empty("No tracks.")
                .view()
        }
        ArtistView::TopSongs => ListView::new(state, ListId::Artist(id.to_string()), &page.top_all)
            .title("Top songs")
            .subtitle(plural(page.top_all.len(), "song"))
            .empty("No songs.")
            .view(),
        ArtistView::FavoriteSongs => ListView::new(state, ListId::Artist(id.to_string()), &page.fav_all)
            .title("Favorite songs")
            .subtitle(plural(page.fav_all.len(), "song"))
            .empty("No songs.")
            .view(),
    };
    column![
        container(back).padding(Padding { top: 16.0, left: metrics::PAGE_PADDING_X, ..Padding::ZERO }),
        list
    ]
    .into()
}
