//! Recently played, Statistics (with the year review) and Mixes, all computed
//! from the local listening history.

use super::playlist_detail::song_list;
use super::{label, label_w};
use crate::app::history::HistoryMsg;
use crate::app::{Listen, Message, Route};
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, row, scrollable};
use iced::{Element, Length, Padding};
use listen_core::history::{MixKind, Ranked};

fn empty<'a>(state: &'a Listen, title: &'static str, text: &'static str) -> Element<'a, Message> {
    let t = state.tokens;
    container(
        column![label_w(title, 36.0, t.foreground, Weight::Bold), label(text, metrics::font_md(), t.muted),]
            .spacing(metrics::space_md()),
    )
    .padding(Padding { top: 80.0, right: 32.0, bottom: 32.0, left: 32.0 })
    .into()
}

fn disabled_hint(state: &Listen) -> &'static str {
    if state.config.settings.history.enabled {
        "Songs you listen to will show up here. The history stays on this computer."
    } else {
        "Listening history is switched off in Settings."
    }
}

pub fn recent(state: &Listen) -> Element<'_, Message> {
    if state.history_view.recent.is_empty() {
        return empty(state, "Recently Played", disabled_hint(state));
    }
    song_list(state, "HISTORY", "Recently Played", String::new(), None, &state.history_view.recent, |i| {
        Message::History(HistoryMsg::PlayRecent(i))
    })
}

pub fn mix(state: &Listen, kind: MixKind) -> Element<'_, Message> {
    if state.history_view.mix.is_empty() {
        return empty(state, kind.title(), "Not enough listening history for this mix yet.");
    }
    song_list(
        state,
        "MIX",
        kind.title(),
        format!("{} • ", kind.blurb()),
        None,
        &state.history_view.mix,
        |i| Message::History(HistoryMsg::PlayMix(i)),
    )
}

pub fn mixes(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let mut page = Column::new()
        .spacing(metrics::space_md())
        .push(label_w("Mixes", 36.0, t.foreground, Weight::Bold))
        .push(label("Made from your listening history, on this computer.", metrics::font_md(), t.muted));
    for (kind, count) in &state.history_view.mix_counts {
        let enabled = *count > 0;
        let detail = if enabled {
            format!("{} • {count} songs", kind.blurb())
        } else {
            "Not enough history yet".into()
        };
        page = page.push(
            button(
                column![
                    label_w(kind.title(), metrics::font_lg(), t.foreground, Weight::Semibold),
                    label(detail, metrics::font_sm(), t.muted),
                ]
                .spacing(metrics::space_xs()),
            )
            .width(Length::Fill)
            .padding(metrics::space_md())
            .style(style::default_button(t))
            .on_press_maybe(enabled.then(|| Message::Navigate(Route::Mix(*kind)))),
        );
    }
    scrollable(
        container(page).padding(Padding { top: 80.0, right: 32.0, bottom: 80.0, left: 32.0 }).max_width(900),
    )
    .style(style::scrollbar(t))
    .height(Length::Fill)
    .into()
}

fn duration_text(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    if h > 0 { format!("{h} h {m} min") } else { format!("{m} min") }
}

fn tile<'a>(state: &'a Listen, value: String, name: &'static str) -> Element<'a, Message> {
    let t = state.tokens;
    container(
        column![label_w(value, 28.0, t.foreground, Weight::Bold), label(name, metrics::font_sm(), t.muted)]
            .spacing(metrics::space_xs()),
    )
    .padding(metrics::space_lg())
    .width(Length::FillPortion(1))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(t.surface)),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        ..Default::default()
    })
    .into()
}

fn top_list<'a>(state: &'a Listen, title: &'static str, items: &'a [Ranked]) -> Element<'a, Message> {
    let t = state.tokens;
    let mut col = Column::new().spacing(metrics::space_sm()).push(label_w(
        title,
        metrics::font_lg(),
        t.foreground,
        Weight::Bold,
    ));
    if items.is_empty() {
        col = col.push(label("Nothing yet", metrics::font_sm(), t.muted));
    }
    for (i, r) in items.iter().enumerate() {
        let detail = if r.detail.is_empty() {
            format!("{} plays • {}", r.plays, duration_text(r.secs))
        } else {
            format!("{} • {} plays", r.detail, r.plays)
        };
        col = col.push(row![
            container(label(format!("{}", i + 1), metrics::font_md(), t.muted)).width(28),
            column![
                label_w(r.name.clone(), metrics::font_md(), t.foreground, Weight::Medium),
                label(detail, metrics::font_xs(), t.muted),
            ]
            .spacing(2),
        ]);
    }
    container(col).width(Length::FillPortion(1)).into()
}

pub fn stats(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let v = &state.history_view;
    if state.history.is_empty() {
        return empty(state, "Statistics", disabled_hint(state));
    }
    let year_button = |text: String, year: Option<i32>| {
        let active = v.year == year;
        button(label_w(
            text,
            metrics::font_md(),
            if active { iced::Color::WHITE } else { t.foreground },
            Weight::Semibold,
        ))
        .padding(Padding::from([6.0, 14.0]))
        .style(
            move |th, st| {
                if active { style::primary_button(t)(th, st) } else { style::default_button(t)(th, st) }
            },
        )
        .on_press(Message::History(HistoryMsg::Year(year)))
    };
    let mut years = Row::new().spacing(metrics::space_sm()).push(year_button("All time".into(), None));
    for y in &v.stats.years {
        years = years.push(year_button(y.to_string(), Some(*y)));
    }
    let heading = match v.year {
        Some(y) => format!("Your {y} in music"),
        None => "Your listening, all time".to_string(),
    };
    let s = &v.stats;
    let page = column![
        label_w("Statistics", 36.0, t.foreground, Weight::Bold),
        label(heading, metrics::font_lg(), t.muted),
        scrollable(years).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default())),
        row![
            tile(state, s.plays.to_string(), "songs played"),
            tile(state, duration_text(s.listening_secs), "listening time"),
        ]
        .spacing(metrics::space_md()),
        row![
            top_list(state, "Top songs", &s.songs),
            top_list(state, "Top artists", &s.artists),
            top_list(state, "Top albums", &s.albums),
        ]
        .spacing(metrics::space_lg()),
        label(
            "Listening time counts the full length of each song you played past the scrobble point.",
            metrics::font_xs(),
            t.muted
        ),
    ]
    .spacing(metrics::space_lg());
    scrollable(
        container(page).padding(Padding { top: 80.0, right: 32.0, bottom: 80.0, left: 32.0 }).max_width(1400),
    )
    .style(style::scrollbar(t))
    .height(Length::Fill)
    .into()
}
