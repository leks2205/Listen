//! Recently played, Statistics (with the year review) and Mixes, all computed
//! from the local listening history.

use super::playlist_detail::song_list;
use super::{data_label_w, label, label_w};
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

/// What an empty history page says: where songs will appear, or that the history is switched off.
fn empty_hint(state: &Listen) -> &'static str {
    if state.config.settings.history.enabled {
        "Songs you listen to will show up here. The history stays on this computer."
    } else {
        "Listening history is switched off in Settings."
    }
}

pub fn recent(state: &Listen) -> Element<'_, Message> {
    if state.history_view.recent.is_empty() {
        return empty(state, "Recently Played", empty_hint(state));
    }
    song_list(state, "History", "Recently Played", String::new(), None, &state.history_view.recent, |i| {
        Message::History(HistoryMsg::PlayRecent(i))
    })
}

pub fn mix(state: &Listen, kind: MixKind) -> Element<'_, Message> {
    if state.history_view.mix.is_empty() {
        return empty(state, kind.title(), "Not enough listening history for this mix yet.");
    }
    song_list(
        state,
        "Mix",
        kind.title(),
        format!("{} • ", crate::i18n::tr(kind.blurb())),
        None,
        &state.history_view.mix,
        |i| Message::History(HistoryMsg::PlayMix(i)),
    )
}

const LEARNABLE_BLURB: &str = "Liked songs where you know 90–98 % of the Korean words (Lute)";

/// The Learnable songs mix: liked songs at 90–98 % known words, easiest first.
pub fn learnable(state: &Listen) -> Element<'_, Message> {
    let l = &state.learn;
    let status = if let Some(e) = &l.error {
        crate::i18n::trf("Could not build the mix: {}", &[e])
    } else if l.loading && l.to_check > 0 {
        crate::i18n::trf(
            "Looking at the lyrics of your liked songs: {} of {}",
            &[&l.checked.min(l.to_check), &l.to_check],
        )
    } else if l.loading {
        crate::i18n::tr("Loading your liked songs and Lute words…").to_string()
    } else {
        String::new()
    };
    if l.songs.is_empty() {
        let t = state.tokens;
        let text = if status.is_empty() {
            crate::i18n::tr(
                "No liked song has 90–98 % known words yet. Songs count once their lyrics were found.",
            )
            .to_string()
        } else {
            status
        };
        return container(
            column![
                label_w("Learnable songs", 36.0, t.foreground, Weight::Bold),
                label(LEARNABLE_BLURB, metrics::font_md(), t.muted)
                    .wrapping(iced::widget::text::Wrapping::Word),
                label(text, metrics::font_md(), t.muted).wrapping(iced::widget::text::Wrapping::Word),
            ]
            .spacing(metrics::space_md()),
        )
        .padding(Padding { top: 80.0, right: 32.0, bottom: 32.0, left: 32.0 })
        .into();
    }
    let mut sub = crate::i18n::trf(
        "{} % to {} % known words • ",
        &[
            &l.cov.last().and_then(|c| c.percent()).unwrap_or(0),
            &l.cov.first().and_then(|c| c.percent()).unwrap_or(0),
        ],
    );
    if !status.is_empty() {
        sub = format!("{status} • {sub}");
    }
    song_list(state, "Mix", "Learnable songs", sub, None, &l.songs, |i| {
        Message::Learn(crate::app::learnable::LearnMsg::Play(i))
    })
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
            crate::i18n::trf("{} • {} songs", &[&crate::i18n::tr(kind.blurb()), count])
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
    if state.learn_available() {
        page = page.push(
            button(
                column![
                    label_w("Learnable songs", metrics::font_lg(), t.foreground, Weight::Semibold),
                    label(LEARNABLE_BLURB, metrics::font_sm(), t.muted),
                ]
                .spacing(metrics::space_xs()),
            )
            .width(Length::Fill)
            .padding(metrics::space_md())
            .style(style::default_button(t))
            .on_press(Message::Navigate(Route::Learnable)),
        );
    }
    scrollable(
        container(page).padding(Padding { top: 80.0, right: 32.0, bottom: 80.0, left: 32.0 }).max_width(900),
    )
    .style(style::scrollbar(t))
    .height(Length::Fill)
    .into()
}

/// "3 h 5 min", translated (it goes into the holes of longer lines).
fn duration_text(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    if h > 0 { crate::i18n::trf("{} h {} min", &[&h, &m]) } else { crate::i18n::trf("{} min", &[&m]) }
}

fn tile<'a>(state: &'a Listen, value: String, name: &'static str) -> Element<'a, Message> {
    let t = state.tokens;
    container(
        column![
            label_w(value, 28.0, t.foreground, Weight::Bold)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            label(name, metrics::font_sm(), t.muted)
        ]
        .spacing(metrics::space_xs()),
    )
    .padding(metrics::space_lg())
    .width(Length::FillPortion(1))
    .clip(true)
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
        let detail = match (r.detail.is_empty(), r.skips) {
            (true, 0) => crate::i18n::trf("{} plays • {}", &[&r.plays, &duration_text(r.secs)]),
            (true, n) => {
                crate::i18n::trf("{} plays • {} • {} skipped", &[&r.plays, &duration_text(r.secs), &n])
            }
            (false, 0) => crate::i18n::trf("{} • {} plays", &[&r.detail, &r.plays]),
            (false, n) => crate::i18n::trf("{} • {} plays • {} skipped", &[&r.detail, &r.plays, &n]),
        };
        col = col.push(row![
            container(label(format!("{}", i + 1), metrics::font_md(), t.muted)).width(28),
            column![
                data_label_w(r.name.clone(), metrics::font_md(), t.foreground, Weight::Medium)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                label(detail, metrics::font_xs(), t.muted)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]);
    }
    container(col).width(Length::FillPortion(1)).clip(true).into()
}

pub fn stats(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let v = &state.history_view;
    if state.history.is_empty() {
        return empty(state, "Statistics", empty_hint(state));
    }
    let period_button = |text: String, active: bool, msg: HistoryMsg| {
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
        .on_press(Message::History(msg))
    };
    let year_button = |text: String, year: Option<i32>| {
        period_button(text, !v.weeks && v.year == year, HistoryMsg::Year(year))
    };
    let mut years = Row::new()
        .spacing(metrics::space_sm())
        .push(period_button("Last 4 weeks".into(), v.weeks, HistoryMsg::Weeks))
        .push(year_button("All time".into(), None));
    for y in &v.stats.years {
        years = years.push(year_button(y.to_string(), Some(*y)));
    }
    let years =
        scrollable(years).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default()));
    if v.weeks {
        return page(state, weeks(state, years));
    }
    let heading = match v.year {
        Some(y) => format!("Your {y} in music"),
        None => "Your listening, all time".to_string(),
    };
    let s = &v.stats;
    let body = column![
        label_w("Statistics", 36.0, t.foreground, Weight::Bold),
        label(heading, metrics::font_lg(), t.muted),
        years,
        row![
            tile(state, s.plays.to_string(), "songs played"),
            tile(state, duration_text(s.listening_secs), "listening time"),
            tile(state, s.skips.to_string(), "songs skipped"),
        ]
        .spacing(metrics::space_md()),
        row![
            top_list(state, "Top songs", &s.songs),
            top_list(state, "Top artists", &s.artists),
            top_list(state, "Top albums", &s.albums),
        ]
        .spacing(metrics::space_lg()),
        label(
            "Listening time counts the full length of each song you played past the scrobble point. A song you leave for another one before that point counts as skipped, never as played.",
            metrics::font_xs(),
            t.muted
        )
        .wrapping(iced::widget::text::Wrapping::Word),
    ]
    .spacing(metrics::space_lg());
    page(state, body)
}

fn page<'a>(state: &'a Listen, body: Column<'a, Message>) -> Element<'a, Message> {
    scrollable(
        container(body).padding(Padding { top: 80.0, right: 32.0, bottom: 80.0, left: 32.0 }).max_width(1400),
    )
    .style(style::scrollbar(state.tokens))
    .height(Length::Fill)
    .into()
}

/// "+12 % vs last week", or "first week" when last week had nothing.
fn versus(before: u64, now: u64) -> String {
    match listen_core::weekly::change_pct(before, now) {
        Some(0) => crate::i18n::tr("same as last week").to_string(),
        Some(p) => crate::i18n::trf("{} % vs last week", &[&format!("{p:+}")]),
        None if now > 0 => crate::i18n::tr("nothing last week").to_string(),
        None => String::new(),
    }
}

fn week_tile<'a>(
    state: &'a Listen,
    value: String,
    name: &'static str,
    before: u64,
    now: u64,
) -> Element<'a, Message> {
    let t = state.tokens;
    container(
        column![
            label_w(value, 28.0, t.foreground, Weight::Bold)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            label(name, metrics::font_sm(), t.muted).wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            label(versus(before, now), metrics::font_xs(), if now >= before { t.primary } else { t.muted })
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        ]
        .spacing(metrics::space_xs()),
    )
    .padding(metrics::space_lg())
    .width(Length::FillPortion(1))
    .clip(true)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(t.surface)),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        ..Default::default()
    })
    .into()
}

/// The rolling view: this week against last week, the 4 weeks' top lists, new artists, skips.
fn weeks<'a>(state: &'a Listen, periods: impl Into<Element<'a, Message>>) -> Column<'a, Message> {
    let t = state.tokens;
    let w = &state.history_view.weekly;
    let (a, b) = (&w.this_week, &w.last_week);
    let skipped: Vec<Ranked> = w
        .most_skipped
        .iter()
        .map(|r| Ranked {
            detail: crate::i18n::trf("{} • {} skipped", &[&r.detail, &r.skips]),
            secs: 0,
            skips: 0,
            ..r.clone()
        })
        .collect();
    column![
        label_w("Statistics", 36.0, t.foreground, Weight::Bold),
        label("This week: the last 7 days", metrics::font_lg(), t.muted),
        periods.into(),
        row![
            week_tile(state, a.plays.to_string(), "songs played", b.plays.into(), a.plays.into()),
            week_tile(state, duration_text(a.listening_secs), "listening time", b.listening_secs, a.listening_secs),
            week_tile(state, a.songs.to_string(), "different songs", b.songs.into(), a.songs.into()),
            week_tile(state, a.artists.to_string(), "different artists", b.artists.into(), a.artists.into()),
        ]
        .spacing(metrics::space_md()),
        row![
            column![
                top_list(state, "New artists this week", &w.new_artists),
                label(
                    crate::i18n::trf("{} new artists in all", &[&w.new_artists_total]),
                    metrics::font_xs(),
                    t.muted
                ),
            ]
            .spacing(metrics::space_xs())
            .width(Length::FillPortion(1)),
            owned_list(state, "Skipped most (4 weeks)", skipped),
        ]
        .spacing(metrics::space_lg()),
        label_w("The last 4 weeks", metrics::font_lg(), t.foreground, Weight::Bold),
        label(
            crate::i18n::trf(
                "{} songs played • {} • {} skipped",
                &[&w.four_weeks.plays, &duration_text(w.four_weeks.listening_secs), &w.four_weeks.skips]
            ),
            metrics::font_sm(),
            t.muted
        ),
        row![
            top_list(state, "Top songs", &w.four_weeks.songs),
            top_list(state, "Top artists", &w.four_weeks.artists),
            top_list(state, "Top albums", &w.four_weeks.albums),
        ]
        .spacing(metrics::space_lg()),
        label(
            "Rolling windows: this week is the last 7 days, last week the 7 days before, 4 weeks the last 28 days. A new artist has a play this week and none before it.",
            metrics::font_xs(),
            t.muted
        )
        .wrapping(iced::widget::text::Wrapping::Word),
    ]
    .spacing(metrics::space_lg())
}

/// [`top_list`] over rows built for the view.
fn owned_list<'a>(state: &'a Listen, title: &'static str, items: Vec<Ranked>) -> Element<'a, Message> {
    let t = state.tokens;
    let mut col = Column::new().spacing(metrics::space_sm()).push(label_w(
        title,
        metrics::font_lg(),
        t.foreground,
        Weight::Bold,
    ));
    if items.is_empty() {
        col = col.push(label("Nothing skipped", metrics::font_sm(), t.muted));
    }
    for (i, r) in items.into_iter().enumerate() {
        col = col.push(row![
            container(label(format!("{}", i + 1), metrics::font_md(), t.muted)).width(28),
            column![
                data_label_w(r.name, metrics::font_md(), t.foreground, Weight::Medium)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                label(crate::i18n::trf("{} • {} plays", &[&r.detail, &r.plays]), metrics::font_xs(), t.muted)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]);
    }
    container(col).width(Length::FillPortion(1)).clip(true).into()
}
