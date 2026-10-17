//! Playback tab: the queue (Feishin: application, control and sidebar
//! settings) and the player filters.

use super::{line, section};
use crate::app::queue_ui::{QueueMsg, QueueSetting};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label};
use iced::widget::{button, column, pick_list, row, slider, text_input, toggler};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::{FilterField, FilterOp, SideQueueLayout, SideQueueStyle};

fn setting(s: QueueSetting) -> Message {
    Message::Queue(QueueMsg::Setting(s))
}

fn switch<'a>(value: bool, f: fn(bool) -> QueueSetting) -> Element<'a, Message> {
    toggler(value).on_toggle(move |v| setting(f(v))).size(20).into()
}

/// The panels of the right sidebar, with buttons that move them.
fn panel_order(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let mut order = state.config.settings.sidebar.panel_order.clone();
    crate::ui::queue::normalize_order(&mut order);
    let last = order.len() - 1;
    let mut col = column![].spacing(metrics::space_xs());
    for (i, name) in order.iter().enumerate() {
        let title = match name.as_str() {
            "queue" => "Queue",
            "lyrics" => "Lyrics",
            _ => "Visualizer",
        };
        let mv = |glyph: Icon, by: i8, enabled: bool| {
            crate::ui::tip::tip_icon(
                state,
                button(icon(glyph, metrics::font_md(), if enabled { t.foreground } else { t.muted }))
                    .padding(4)
                    .style(style::icon_button(t, false))
                    .on_press_maybe(enabled.then(|| setting(QueueSetting::MovePanel(name.clone(), by)))),
                glyph,
                crate::ui::tip::Position::Left,
            )
        };
        col = col.push(
            row![
                label(title, metrics::font_md(), t.foreground).width(110),
                mv(Icon::LuChevronUp, -1, i > 0),
                mv(Icon::LuChevronDown, 1, i < last),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

pub(super) fn queue(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings;
    let q = &s.queue;
    let every: Element<'_, Message> = row![
        slider(1.0..=20.0, q.autosave_every as f32, |v| setting(QueueSetting::AutosaveEvery(
            v.round() as u32
        )))
        .width(140),
        label(format!("{}", q.autosave_every), metrics::font_md(), t.muted),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .into();
    section(
        state,
        "Queue",
        vec![
            line(
                state,
                "Follow current song",
                "Scroll the queue to the playing song whenever the track changes.",
                switch(q.follow_current_song, QueueSetting::FollowCurrent),
            ),
            line(
                state,
                "Up next queue",
                "Label the songs added with Play next and Add to queue as Up Next. They play right after the current song, ahead of the rest of the queue and the Auto-DJ songs.",
                switch(q.up_next_queue, QueueSetting::UpNext),
            ),
            line(
                state,
                "Confirm queue changes",
                "Ask before playing something replaces the songs in the queue, or the queue is cleared.",
                switch(q.confirm_queue_changes, QueueSetting::Confirm),
            ),
            line(
                state,
                "Save play queue",
                "Keep the queue and where you were when Listen closes, and restore it on the next start.",
                switch(q.save_play_queue, QueueSetting::SavePlayQueue),
            ),
            line(
                state,
                "Autosave play queue to server",
                "Save the queue to the server (OpenSubsonic, Navidrome) as the track changes, so another app can pick it up.",
                switch(q.autosave_server, QueueSetting::AutosaveServer),
            ),
            line(state, "Autosave every", "Track changes between two saves to the server.", every),
            line(
                state,
                "Side play queue style",
                "Attached is a panel at the side; Detached is a drawer that slides in when you rest the pointer on the right edge.",
                pick_list([SideQueueStyle::Attached, SideQueueStyle::Detached], Some(q.side_style), |v| {
                    setting(QueueSetting::Style(v))
                })
                .into(),
            ),
            line(
                state,
                "Side queue layout",
                "Horizontal puts the queue in a column on the right; vertical puts it in a panel under the page.",
                pick_list(
                    [SideQueueLayout::Horizontal, SideQueueLayout::Vertical],
                    Some(q.side_layout),
                    |v| setting(QueueSetting::Layout(v)),
                )
                .into(),
            ),
        ],
    )
}

pub(super) fn sidebar_panels(state: &Listen) -> Element<'_, Message> {
    let s = &state.config.settings;
    section(
        state,
        "Player sidebar panels",
        vec![
            line(
                state,
                "Queue panel",
                "Show the play queue in the right sidebar.",
                switch(s.sidebar.show_queue, QueueSetting::QueuePanel),
            ),
            line(
                state,
                "Play mode tag",
                "Show the play mode (Shuffle, Auto or AI) above the sidebar queue, and Auto and AI tags on the songs the Auto-DJ and the AI DJ added.",
                switch(s.queue.show_mode_tag, QueueSetting::ModeTag),
            ),
            line(
                state,
                "Lyrics panel",
                "Show the lyrics of the playing song in the right sidebar.",
                switch(s.queue.lyrics_in_sidebar, QueueSetting::LyricsInSidebar),
            ),
            line(
                state,
                "Visualizer panel",
                "Show the spectrum in the right sidebar (captures only Listen's own audio).",
                super::switch(s.visualizer.in_queue_panel, crate::app::Setting::QueueVisualizer),
            ),
            line(
                state,
                "Combine lyrics and visualizer",
                "Show lyrics with the visualizer under them in one panel.",
                switch(s.queue.combined_panel, QueueSetting::Combined),
            ),
            line(
                state,
                "Panel order",
                "The order of the panels in the right sidebar, top to bottom.",
                panel_order(state),
            ),
        ],
    )
}

/// Feishin's Player filters: rules that keep songs out of the queue.
pub(super) fn filters(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let q = &state.config.settings.queue;
    let mut rows: Vec<Element<'_, Message>> = vec![line(
        state,
        "Player filters",
        "Songs that match a rule are kept out of the queue when you play or queue them. Auto-DJ and the AI DJ are not filtered.",
        button(label("Add rule", metrics::font_md(), t.foreground))
            .padding(Padding::from([6.0, 14.0]))
            .style(style::default_button(t))
            .on_press(setting(QueueSetting::FilterAdd))
            .into(),
    )];
    for (i, f) in q.filters.iter().enumerate() {
        let ops = FilterOp::for_kind(f.field.kind());
        let rule = row![
            toggler(f.enabled).on_toggle(move |v| setting(QueueSetting::FilterEnabled(i, v))).size(20),
            pick_list(FilterField::ALL, Some(f.field), move |v| setting(QueueSetting::FilterField(i, v)))
                .width(150),
            pick_list(ops, Some(f.op), move |v| setting(QueueSetting::FilterOp(i, v))).width(170),
            text_input(crate::i18n::tr("Value"), &f.value)
                .on_input(move |v| setting(QueueSetting::FilterValue(i, v)))
                .style(style::input(t))
                .width(Length::Fill),
            crate::ui::tip::tip(
                state,
                button(icon(Icon::LuTrash2, metrics::font_md(), t.muted))
                    .padding(6)
                    .style(style::icon_button(t, false))
                    .on_press(setting(QueueSetting::FilterRemove(i))),
                "Remove filter",
                None,
                crate::ui::tip::Position::Left,
            ),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center);
        rows.push(rule.into());
        if f.invalid_regex() {
            rows.push(
                label(
                    "This is not a valid regular expression, so the rule matches nothing.",
                    metrics::font_sm(),
                    t.error,
                )
                .into(),
            );
        }
    }
    section(state, "Player filters", rows)
}
