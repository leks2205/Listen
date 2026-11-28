//! Settings page (subset of Feishin's: appearance, playback/EQ, Auto-DJ,
//! lyrics, window & integrations, Feishin import, server).
//!
//! Search contract: rows built with [`line`] or [`searchable`] are counted for the [`section`] that
//! follows them and hide themselves when the query misses them, so build a section's rows right
//! before calling `section` for it. Rows that belong to a line (a list under a heading) go through [`child`]; a search for a section's title shows the whole section. A row
//! that is a 0×0 fixed-size widget counts as [`hidden`] and is dropped.

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
    /// Whether the last [`line`] matched the search (the rows that belong to it follow its fate).
    /// Titles of sections whose title matched the search while some of their rows did not (pass 1).
    static TITLE_HITS: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Pass 2 of a search: build these sections with every row shown and drop the others.
    static FORCE: std::cell::RefCell<Option<Vec<&'static str>>> = const { std::cell::RefCell::new(None) };
    static LAST_FOUND: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// A row that belongs to the [`line`] built just before it (a list of items under a heading line):
/// while searching it is shown only when that line matched, so a sub-list is not left without its heading.
pub(super) fn child<'a>(state: &Listen, row: Element<'a, Message>) -> Element<'a, Message> {
    if query(state).is_some() && !LAST_FOUND.with(std::cell::Cell::get) { hidden() } else { row }
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
    let forced = FORCE.with(|f| f.borrow().as_ref().map(|titles| titles.contains(&title)));
    if forced == Some(false) {
        return hidden();
    }
    if let (Some(q), None) = (query(state), forced) {
        // Rows that don't match are already gone; a section without a match
        // goes too (one without rows only stays when its title matches).
        let title_hit = crate::i18n::tr(title).to_lowercase().contains(&q);
        let keep = if built == 0 { title_hit } else { shown > 0 };
        if keep && built == 0 {
            MATCHES.with(|m| m.set(m.get() + 1));
        }
        if title_hit && built > shown {
            // Searching a section's name shows the whole section (view() builds it again with all rows).
            TITLE_HITS.with(|h| h.borrow_mut().push(title));
            if shown == 0 {
                MATCHES.with(|m| m.set(m.get() + 1));
            }
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
    line_in(state, name, hint, control, false)
}

/// A [`line`] whose control is wide (several fields and a button) and always goes under the text:
/// beside it the description was squeezed into a 120 px column.
pub(super) fn line_below<'a>(
    state: &'a Listen,
    name: &'static str,
    hint: impl iced::widget::text::IntoFragment<'a>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    line_in(state, name, hint, control, true)
}

fn line_in<'a>(
    state: &'a Listen,
    name: &'static str,
    hint: impl iced::widget::text::IntoFragment<'a>,
    control: Element<'a, Message>,
    below: bool,
) -> Element<'a, Message> {
    let t = state.tokens;
    let hint = crate::i18n::tr_cow(hint.into_fragment());
    if let (Some(q), false) = (query(state), FORCE.with(|f| f.borrow().is_some())) {
        let found = crate::i18n::tr(name).to_lowercase().contains(&q) || hint.to_lowercase().contains(&q);
        ROWS.with(|r| {
            let (shown, built) = r.get();
            r.set((shown + usize::from(found), built + 1));
            MATCHES.with(|m| m.set(m.get() + usize::from(found)));
        });
        LAST_FOUND.with(|l| l.set(found));
        if !found {
            return hidden();
        }
    }
    let text = column![
        label_w(name, metrics::font_md(), t.foreground, Weight::Medium),
        iced::widget::text(hint)
            .font(crate::appearance::current_font())
            .size(metrics::font_sm())
            .color(t.muted),
    ]
    .spacing(2)
    .width(Length::Fill);
    if below || narrow(state) {
        // No room beside the text: the control goes under it instead of squeezing it to a word a line.
        return column![text, control].spacing(metrics::space_sm()).into();
    }
    row![text, control].spacing(metrics::space_lg()).align_y(Alignment::Center).into()
}

/// The page is too narrow for a control beside its text (compact windows, a wide sidebar or queue).
pub(super) fn narrow(state: &Listen) -> bool {
    state.page_width() - 2.0 * 32.0 - 2.0 * 20.0 < 560.0
}

/// A row that is not a [`line`] (e.g. a hotkey) taking part in the search: it counts like a line and
/// is hidden when none of `keys` (English UI texts, matched translated, or data) contains the query.
pub(super) fn searchable<'a>(
    state: &Listen,
    keys: &[&str],
    row: Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(q) = query(state) else { return row };
    let found = keys.iter().any(|k| crate::i18n::tr(k).to_lowercase().contains(&q));
    ROWS.with(|r| {
        let (shown, built) = r.get();
        r.set((shown + usize::from(found), built + 1));
    });
    MATCHES.with(|m| m.set(m.get() + usize::from(found)));
    if found { row } else { hidden() }
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
            v.push(advanced::hidden_songs(state));
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
    let title = label_w("Settings", 36.0, t.foreground, Weight::Bold);
    // The title (about 270 px), the search field (260) and the reset button (up to 200 in German) in one row.
    if state.page_width() - 2.0 * 32.0 < 800.0 {
        // Title on its own line, the search field takes what the reset button leaves.
        return column![
            title,
            row![search.width(Length::Fill), reset].spacing(metrics::space_md()).align_y(Alignment::Center)
        ]
        .spacing(metrics::space_md())
        .into();
    }
    row![title, space().width(Length::Fill), search, reset,]
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
    // A row of tabs that wraps: at 480 px "Erweitert" and "Über" would be off the window.
    bar.wrap().vertical_spacing(metrics::space_xs()).into()
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    ROWS.with(|r| r.set((0, 0)));
    MATCHES.with(|m| m.set(0));
    // The title, search and tabs stay put; only the sections scroll (they used to scroll away, and
    // with them the way to another tab).
    let mut top = Column::new().spacing(metrics::space_xl()).push(header(state));
    let mut page = Column::new().spacing(metrics::space_xl());
    let searching = query(state).is_some();
    let sections: Vec<_> = if searching {
        let build =
            || SettingsTab::ALL.into_iter().flat_map(|tab| tab_sections(state, tab)).collect::<Vec<_>>();
        TITLE_HITS.with(|h| h.borrow_mut().clear());
        let mut sections = build();
        let hits = TITLE_HITS.with(|h| std::mem::take(&mut *h.borrow_mut()));
        if !hits.is_empty() {
            // A section found by its name: build it again with all its rows and swap it in.
            let counts = (ROWS.with(std::cell::Cell::get), MATCHES.with(std::cell::Cell::get));
            FORCE.with(|f| *f.borrow_mut() = Some(hits));
            let full = build();
            FORCE.with(|f| *f.borrow_mut() = None);
            ROWS.with(|r| r.set(counts.0));
            MATCHES.with(|m| m.set(counts.1));
            if full.len() == sections.len() {
                sections =
                    sections.into_iter().zip(full).map(|(a, b)| if is_hidden(&b) { a } else { b }).collect();
            }
        }
        sections
    } else {
        top = top.push(tab_bar(state));
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

    if state.window_size.height < 520.0 {
        // A short window has no room for a fixed header: everything scrolls.
        let all = Column::new().spacing(metrics::space_xl()).push(top).push(page);
        return scrollable(
            container(
                all.padding(Padding { top: 48.0, right: 32.0, bottom: 80.0, left: 32.0 }).max_width(1000),
            )
            .center_x(Length::Fill),
        )
        .style(style::scrollbar(t))
        .height(Length::Fill)
        .into();
    }
    let fixed = container(
        container(top.padding(Padding { top: 48.0, right: 32.0, bottom: 0.0, left: 32.0 }).max_width(1000))
            .center_x(Length::Fill),
    );
    column![
        fixed,
        scrollable(
            container(
                page.padding(Padding { top: metrics::space_xl(), right: 32.0, bottom: 80.0, left: 32.0 })
                    .max_width(1000)
            )
            .center_x(Length::Fill),
        )
        .style(style::scrollbar(t))
        .height(Length::Fill)
    ]
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

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn searchable_rows_count_like_lines() {
        let (mut s, _) = Listen::boot(&crate::Args { demo: true, ephemeral: true, ..crate::Args::default() });
        s.settings_search = "shuff".into();
        ROWS.with(|r| r.set((0, 0)));
        let shown = searchable(&s, &["Toggle shuffle"], space().into());
        let gone = searchable(&s, &["Volume up"], space().into());
        assert!(!is_hidden(&shown) && is_hidden(&gone));
        assert_eq!(ROWS.with(|r| r.replace((0, 0))), (1, 2));
    }
}
