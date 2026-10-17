//! The "album group" of a song table (Feishin `album-group-column.tsx` and friends):
//! consecutive songs of one album sit under a header row ([`super::RowGroup`]) with the
//! album's cover, facts and favourite / rating. Configured in `settings.lists.album_group`.

use super::columns::{Cx, artist_links, link};
use super::{GroupLook, RowGroup};
use crate::app::list::{ListId, ListMsg};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::album_detail::format::{duration_words, partial_date, size_string};
use crate::ui::tip::{Position, tip, tip_raw};
use crate::ui::{ellipsize_px, icon, label, song_cover};
use iced::widget::{Column, Row, button, container};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::Song;
use listen_core::settings::{AlbumGroupConfig, AlbumGroupItem};
use std::rc::Rc;
use std::time::Duration;

const TITLE_H: f32 = 24.0;
const FACT_H: f32 = 18.0;
const CONTROLS_H: f32 = 30.0;
const PAD_Y: f32 = 8.0;

/// What the header of one run of songs shows.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupMeta {
    /// Index of the run's first song in the list (global).
    pub first: usize,
    pub count: usize,
    /// The first song, for the cover and the ids.
    pub song: Song,
    pub genres: Vec<String>,
    pub duration: Duration,
    pub size: u64,
    /// Lines of the header (what the config enables and the songs have).
    pub lines: usize,
    pub height: f32,
}

/// The runs of consecutive songs of the same album among `songs[from..to]`
/// (Feishin compares the album names). Songs without an album end a run and
/// get no header.
pub fn runs(songs: &[Song], from: usize, to: usize) -> Vec<(usize, usize)> {
    let to = to.min(songs.len());
    let mut out: Vec<(usize, usize)> = Vec::new();
    for i in from..to {
        let Some(album) = songs[i].album.as_deref() else { continue };
        match out.last_mut() {
            Some((start, count)) if *start + *count == i && songs[*start].album.as_deref() == Some(album) => {
                *count += 1;
            }
            _ => out.push((i, 1)),
        }
    }
    out
}

fn meta(songs: &[Song], first: usize, count: usize, cfg: &AlbumGroupConfig, controls: bool) -> GroupMeta {
    let run = &songs[first..first + count];
    let mut genres: Vec<String> = Vec::new();
    for g in run.iter().flat_map(|s| &s.genres) {
        if !genres.contains(g) {
            genres.push(g.clone());
        }
    }
    let mut m = GroupMeta {
        first,
        count,
        song: run[0].clone(),
        genres,
        duration: run.iter().map(|s| s.duration).sum(),
        size: run.iter().filter_map(|s| s.size).sum(),
        lines: 0,
        height: 0.0,
    };
    m.lines = cfg.shown().filter(|i| fact(&m, *i).is_some()).count();
    m.height = height(cfg, m.lines, controls);
    m
}

/// The height of a header: the cover (beside or above the facts) or the
/// facts, whichever is taller.
pub fn height(cfg: &AlbumGroupConfig, lines: usize, controls: bool) -> f32 {
    let text = TITLE_H
        + lines as f32 * FACT_H
        + if controls { CONTROLS_H } else { 0.0 }
        + metrics::space_xs() * (lines + usize::from(controls)) as f32;
    let cover = cfg.image_size as f32;
    let h = if cover <= 0.0 {
        text
    } else if cfg.vertical_layout {
        cover + metrics::space_sm() + text
    } else {
        cover.max(text)
    };
    h + 2.0 * PAD_Y
}

/// The text of one fact; `None` when the songs have none. Album artists are
/// drawn as links, so they are not text here (`Some("")`).
fn fact(m: &GroupMeta, item: AlbumGroupItem) -> Option<String> {
    let s = &m.song;
    match item {
        AlbumGroupItem::AlbumArtists => (!s.album_artists.is_empty()).then(String::new),
        AlbumGroupItem::ReleaseDate => s.extra.release_date.as_deref().map(partial_date),
        AlbumGroupItem::ReleaseYear => s
            .extra
            .release_date
            .as_deref()
            .and_then(super::dates::year_of)
            .or(s.year)
            .filter(|y| *y > 0)
            .map(|y| y.to_string()),
        AlbumGroupItem::SongCount => {
            Some(if m.count == 1 { "1 track".into() } else { format!("{} tracks", m.count) })
        }
        AlbumGroupItem::Duration => (m.duration.as_secs() > 0).then(|| duration_words(m.duration)),
        // Songs do not say which kind of release they are on.
        AlbumGroupItem::ReleaseType => None,
        AlbumGroupItem::Genres => (!m.genres.is_empty()).then(|| m.genres.join(", ")),
        AlbumGroupItem::Size => (m.size > 0).then(|| size_string(m.size)),
    }
}

/// The group rows for the songs `from..to`: one header per run, `before`
/// counted from `from`. Empty when no run has an album.
pub fn groups(state: &Listen, songs: &[Song], from: usize, to: usize) -> Vec<RowGroup> {
    let cfg = &state.config.settings.lists.album_group;
    runs(songs, from, to)
        .into_iter()
        .map(|(first, count)| {
            let m = meta(songs, first, count, cfg, controls_shown(state, &songs[first]));
            RowGroup {
                before: first - from,
                title: m.song.album.clone().unwrap_or_default(),
                checked: false,
                on_toggle: Message::Noop,
                look: GroupLook::Album(Rc::new(m)),
            }
        })
        .collect()
}

/// Favourite and rating controls are offered for albums of this source.
fn controls_shown(state: &Listen, song: &Song) -> bool {
    let cfg = &state.config.settings.lists.album_group;
    cfg.show_favorite_rating && song.album_id.is_some() && (hearts(state, song) || stars(state, song))
}

fn hearts(state: &Listen, song: &Song) -> bool {
    state.show_favorites() && (state.demo || state.can_favorite_items(&song.server_id))
}

fn stars(state: &Listen, song: &Song) -> bool {
    state.show_ratings() && (state.demo || state.can_rate(&song.server_id, song.source))
}

/// The header row; `song` is the run's first song (it outlives `m`).
pub fn view<'a>(state: &'a Listen, id: &ListId, m: &GroupMeta, song: &'a Song) -> Element<'a, Message> {
    let t = state.tokens;
    let cfg = &state.config.settings.lists.album_group;
    let width = state.page_width().max(300.0) - 2.0 * metrics::space_md() - cfg.image_size as f32 - 24.0;
    let cx = Cx {
        state,
        id: id.clone(),
        index: m.first,
        size: listen_core::settings::RowSize::Compact,
        hovered: false,
        selected: false,
        width: width.max(120.0),
        card: false,
    };

    let name = song.album.clone().unwrap_or_default();
    let title: Element<'a, Message> = link(
        t,
        ellipsize_px(&name, cx.width, metrics::font_lg()),
        metrics::font_lg(),
        t.foreground,
        song.album_id.clone().map(Message::OpenAlbum),
    );
    let mut info = Column::new().spacing(metrics::space_xs()).push(container(title).height(TITLE_H));
    for item in cfg.shown() {
        let Some(text) = fact(m, item) else { continue };
        let line: Element<'a, Message> = if item == AlbumGroupItem::AlbumArtists {
            artist_links(&cx, &song.album_artists, true)
        } else {
            label(ellipsize_px(&text, cx.width, metrics::font_sm()), metrics::font_sm(), t.muted).into()
        };
        info = info.push(container(line).height(FACT_H));
    }
    if controls_shown(state, song) {
        info = info.push(controls(state, id, m, song));
    }

    let cover: Option<Element<'a, Message>> =
        (cfg.image_size > 0).then(|| song_cover(state, song, cfg.image_size as f32, metrics::radius_md()));
    let body: Element<'a, Message> = match (cover, cfg.vertical_layout) {
        (Some(c), true) => Column::new().spacing(metrics::space_sm()).push(c).push(info).into(),
        (Some(c), false) => Row::new().spacing(metrics::space_md()).push(c).push(info).into(),
        (None, _) => info.into(),
    };
    container(body)
        .width(Length::Fill)
        .height(m.height)
        .padding(Padding::from([PAD_Y, metrics::space_md()]))
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(style::alpha(t.foreground, 0.05))),
            ..Default::default()
        })
        .into()
}

/// Play, heart and stars of the album (Feishin `AlbumGroupControls`).
fn controls<'a>(state: &'a Listen, id: &ListId, m: &GroupMeta, song: &Song) -> Element<'a, Message> {
    let t = state.tokens;
    let album = song.album_id.as_deref().and_then(|a| state.lists.album_cache(&song.server_id, a));
    let mut out = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    out = out.push(tip(
        state,
        button(icon(Icon::LuPlay, metrics::font_md(), t.foreground))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(Message::List(ListMsg::GroupPlay(id.clone(), m.first, m.count, state.play_behavior()))),
        "Play album",
        None,
        Position::Bottom,
    ));
    if let (Some(album_id), Some(album)) = (song.album_id.clone(), album) {
        if hearts(state, song) {
            let (glyph, color) =
                if album.favorite { (Icon::RiHeartFill, t.primary) } else { (Icon::RiHeartLine, t.muted) };
            out = out.push(tip(
                state,
                button(icon(glyph, metrics::font_md(), color))
                    .padding(4)
                    .style(style::icon_button(t, false))
                    .on_press(Message::List(ListMsg::GroupFavorite(
                        song.server_id.clone(),
                        album_id.clone(),
                    ))),
                if album.favorite { "Remove from favorites" } else { "Add to favorites" },
                None,
                Position::Bottom,
            ));
        }
        if stars(state, song) {
            let rating = album.rating.unwrap_or(0);
            let mut s = Row::new().align_y(Alignment::Center);
            for n in 1..=5u8 {
                let on = n <= rating;
                s = s.push(tip_raw(
                    state,
                    button(icon(
                        if on { Icon::LuStarFill } else { Icon::LuStar },
                        metrics::font_sm(),
                        if on { t.primary } else { t.muted },
                    ))
                    .padding(2)
                    .style(style::icon_button(t, false))
                    .on_press(Message::List(ListMsg::GroupRate(
                        song.server_id.clone(),
                        album_id.clone(),
                        n,
                    ))),
                    &crate::ui::tip::rate_text(n),
                    None,
                    Position::Bottom,
                ));
            }
            out = out.push(s);
        }
    }
    container(out).height(CONTROLS_H).center_y(CONTROLS_H).into()
}
