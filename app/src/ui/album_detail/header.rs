//! The top of the album page: header block, toolbar and comment.

use super::format;
use super::{HEADER_H, TOOLBAR_H};
use crate::app::album_page::{AlbumPageMsg as P, PagePlay, TrackSort};
use crate::app::list::ListId;
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::list::anchor::anchored;
use crate::ui::{album_cover, icon, label, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, pick_list, row, space, stack, text_input};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::{Album, SourceKind};
use listen_source::Capabilities;

const COVER: f32 = 250.0;

/// Width the page text gets (page minus its padding).
fn text_width(state: &Listen) -> f32 {
    (state.page_width() - 2.0 * metrics::PAGE_PADDING_X).max(200.0)
}

/// Exact height of [`lead`] (the list needs it to virtualize the rows).
pub fn lead_height(state: &Listen, album: &Album) -> f32 {
    HEADER_H + TOOLBAR_H + comment_block_height(state, album)
}

fn comment_block_height(state: &Listen, album: &Album) -> f32 {
    let Some(text) = format::comment(album) else { return 0.0 };
    let w = text_width(state);
    let long = format::comment_is_long(&text, w);
    let h = format::comment_height(&text, state.album_page.comment_open, w);
    // Text, the show more / less line and the gap below.
    h + if long { 28.0 } else { 0.0 } + 12.0
}

pub fn lead<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let mut out = Column::new();
    out = out.push(header(state, album));
    out = out.push(toolbar(state, album));
    if let Some(c) = comment(state, album) {
        out = out.push(c);
    }
    out.into()
}

fn pill_button<'a>(state: &Listen, content: Element<'a, Message>, msg: Message) -> Element<'a, Message> {
    button(content)
        .padding(Padding::from([8.0, 14.0]))
        .style(style::default_button(state.tokens))
        .on_press(msg)
        .into()
}

fn play_row<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let t = state.tokens;
    let caps = state.album_caps(album);
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
    .on_press(Message::AlbumPage(P::Play(PagePlay::of_behavior(behavior))));
    let play = crate::ui::long_press::long_press(
        play,
        Message::AlbumPage(P::Play(PagePlay::long_press_of(behavior))),
    );
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
            Message::AlbumPage(P::Play(kind)),
        )
    };
    let open = state.album_page.play_menu;
    let chevron = button(icon(
        if open { Icon::LuChevronUp } else { Icon::LuChevronDown },
        metrics::font_lg(),
        t.foreground,
    ))
    .padding(8)
    .style(style::icon_button(t, open))
    .on_press(Message::AlbumPage(P::TogglePlayMenu));
    let chevron =
        crate::ui::tip::tip(state, chevron, "More play options", None, crate::ui::tip::Position::Bottom);

    let mut left = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    left = left
        .push(play)
        .push(text_button(Icon::LuSkipForward, "Add next", PagePlay::Next))
        .push(text_button(Icon::RiPlayListAddLine, "Add last", PagePlay::Last))
        .push(chevron);
    if caps.intersects(Capabilities::SIMILAR_SONGS | Capabilities::RADIO) {
        let busy = state.album_page.radio_busy;
        left = left.push(crate::ui::tip::tip(
            state,
            button(icon(Icon::RiRadioLine, metrics::font_xl(), if busy { t.muted } else { t.foreground }))
                .padding(8)
                .style(style::icon_button(t, false))
                .on_press_maybe((!busy).then_some(Message::AlbumPage(P::Radio))),
            "Start radio",
            None,
            crate::ui::tip::Position::Bottom,
        ));
    }

    let mut right = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    if caps.contains(Capabilities::RATINGS) && album.source != SourceKind::YouTubeMusic {
        let rating = album.rating.unwrap_or(0);
        let mut stars = Row::new().align_y(Alignment::Center);
        for n in 1..=5u8 {
            let on = n <= rating;
            stars = stars.push(crate::ui::tip::tip_raw(
                state,
                button(icon(
                    if on { Icon::LuStarFill } else { Icon::LuStar },
                    metrics::font_lg(),
                    if on { t.primary } else { t.muted },
                ))
                .padding(3)
                .style(style::icon_button(t, false))
                .on_press(Message::AlbumPage(P::Rate(n))),
                &crate::ui::tip::rate_text(n),
                None,
                crate::ui::tip::Position::Bottom,
            ));
        }
        right = right.push(stars);
    }
    if caps.contains(Capabilities::FAVORITE_ITEMS) {
        right = right.push(crate::ui::tip::tip(
            state,
            button(icon(
                if album.favorite { Icon::RiHeartFill } else { Icon::RiHeartLine },
                metrics::font_xl(),
                if album.favorite { t.primary } else { t.foreground },
            ))
            .padding(8)
            .style(style::icon_button(t, false))
            .on_press(Message::AlbumPage(P::Favorite)),
            if album.favorite { "Remove from favorites" } else { "Add to favorites" },
            None,
            crate::ui::tip::Position::Bottom,
        ));
    }
    right = right.push(anchored(
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
        |b| Message::AlbumPage(P::More(iced::Point::new(b.x, b.y + b.height + 4.0))),
    ));

    let mut col = Column::new().spacing(metrics::space_sm());
    col = col
        .push(row![left, space::horizontal(), right].align_y(Alignment::Center).spacing(metrics::space_md()));
    if open {
        let extra = |glyph: Icon, name: &'static str, kind: PagePlay| text_button(glyph, name, kind);
        col = col.push(
            row![
                extra(Icon::LuShuffle, "Shuffle", PagePlay::Shuffle),
                extra(Icon::LuShuffle, "Add next shuffled", PagePlay::NextShuffled),
                extra(Icon::LuShuffle, "Add last shuffled", PagePlay::LastShuffled),
            ]
            .spacing(metrics::space_sm()),
        );
    }
    col.into()
}

/// The album artists, each a link to its page.
fn artist_links<'a>(state: &Listen, album: &'a Album) -> Element<'a, Message> {
    let t = state.tokens;
    let mut r = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    for (i, artist) in album.album_artists.iter().enumerate() {
        if i > 0 {
            r = r.push(label("•", metrics::font_lg(), t.muted));
        }
        let to = artist.id.clone().filter(|id| !id.is_empty()).map(|id| Message::Navigate(Route::Artist(id)));
        r = r.push(crate::ui::list::columns::link(
            t,
            artist.name.clone(),
            metrics::font_lg(),
            t.foreground,
            to,
        ));
    }
    r.wrap().into()
}

fn header<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let t = state.tokens;
    // "ALBUM" (the release type, linking to the album list) and the version.
    let kind =
        album.release_type.as_deref().map(format::release_type_label).unwrap_or_else(|| "Album".into());
    let mut type_row = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    type_row = type_row.push(crate::ui::list::columns::link(
        t,
        kind.to_uppercase(),
        metrics::font_md(),
        t.foreground,
        Some(Message::Navigate(Route::Albums)),
    ));
    if let Some(version) = &album.info.version {
        type_row = type_row.push(label("•", metrics::font_md(), t.muted)).push(label(
            version.clone(),
            metrics::font_md(),
            t.foreground,
        ));
    }

    let mut meta = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    meta = meta.push(label("♫", metrics::font_md(), t.foreground));
    for (i, item) in format::meta_items(album).into_iter().enumerate() {
        if i > 0 {
            meta = meta.push(label("•", metrics::font_md(), t.muted));
        }
        meta = meta.push(label(item, metrics::font_md(), t.foreground));
    }

    let info = column![
        type_row,
        crate::ui::header_title(&album.name, t.foreground),
        meta,
        artist_links(state, album),
        play_row(state, album)
    ]
    .spacing(metrics::space_sm())
    .width(Length::Fill);
    let body = row![
        album_cover(state, album, COVER, metrics::radius_md()),
        container(info).align_bottom(COVER).width(Length::Fill)
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::End);
    stack![
        crate::ui::header_background(
            state,
            album.image.as_ref(),
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

/// Search, sort, order and the column config.
fn toolbar<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let t = state.tokens;
    let page = &state.album_page;
    let search = text_input(crate::i18n::tr("Search"), &page.search)
        .font(crate::ui::font_for(&page.search, crate::appearance::current_font()))
        .on_input(|s| Message::AlbumPage(P::Search(s)))
        .padding(Padding::from([8.0, 14.0]))
        .style(style::input(t))
        .width(Length::Fill);
    let clear = (!page.search.is_empty()).then(|| {
        crate::ui::tip::tip(
            state,
            button(icon(Icon::LuX, metrics::font_md(), t.muted))
                .padding(6)
                .style(style::icon_button(t, false))
                .on_press(Message::AlbumPage(P::Search(String::new()))),
            "Clear search",
            None,
            crate::ui::tip::Position::Bottom,
        )
    });
    let sort = pick_list(TrackSort::ALL, Some(page.sort), |s| Message::AlbumPage(P::Sort(s))).width(160);
    let order = button(icon(
        if page.desc { Icon::LuArrowDown } else { Icon::LuArrowUp },
        metrics::font_lg(),
        t.foreground,
    ))
    .padding(8)
    .style(style::icon_button(t, false))
    .on_press(Message::AlbumPage(P::ToggleOrder));
    let order = crate::ui::tip::tip(
        state,
        order,
        if page.desc { "Sort order: descending" } else { "Sort order: ascending" },
        None,
        crate::ui::tip::Position::Bottom,
    );
    let id = ListId::Album(album.id.clone());
    let config = state.config.settings.lists.config(id.kind());
    let mut r = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center).push(search);
    if let Some(clear) = clear {
        r = r.push(clear);
    }
    r = r.push(sort).push(order).push(crate::ui::list::header::toolbar(state, &id, &config));
    container(r)
        .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]))
        .height(TOOLBAR_H)
        .center_y(TOOLBAR_H)
        .into()
}

/// The comment, collapsed to 75 px with a show more / less line.
fn comment<'a>(state: &'a Listen, album: &'a Album) -> Option<Element<'a, Message>> {
    let text = format::comment(album)?;
    let t = state.tokens;
    let w = text_width(state);
    let long = format::comment_is_long(&text, w);
    let open = state.album_page.comment_open;
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
                .on_press(Message::AlbumPage(P::ToggleComment)),
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
