//! The top of the playlist page: header block, description and toolbar
//! (Feishin `playlist-detail-song-list-header.tsx`).

use crate::app::album_page::{PagePlay, TrackSort};
use crate::app::playlists::PlaylistPageMsg as P;
use crate::app::{Listen, Message, PlaylistDetail};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::album_detail::format;
use crate::ui::list::anchor::anchored;
use crate::ui::{cover, icon, label, label_w, song_cover};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, pick_list, row, space, stack, text_input};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::Song;
use listen_core::settings::PlaylistTarget;
use listen_source::Capabilities;

pub const COVER: f32 = 250.0;
/// Height of the header block with its background.
pub const HEADER_H: f32 = 362.0;
/// Height of the collapsed header (title bar with Play).
pub const COLLAPSED_H: f32 = 76.0;
/// Height of the search / sort row.
pub const TOOLBAR_H: f32 = 64.0;

fn msg(m: P) -> Message {
    Message::PlaylistPage(m)
}

fn text_width(state: &Listen) -> f32 {
    (state.page_width() - 2.0 * metrics::PAGE_PADDING_X).max(200.0)
}

/// The description shown under the header (none while collapsed).
fn description(detail: &PlaylistDetail) -> Option<String> {
    detail.playlist.description.as_deref().map(str::trim).filter(|d| !d.is_empty()).map(String::from)
}

/// Exact height of [`lead`] (the list needs it to virtualize the rows).
pub fn lead_height(state: &Listen, detail: &PlaylistDetail) -> f32 {
    let page = &state.playlist_page;
    if page.collapsed {
        return COLLAPSED_H + TOOLBAR_H;
    }
    let comment = match description(detail) {
        Some(text) => {
            let w = text_width(state);
            let long = format::comment_is_long(&text, w);
            format::comment_height(&text, page.comment_open, w) + if long { 28.0 } else { 0.0 } + 12.0
        }
        None => 0.0,
    };
    HEADER_H + TOOLBAR_H + comment
}

pub fn lead<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Element<'a, Message> {
    let mut out = Column::new();
    if state.playlist_page.collapsed {
        out = out.push(collapsed(state, detail));
    } else {
        out = out.push(header(state, detail));
    }
    out = out.push(toolbar(state, detail));
    if !state.playlist_page.collapsed
        && let Some(c) = comment(state, detail)
    {
        out = out.push(c);
    }
    out.into()
}

/// The album-style cover: the playlist's own image, else a mosaic of its first songs' covers.
pub fn cover_art<'a>(state: &Listen, detail: &PlaylistDetail, size: f32) -> Element<'a, Message> {
    let radius = metrics::radius_md();
    if detail.playlist.image.is_some() || detail.songs.len() < 4 {
        return cover(state, detail.playlist.image.as_ref(), size, radius, Icon::LuListMusic);
    }
    mosaic(state, &detail.songs, size, radius)
}

/// 2 x 2 covers of distinct albums (else the first songs).
fn mosaic<'a>(state: &Listen, songs: &[Song], size: f32, radius: f32) -> Element<'a, Message> {
    let mut picks: Vec<&Song> = Vec::new();
    for s in songs {
        if picks.len() == 4 {
            break;
        }
        if !picks.iter().any(|p| p.album_id.is_some() && p.album_id == s.album_id) {
            picks.push(s);
        }
    }
    for s in songs {
        if picks.len() == 4 {
            break;
        }
        if !picks.iter().any(|p| std::ptr::eq(*p, s)) {
            picks.push(s);
        }
    }
    let half = size / 2.0;
    let tile = |i: usize| song_cover(state, picks[i], half, 0.0);
    container(column![row![tile(0), tile(1)], row![tile(2), tile(3)]])
        .width(size)
        .height(size)
        .clip(true)
        .style(move |_| container::Style { border: iced::border::rounded(radius), ..Default::default() })
        .into()
}

fn pill_button<'a>(state: &Listen, content: Element<'a, Message>, m: Message) -> Element<'a, Message> {
    button(content)
        .padding(Padding::from([8.0, 14.0]))
        .style(style::default_button(state.tokens))
        .on_press(m)
        .into()
}

fn badge<'a>(state: &Listen, text: impl Into<String>) -> Element<'a, Message> {
    let t = state.tokens;
    container(label_w(text.into(), metrics::font_xs(), t.foreground, Weight::Semibold))
        .padding(Padding::from([2.0, 8.0]))
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(style::alpha(t.foreground, 0.14))),
            border: iced::border::rounded(999),
            ..Default::default()
        })
        .into()
}

fn play_row<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Element<'a, Message> {
    let t = state.tokens;
    let has_songs = !detail.songs.is_empty();
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
    .on_press_maybe(has_songs.then(|| msg(P::Play(PagePlay::of_behavior(behavior)))));
    let play = crate::ui::long_press::long_press(play, msg(P::Play(PagePlay::long_press_of(behavior))));
    let text_button = |glyph: Icon, name: &'static str, kind: PagePlay| {
        pill_button(
            state,
            row![
                icon(glyph, metrics::font_lg(), t.foreground),
                label_w(name, metrics::font_md(), t.foreground, Weight::Medium)
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
            msg(P::Play(kind)),
        )
    };
    let open = state.playlist_page.play_menu;
    let chevron = button(icon(
        if open { Icon::LuChevronUp } else { Icon::LuChevronDown },
        metrics::font_lg(),
        t.foreground,
    ))
    .padding(8)
    .style(style::icon_button(t, open))
    .on_press(msg(P::TogglePlayMenu));
    let chevron =
        crate::ui::tip::tip(state, chevron, "More play options", None, crate::ui::tip::Position::Bottom);

    let mut left = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    left = left.push(play);
    if has_songs {
        left = left
            .push(text_button(Icon::LuSkipForward, "Add next", PagePlay::Next))
            .push(text_button(Icon::RiPlayListAddLine, "Add last", PagePlay::Last))
            .push(chevron);
    }
    let more = anchored(
        crate::ui::tip::tip(
            state,
            button(icon(Icon::LuEllipsis, metrics::font_xl(), t.foreground))
                .padding(8)
                .style(style::icon_button(t, false))
                .on_press(Message::Noop),
            "More options",
            None,
            crate::ui::tip::Position::Bottom,
        ),
        |b| msg(P::More(iced::Point::new(b.x, b.y + b.height + 4.0))),
    );
    let mut col = Column::new().spacing(metrics::space_sm());
    col = col
        .push(row![left, space::horizontal(), more].align_y(Alignment::Center).spacing(metrics::space_md()));
    if open && has_songs {
        col = col.push(
            row![
                text_button(Icon::LuShuffle, "Shuffle", PagePlay::Shuffle),
                text_button(Icon::LuShuffle, "Add next shuffled", PagePlay::NextShuffled),
                text_button(Icon::LuShuffle, "Add last shuffled", PagePlay::LastShuffled),
            ]
            .spacing(metrics::space_sm()),
        );
    }
    col.into()
}

/// Upload and delete buttons over the cover (servers that store covers, own playlists).
fn cover_with_controls<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Element<'a, Message> {
    let t = state.tokens;
    let art = cover_art(state, detail, COVER);
    let caps = state.playlist_caps(&detail.playlist);
    if !(detail.playlist.editable && caps.contains(Capabilities::PLAYLIST_IMAGE)) {
        return art;
    }
    let small = |glyph: Icon, m: Message| {
        button(icon(glyph, metrics::font_md(), Color::WHITE))
            .padding(6)
            .style(move |_, _| button::Style {
                background: Some(iced::Background::Color(Color { a: 0.7, ..Color::BLACK })),
                text_color: Color::WHITE,
                border: iced::border::rounded(999).color(t.border).width(1),
                ..Default::default()
            })
            .on_press(m)
    };
    let controls = container(
        row![small(Icon::LuPlus, msg(P::CoverPick)), small(Icon::LuTrash2, msg(P::CoverDelete))]
            .spacing(metrics::space_xs()),
    )
    .width(COVER)
    .height(COVER)
    .padding(8)
    .align_right(COVER)
    .align_bottom(COVER);
    stack![art, controls].into()
}

fn header<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Element<'a, Message> {
    let t = state.tokens;
    let p = &detail.playlist;
    let mut kind = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    kind = kind.push(label_w("PLAYLIST", metrics::font_md(), t.foreground, Weight::Semibold));
    kind = kind.push(badge(state, if p.public { "Public" } else { "Private" }));
    if state.is_smart(&p.id) {
        kind = kind.push(crate::ui::smart_playlist::badge(t));
    }

    let mut meta = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    meta = meta.push(label("♫", metrics::font_md(), t.foreground));
    let mut items: Vec<String> = Vec::new();
    if let Some(owner) = p.owner.as_deref().filter(|o| !o.is_empty()) {
        items.push(owner.to_string());
    }
    let count = p.song_count.map_or(detail.songs.len(), |n| n as usize).max(detail.songs.len());
    items.push(crate::ui::plural(count, "song"));
    let duration = if p.duration.is_zero() {
        crate::app::playlists::rows::total_duration(&detail.songs)
    } else {
        p.duration
    };
    if !duration.is_zero() {
        items.push(format::duration_words(duration));
    }
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            meta = meta.push(label("•", metrics::font_md(), t.muted));
        }
        meta = meta.push(label(item, metrics::font_md(), t.foreground));
    }

    let info = column![kind, crate::ui::header_title(&p.name, t.foreground), meta, play_row(state, detail)]
        .spacing(metrics::space_sm())
        .width(Length::Fill);
    let body =
        row![cover_with_controls(state, detail), container(info).align_bottom(COVER).width(Length::Fill)]
            .spacing(metrics::space_md())
            .align_y(Alignment::End);
    stack![
        crate::ui::header_background(
            state,
            p.image.as_ref().or_else(|| detail.songs.first().and_then(|s| s.image.as_ref())),
            HEADER_H,
            crate::app::appearance::BlurKind::Album
        ),
        container(body)
            .padding(Padding {
                top: 80.0,
                right: metrics::PAGE_PADDING_X,
                bottom: 32.0,
                left: metrics::PAGE_PADDING_X
            })
            .height(HEADER_H)
            .clip(true),
    ]
    .height(HEADER_H)
    .into()
}

/// The collapsed header: Play, name and badges.
fn collapsed<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Element<'a, Message> {
    let t = state.tokens;
    let p = &detail.playlist;
    let play = button(icon(Icon::LuPlay, metrics::font_lg(), Color::BLACK))
        .padding(12)
        .style(style::wide_play_button(t))
        .on_press_maybe(
            (!detail.songs.is_empty()).then(|| msg(P::Play(PagePlay::of_behavior(state.play_behavior())))),
        );
    let count = p.song_count.map_or(detail.songs.len(), |n| n as usize).max(detail.songs.len());
    let mut bar = Row::new()
        .spacing(metrics::space_md())
        .align_y(Alignment::Center)
        .push(play)
        .push(label_w(crate::ui::ellipsize(&p.name, 60), 28.0, t.foreground, Weight::Bold))
        .push(badge(state, if p.public { "Public" } else { "Private" }));
    if !p.duration.is_zero() {
        bar = bar.push(badge(state, format::duration_words(p.duration)));
    }
    bar = bar.push(badge(state, count.to_string()));
    container(bar)
        .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]))
        .height(COLLAPSED_H)
        .center_y(COLLAPSED_H)
        .into()
}

/// Search, sort, order, filters, the Tracks / Albums switch, refresh, Edit and the collapse arrow.
fn toolbar<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Element<'a, Message> {
    let t = state.tokens;
    let page = &state.playlist_page;
    let editing = page.edit.is_some();
    let target = state.config.settings.lists.playlist_target;
    let on = |m: Message| (!editing).then_some(m);
    let tool = |glyph: Icon, active: bool, m: Option<Message>| {
        let b = button(icon(
            glyph,
            metrics::font_lg(),
            if m.is_none() {
                t.muted
            } else if active {
                t.primary
            } else {
                t.foreground
            },
        ))
        .padding(8)
        .style(style::icon_button(t, active))
        .on_press_maybe(m);
        crate::ui::tip::tip_icon(state, b, glyph, crate::ui::tip::Position::Bottom)
    };

    let search = text_input(crate::i18n::tr("Search"), &page.search)
        .font(crate::ui::font_for(&page.search, crate::appearance::current_font()))
        .padding(Padding::from([8.0, 14.0]))
        .style(style::input(t))
        .on_input_maybe((!editing).then_some(|s| msg(P::Search(s))))
        .width(Length::Fill);
    let mut r = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center).push(search);
    if !page.search.is_empty() && !editing {
        r = r.push(tool(Icon::LuX, false, Some(msg(P::Search(String::new())))));
    }
    // Tracks <-> Albums.
    let (to, name) = match target {
        PlaylistTarget::Tracks => (PlaylistTarget::Albums, "Tracks"),
        PlaylistTarget::Albums => (PlaylistTarget::Tracks, "Albums"),
    };
    r = r.push(
        button(label_w(
            name,
            metrics::font_md(),
            if editing { t.muted } else { t.foreground },
            Weight::Medium,
        ))
        .padding(Padding::from([8.0, 12.0]))
        .style(style::default_button(t))
        .on_press_maybe(on(msg(P::Target(to)))),
    );
    let sort = pick_list(TrackSort::ALL, Some(page.sort), |s| msg(P::Sort(s))).width(150);
    if editing {
        r = r.push(container(label(page.sort.label(), metrics::font_md(), t.muted)).width(150).padding(8));
    } else {
        r = r.push(sort);
    }
    r = r
        .push(tool(
            if page.desc { Icon::LuArrowDown } else { Icon::LuArrowUp },
            false,
            on(msg(P::ToggleOrder)),
        ))
        .push(tool(Icon::LuFilter, page.filters.active(), on(msg(P::OpenFilters))))
        .push(tool(Icon::LuRefreshCw, false, on(msg(P::Refresh))));

    if state.is_smart(&detail.playlist.id) && state.smart_supported() && detail.playlist.editable {
        r = r.push(
            button(label_w("Edit rules", metrics::font_md(), t.foreground, Weight::Medium))
                .padding(Padding::from([8.0, 12.0]))
                .style(style::default_button(t))
                .on_press_maybe(on(Message::Smart(crate::app::smart_playlist::SmartMsg::Edit(
                    detail.playlist.id.clone(),
                )))),
        );
    }
    // Edit / Save and replace / Cancel.
    if state.playlist_can_edit() && target == PlaylistTarget::Tracks {
        match &page.edit {
            Some(edit) => {
                r = r
                    .push(
                        button(label_w(
                            "Save and replace",
                            metrics::font_md(),
                            Color::WHITE,
                            Weight::Semibold,
                        ))
                        .padding(Padding::from([8.0, 12.0]))
                        .style(style::primary_button(t))
                        .on_press_maybe((!edit.saving).then(|| msg(P::EditSave))),
                    )
                    .push(
                        button(label_w("Cancel", metrics::font_md(), t.foreground, Weight::Medium))
                            .padding(Padding::from([8.0, 12.0]))
                            .style(style::default_button(t))
                            .on_press(msg(P::EditCancel)),
                    );
            }
            None => {
                r = r.push(
                    button(label_w("Edit", metrics::font_md(), t.foreground, Weight::Medium))
                        .padding(Padding::from([8.0, 12.0]))
                        .style(style::default_button(t))
                        .on_press(msg(P::EditStart)),
                );
            }
        }
    }
    r = r.push(tool(
        if page.collapsed { Icon::LuChevronDown } else { Icon::LuChevronUp },
        false,
        Some(msg(P::ToggleCollapsed)),
    ));
    let id = state.playlist_list_id(&detail.playlist.id);
    let config = state.config.settings.lists.config(id.kind());
    r = r.push(crate::ui::list::header::toolbar(state, &id, &config));
    container(r)
        .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]))
        .height(TOOLBAR_H)
        .center_y(TOOLBAR_H)
        .into()
}

/// The description, collapsed to 75 px with a show more / less line.
fn comment<'a>(state: &'a Listen, detail: &'a PlaylistDetail) -> Option<Element<'a, Message>> {
    let text = description(detail)?;
    let t = state.tokens;
    let w = text_width(state);
    let long = format::comment_is_long(&text, w);
    let open = state.playlist_page.comment_open;
    let height = format::comment_height(&text, open, w);
    let body = container(
        crate::ui::text_in(text, crate::ui::font(Weight::Normal))
            .size(metrics::font_md())
            .color(t.foreground)
            .wrapping(iced::widget::text::Wrapping::Word)
            .width(Length::Fill),
    )
    .height(if long && !open { Length::Fixed(height) } else { Length::Shrink })
    .clip(true);
    let mut col = Column::new().push(body);
    if long {
        col = col.push(
            button(label(if open { "Show less" } else { "Show more" }, metrics::font_sm(), t.primary))
                .padding(Padding::from([4.0, 0.0]))
                .style(style::bare_button(t))
                .on_press(msg(P::ToggleComment)),
        );
    }
    Some(
        container(col.spacing(4))
            .padding(Padding {
                top: 0.0,
                right: metrics::PAGE_PADDING_X,
                bottom: 12.0,
                left: metrics::PAGE_PADDING_X,
            })
            .into(),
    )
}
