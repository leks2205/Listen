//! Settings page (subset of Feishin's: appearance, playback/EQ, Auto-DJ,
//! lyrics, window & integrations, Feishin import, server).

mod about;
mod advanced;
mod appearance;
mod artist;
mod controls;
mod fullscreen;
mod general;
pub mod hotkeys;
mod integrations;
mod lists;
mod milkdrop;
mod playback;
mod queue;
mod sharing;
mod smart_playlist;
mod window;

use super::{label, label_w};
use crate::app::settings_shell::ShellMsg;
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{
    Column, Row, button, column, container, row, scrollable, slider, space, text_input, toggler,
};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::SettingsTab;

thread_local! {
    /// (rows shown, rows built) by `line` since the last `section`, while searching.
    static ROWS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    /// Rows (or section titles) that matched the search on the whole page.
    static MATCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Placeholder that takes no room; `section` drops it.
pub(super) fn hidden<'a>() -> Element<'a, Message> {
    space().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into()
}

fn is_hidden(e: &Element<'_, Message>) -> bool {
    let size = e.as_widget().size();
    size.width == Length::Fixed(0.0) && size.height == Length::Fixed(0.0)
}

/// The lowercased search text, if any.
fn query(state: &Listen) -> Option<String> {
    let q = state.settings_search.trim().to_lowercase();
    (!q.is_empty()).then_some(q)
}

pub(super) fn section<'a>(
    state: &'a Listen,
    title: &'static str,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let (shown, built) = ROWS.with(|r| r.replace((0, 0)));
    if let Some(q) = query(state) {
        // Rows that don't match are already gone; a section without a match
        // goes too (one without rows only stays when its title matches).
        let keep = if built == 0 { crate::i18n::tr(title).to_lowercase().contains(&q) } else { shown > 0 };
        if keep && built == 0 {
            MATCHES.with(|m| m.set(m.get() + 1));
        }
        if !keep {
            return hidden();
        }
    }
    let mut col =
        Column::new().spacing(metrics::space_md()).push(label_w(title, 20.0, t.foreground, Weight::Bold));
    for r in rows.into_iter().filter(|r| !is_hidden(r)) {
        col = col.push(r);
    }
    container(col)
        .padding(20)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.surface)),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
            ..Default::default()
        })
        .into()
}

/// One option row. While searching, rows whose title and description don't
/// contain the query (case-insensitive) are hidden, like Feishin's filter.
pub(super) fn line<'a>(
    state: &'a Listen,
    name: &'static str,
    hint: impl iced::widget::text::IntoFragment<'a>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let hint = crate::i18n::tr_cow(hint.into_fragment());
    if let Some(q) = query(state) {
        let found = crate::i18n::tr(name).to_lowercase().contains(&q) || hint.to_lowercase().contains(&q);
        ROWS.with(|r| {
            let (shown, built) = r.get();
            r.set((shown + usize::from(found), built + 1));
            MATCHES.with(|m| m.set(m.get() + usize::from(found)));
        });
        if !found {
            return hidden();
        }
    }
    row![
        column![
            label_w(name, metrics::font_md(), t.foreground, Weight::Medium),
            iced::widget::text(hint)
                .font(crate::appearance::current_font())
                .size(metrics::font_sm())
                .color(t.muted),
        ]
        .spacing(2)
        .width(Length::Fill),
        control,
    ]
    .spacing(metrics::space_lg())
    .align_y(Alignment::Center)
    .into()
}

/// A switch whose change maps to any message.
pub(super) fn switch_with<'a>(value: bool, f: fn(bool) -> Message) -> Element<'a, Message> {
    toggler(value).on_toggle(f).size(20).into()
}

pub(super) fn switch<'a>(value: bool, f: fn(bool) -> Setting) -> Element<'a, Message> {
    toggler(value).on_toggle(move |v| Message::Setting(f(v))).size(20).into()
}

/// The sections of one tab, in order.
fn tab_sections(state: &Listen, tab: SettingsTab) -> Vec<Element<'_, Message>> {
    let mut v: Vec<Element<'_, Message>> = Vec::new();
    match tab {
        SettingsTab::General => {
            v.extend(general::servers(state));
            v.push(integrations::scrobble(state));
            v.push(general::appearance(state));
            v.push(general::application(state));
            v.push(controls::home(state));
            v.push(controls::player_bar(state));
            v.push(fullscreen::full_screen(state));
            v.push(controls::sidebar(state));
            v.push(lists::lists(state));
            v.push(artist::artist_page(state));
            v.push(artist::external_links(state));
            v.extend(smart_playlist::query_builder(state));
            v.push(sharing::sharing(state));
        }
        SettingsTab::Playback => {
            v.extend(playback::playback(state));
            v.push(queue::queue(state));
            v.push(queue::sidebar_panels(state));
            v.push(queue::filters(state));
            v.push(playback::visualizer(state));
            v.push(milkdrop::milkdrop(state));
            v.push(playback::auto_dj(state));
            v.push(playback::ai_dj(state));
            v.push(playback::dj_transitions(state));
            v.push(playback::lyrics(state));
        }
        SettingsTab::Hotkeys => {
            // The whole hotkey editor (window + system-wide shortcuts).
            v.push(hotkeys::global_shortcuts(state));
        }
        SettingsTab::Window => {
            v.push(window::integrations(state));
            v.push(integrations::remote(state));
            v.push(integrations::mcp(state));
        }
        SettingsTab::Advanced => {
            v.push(advanced::updates(state));
            v.push(advanced::history(state));
            if cfg!(target_os = "linux") && !state.demo {
                v.push(advanced::ripping(state));
            }
            v.push(general::import_feishin(state));
            v.push(advanced::backup(state));
            v.push(advanced::logging(state));
            v.push(advanced::cache(state));
        }
        SettingsTab::About => v.extend(about::about(state)),
    }
    v
}

fn header(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let search = text_input(crate::i18n::tr("Search"), &state.settings_search)
        .on_input(|v| Message::Shell(ShellMsg::Search(v)))
        .style(style::input(t))
        .width(260);
    let reset = button(label_w("Reset to default", metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(Padding::from([8.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Shell(ShellMsg::ResetAsk));
    row![label_w("Settings", 36.0, t.foreground, Weight::Bold), space().width(Length::Fill), search, reset,]
        .spacing(metrics::space_md())
        .align_y(Alignment::Center)
        .into()
}

fn tab_bar(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let current = state.config.settings.settings_tab;
    let mut bar = Row::new().spacing(metrics::space_xs());
    for tab in SettingsTab::ALL {
        bar = bar.push(
            button(label_w(
                tab.label(),
                metrics::font_md(),
                if tab == current { t.primary } else { t.foreground },
                Weight::Semibold,
            ))
            .padding(Padding::from([8.0, 16.0]))
            .style(style::nav_item(t, tab == current))
            .on_press(Message::Shell(ShellMsg::Tab(tab))),
        );
    }
    bar.into()
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    ROWS.with(|r| r.set((0, 0)));
    MATCHES.with(|m| m.set(0));
    let mut page = Column::new().spacing(metrics::space_xl()).push(header(state));
    let searching = query(state).is_some();
    let sections: Vec<_> = if searching {
        SettingsTab::ALL.into_iter().flat_map(|tab| tab_sections(state, tab)).collect()
    } else {
        page = page.push(tab_bar(state));
        tab_sections(state, state.config.settings.settings_tab)
    };
    let mut any = false;
    for s in sections.into_iter().filter(|s| !is_hidden(s)) {
        any = true;
        page = page.push(s);
    }
    if searching && (!any || MATCHES.with(std::cell::Cell::get) == 0) {
        page = page.push(label("No settings match your search.", metrics::font_md(), t.muted));
    }

    scrollable(
        container(page.padding(Padding { top: 48.0, right: 32.0, bottom: 80.0, left: 32.0 }).max_width(1000))
            .center_x(Length::Fill),
    )
    .style(style::scrollbar(t))
    .height(Length::Fill)
    .into()
}

/// Number slider with its value next to it.
pub(super) fn stepper<'a>(
    state: &'a Listen,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    shown: String,
    f: impl Fn(f32) -> Setting + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    row![
        slider(range, value, move |v| Message::Setting(f(v))).width(160),
        label(shown, metrics::font_md(), t.muted),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .into()
}
