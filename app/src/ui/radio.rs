//! The Radio page (Feishin `features/radio`): the server's internet radio
//! stations with search / sort, play / stop, edit and delete, the create /
//! edit dialog, and the now-playing block the player bar shows while a
//! station plays.

use super::overlay::ModalParts;
use super::{icon, label, label_w, plural};
use crate::app::overlay::OverlayMsg;
use crate::app::radio::{RadioMsg as R, RadioSort};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, pick_list, row, scrollable, space, text_input};
use iced::{Alignment, Color, Element, Length, Padding};

fn msg(m: R) -> Message {
    Message::Radio(m)
}

fn tool<'a>(state: &Listen, glyph: Icon, on_press: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    crate::ui::tip::tip_icon(
        state,
        button(icon(glyph, metrics::font_lg(), if on_press.is_some() { t.foreground } else { t.muted }))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press_maybe(on_press),
        glyph,
        crate::ui::tip::Position::Bottom,
    )
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let radio = &state.radio;
    let shown = radio.visible();
    let count = if shown.len() == radio.stations.len() {
        plural(radio.stations.len(), "station")
    } else {
        format!("{} of {}", shown.len(), plural(radio.stations.len(), "station"))
    };
    let top = row![
        label_w("Radio", 36.0, t.foreground, Weight::Bold),
        container(label(count, metrics::font_md(), t.muted)).padding(Padding::from([0.0, 4.0])),
        space::horizontal(),
        button(label_w("Create station", metrics::font_md(), Color::WHITE, Weight::Semibold))
            .padding(Padding::from([6.0, 14.0]))
            .style(style::primary_button(t))
            .on_press(msg(R::OpenCreate)),
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::Center);

    let search = text_input(crate::i18n::tr("Search"), &radio.search)
        .font(crate::ui::font_for(&radio.search, crate::appearance::current_font()))
        .font(super::font_for(&radio.search, crate::appearance::current_font()))
        .size(metrics::font_md())
        .padding(Padding::from([6.0, 10.0]))
        .style(style::input(t))
        .on_input(|s| msg(R::Search(s)))
        .width(Length::Fixed(220.0));
    let mut find = Row::new().align_y(Alignment::Center).spacing(2.0).push(search);
    if !radio.search.is_empty() {
        find = find.push(tool(state, Icon::LuX, Some(msg(R::Search(String::new())))));
    }
    let order = if radio.desc { Icon::LuArrowDown } else { Icon::LuArrowUp };
    let controls = Row::new()
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center)
        .push(pick_list(RadioSort::ALL, Some(radio.sort()), |s| msg(R::Sort(s))).width(110))
        .push(tool(state, order, Some(msg(R::Order))))
        .push(tool(state, Icon::LuRefreshCw, Some(msg(R::Refresh))));
    let toolbar =
        row![find, space::horizontal(), controls].spacing(metrics::space_md()).align_y(Alignment::Center);

    let head = container(column![top, toolbar].spacing(metrics::space_md())).padding(Padding {
        top: 24.0,
        right: metrics::PAGE_PADDING_X,
        bottom: 12.0,
        left: metrics::PAGE_PADDING_X,
    });

    let body: Element<'_, Message> = if let Some(e) = &radio.error {
        note(state, format!("Could not load the stations: {e}"), t.error)
    } else if radio.loading && radio.stations.is_empty() {
        note(state, "Loading stations…".into(), t.muted)
    } else if radio.stations.is_empty() {
        note(
            state,
            "This server has no internet radio stations yet. Create one to get started.".into(),
            t.muted,
        )
    } else if shown.is_empty() {
        note(state, "No station matches the search.".into(), t.muted)
    } else {
        let mut rows = Column::new().spacing(metrics::space_sm());
        for station in shown {
            rows = rows.push(station_card(state, station));
        }
        scrollable(container(rows).padding(Padding::from([0.0, metrics::PAGE_PADDING_X])))
            .style(style::scrollbar(t))
            .height(Length::Fill)
            .into()
    };
    column![head, super::divider_h(t), body].into()
}

fn note(state: &Listen, text: String, color: Color) -> Element<'_, Message> {
    let _ = state;
    container(label(text, metrics::font_md(), color))
        .padding(Padding::from([metrics::space_lg(), metrics::PAGE_PADDING_X]))
        .into()
}

/// The station's cover when the server has one and it is cached.
pub fn station_image<'a>(
    state: &Listen,
    station: &listen_source::InternetRadio,
) -> Option<Element<'a, Message>> {
    let id = station.image_id.clone()?;
    let server = state.config.active_server.as_ref()?;
    let image = listen_core::model::ImageRef::Id(id);
    state.images.get(&state.image_key_on(server, &image))?;
    Some(super::cover_on(state, server, Some(&image), 56.0, metrics::radius_md(), Icon::LuRadio))
}

fn station_card<'a>(state: &'a Listen, station: &'a listen_source::InternetRadio) -> Element<'a, Message> {
    let t = state.tokens;
    let active = state.radio.active.as_ref().filter(|a| a.station.id == station.id);
    let playing = active.is_some() && state.playback.playing;
    let mut meta = Column::new().spacing(2).push(label_w(
        station.name.as_str(),
        metrics::font_lg(),
        if active.is_some() { t.primary } else { t.foreground },
        Weight::Medium,
    ));
    if let Some(title) = active.and_then(|a| a.title.as_deref()) {
        meta = meta.push(label(format!("Now playing: {title}"), metrics::font_sm(), t.primary));
    }
    meta = meta.push(label(station.stream_url.as_str(), metrics::font_sm(), t.muted));
    if let Some(home) = &station.home_page_url {
        meta = meta.push(label(home.as_str(), metrics::font_sm(), t.muted));
    }
    // The server's picture of the station, else the radio glyph (pause while it plays).
    let picture = station_image(state, station);
    let badge: Element<'a, Message> = match picture {
        Some(img) => img,
        None => container(icon(
            if playing { Icon::LuPause } else { Icon::LuRadio },
            metrics::font_xl(),
            if active.is_some() { t.primary } else { t.muted },
        ))
        .center(Length::Fixed(56.0))
        .into(),
    };
    let main = button(
        row![badge, container(meta).width(Length::Fill).clip(true)]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding::from([metrics::space_sm(), metrics::space_md()]))
    .style(style::row_button(t, active.is_some()))
    .on_press(msg(R::Play(station.id.clone())));
    row![
        main,
        tool(state, Icon::LuPencil, Some(msg(R::OpenEdit(station.id.clone())))),
        tool(state, Icon::LuTrash2, Some(msg(R::AskDelete(station.id.clone())))),
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center)
    .into()
}

/// The create / edit dialog.
pub fn form_modal(state: &Listen) -> Option<ModalParts<'_>> {
    let t = state.tokens;
    let form = state.radio.form.as_ref()?;
    let field = |title: &'static str, hint: &'static str, value: &str, make: fn(String) -> R| {
        column![
            label_w(title, metrics::font_sm(), t.muted, Weight::Semibold),
            text_input(crate::i18n::tr(hint), value)
                .font(super::font_for(value, crate::appearance::current_font()))
                .size(metrics::font_md())
                .padding(Padding::from([8.0, 10.0]))
                .style(style::input(t))
                .on_input_maybe((!form.busy).then_some(move |s| msg(make(s))))
                .on_submit_maybe(form.submittable().then(|| msg(R::FormSubmit))),
        ]
        .spacing(metrics::space_xs())
    };
    let body = column![
        field("Name", "Station name", &form.name, R::FormName),
        field("Stream URL", "https://…", &form.stream_url, R::FormStream),
        field("Home page URL", "Optional", &form.home_page_url, R::FormHome),
    ]
    .spacing(metrics::space_md());
    let cancel = button(label("Cancel", metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Overlay(OverlayMsg::CloseModal));
    let submit = button(label(
        if form.busy {
            "Saving…"
        } else if form.is_create() {
            "Create"
        } else {
            "Save"
        },
        metrics::font_md(),
        Color::WHITE,
    ))
    .padding(Padding::from([6.0, 16.0]))
    .style(style::primary_button(t))
    .on_press_maybe(form.submittable().then(|| msg(R::FormSubmit)));
    let title = if form.is_create() { "Create radio station" } else { "Edit radio station" };
    Some((title, body.into(), vec![cancel.into(), submit.into()]))
}

/// The player bar's left block while a station plays: its picture (the radio
/// glyph), the name and the ICY title.
pub fn now_playing(state: &Listen, width: f32) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let active = state.radio.active.as_ref()?;
    let px = (width - metrics::space_lg() - metrics::PLAYER_ART - 24.0).max(0.0);
    let fit = |s: &str, size: f32| super::ellipsize_px(s, px, size);
    let (artist, song) = match active.title.as_deref().map(listen_player::split_icy_title) {
        Some((artist, song)) => (artist.map(String::from), song.to_string()),
        None => (None, "Live radio".to_string()),
    };
    let art: Element<'_, Message> = match station_image(state, &active.station) {
        Some(img) => container(img).center(Length::Fixed(metrics::PLAYER_ART)).clip(true).into(),
        None => container(icon(Icon::LuRadio, metrics::font_xl(), t.primary))
            .center(Length::Fixed(metrics::PLAYER_ART))
            .style(move |_| container::Style {
                background: Some(t.surface.into()),
                border: iced::Border { radius: metrics::radius_md().into(), ..Default::default() },
                ..Default::default()
            })
            .into(),
    };
    let mut meta =
        column![label_w(fit(&song, metrics::font_md()), metrics::font_md(), t.foreground, Weight::Medium)]
            .spacing(metrics::space_xs());
    if let Some(artist) = artist {
        meta = meta.push(label(fit(&artist, metrics::font_md()), metrics::font_md(), t.muted));
    }
    meta = meta.push(label(fit(&active.station.name, metrics::font_md()), metrics::font_md(), t.muted));
    let info =
        row![container(art).padding(Padding { top: 12.0, right: 12.0, bottom: 12.0, left: 0.0 }), meta]
            .align_y(Alignment::Center);
    Some(
        container(info)
            .padding(Padding { left: metrics::space_lg(), ..Padding::ZERO })
            .width(width)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .clip(true)
            .into(),
    )
}
