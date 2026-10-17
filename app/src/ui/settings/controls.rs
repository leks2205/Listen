//! General tab: player bar controls, sidebar entries and the home carousel (Feishin's labels).

use super::{line, section};
use crate::app::settings_shell::ShellMsg;
use crate::app::sidebar_state::{DragKind, SidebarMsg, SidebarSetting};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label};
use iced::widget::{
    Column, checkbox, column, container, mouse_area, pick_list, row, slider, text_input, toggler,
};
use iced::{Alignment, Element, Padding};
use listen_core::settings::{FolderView, HomeFeatureStyle, PlaylistMode};

fn switch<'a>(value: bool, f: fn(bool) -> ShellMsg) -> Element<'a, Message> {
    toggler(value).on_toggle(move |v| Message::Shell(f(v))).size(20).into()
}

/// Slider with its value (and a unit) next to it.
fn amount<'a>(
    state: &Listen,
    range: std::ops::RangeInclusive<u32>,
    value: u32,
    unit: &'static str,
    f: impl Fn(u32) -> ShellMsg + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    let (lo, hi) = (*range.start() as f32, *range.end() as f32);
    row![
        slider(lo..=hi, value as f32, move |v| Message::Shell(f(v.round() as u32))).width(160),
        label(format!("{value}{unit}"), metrics::font_md(), t.muted),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .into()
}

impl std::fmt::Display for HomeFeatureStyleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            HomeFeatureStyle::Single => "Single",
            HomeFeatureStyle::Multiple => "Multiple",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HomeFeatureStyleChoice(HomeFeatureStyle);

pub(super) fn home(state: &Listen) -> Element<'_, Message> {
    let h = &state.config.settings.home;
    let mut lines = vec![
        line(
            state,
            "Home featured carousel",
            "Controls whether to show the large featured carousel on the home page",
            switch(h.feature, ShellMsg::HomeFeature),
        ),
        line(
            state,
            "Home featured carousel style",
            "Controls the style of the home featured carousel",
            pick_list(
                [HomeFeatureStyle::Single, HomeFeatureStyle::Multiple].map(HomeFeatureStyleChoice),
                Some(HomeFeatureStyleChoice(h.feature_style)),
                |c| Message::Shell(ShellMsg::HomeFeatureStyle(c.0)),
            )
            .into(),
        ),
    ];
    lines.push(line(
        state,
        "Home sections",
        "Select the items and order in which they appear on the home page",
        row![].into(),
    ));
    lines.extend(crate::ui::home::section_rows(state));
    section(state, "Home", lines)
}

pub(super) fn player_bar(state: &Listen) -> Element<'_, Message> {
    let b = &state.config.settings.player_bar;
    let mut rows = vec![
        line(
            state,
            "Player bar button size",
            "The size of the player bar buttons",
            amount(state, 15..=30, u32::from(b.button_size), " px", |v| ShellMsg::ButtonSize(v as u16)),
        ),
        line(
            state,
            "Show skip buttons",
            "Show or hide the skip buttons on the player bar",
            switch(b.skip_buttons, ShellMsg::SkipButtons),
        ),
        line(
            state,
            "Skip backward duration",
            "Sets the duration to skip when using the skip buttons on the player bar (backward)",
            amount(state, 0..=120, b.skip_backward_secs, " s", ShellMsg::SkipBack),
        ),
        line(
            state,
            "Skip forward duration",
            "Sets the duration to skip when using the skip buttons on the player bar (forward)",
            amount(state, 0..=120, b.skip_forward_secs, " s", ShellMsg::SkipForward),
        ),
        line(
            state,
            "Volume slider width",
            "The width of the volume slider",
            amount(state, 25..=300, u32::from(b.volume_width), " px", |v| ShellMsg::VolumeWidth(v as u16)),
        ),
        line(
            state,
            "Volume wheel step",
            "The amount of volume to change when scrolling the mouse wheel on the volume slider",
            amount(state, 1..=20, u32::from(b.volume_wheel_step), "", |v| ShellMsg::VolumeWheelStep(v as u8)),
        ),
    ];
    rows.extend(super::fullscreen::player_bar_rows(state));
    // Slider type and waveform options (ui/waveform.rs).
    rows.extend(crate::ui::waveform::settings_rows(state));
    section(state, "Player bar", rows)
}

/// The sidebar section: items (drag to reorder, tick to show), playlist list and rail.
/// Feishin: `general/sidebar-settings.tsx`, `sidebar-reorder.tsx`.
pub(super) fn sidebar(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let sb = &state.config.settings.sidebar;
    let set = |s: SidebarSetting| Message::Sidebar(SidebarMsg::Setting(s));

    let drag = state.sb.drag.as_ref().filter(|d| d.kind == DragKind::Item);
    let mut items = Column::new().spacing(2);
    for (i, item) in sb.items.iter().enumerate() {
        let target = drag.is_some_and(|d| d.over == item.id && d.from != item.id);
        let grip = mouse_area(container(icon(Icon::LuMenu, metrics::font_md(), t.muted)).padding(6))
            .interaction(iced::mouse::Interaction::Grab)
            .on_press(Message::Sidebar(SidebarMsg::DragStart(DragKind::Item, item.id.clone())));
        let row = row![
            grip,
            checkbox(item.visible)
                .label(capitalise(&item.label))
                .on_toggle(move |v| Message::Shell(ShellMsg::SidebarVisible(i, v)))
                .size(18)
                .text_size(metrics::font_md()),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center);
        items = items.push(
            mouse_area(container(row).width(iced::Length::Fill).padding(Padding::from([2.0, 6.0])).style(
                move |_| container::Style {
                    background: target.then_some(t.nav_hover.into()),
                    border: iced::Border {
                        color: if target { t.primary } else { iced::Color::TRANSPARENT },
                        width: if target { 2.0 } else { 0.0 },
                        radius: t.radius_sm.into(),
                    },
                    ..Default::default()
                },
            ))
            .on_enter(Message::Sidebar(SidebarMsg::DragOver(item.id.clone()))),
        );
    }

    let toggle = |value: bool, f: fn(bool) -> SidebarSetting| -> Element<'_, Message> {
        toggler(value).on_toggle(move |v| Message::Sidebar(SidebarMsg::Setting(f(v)))).size(20).into()
    };
    let filter_bad = !sb.playlist_filter_regex.is_empty()
        && crate::app::sidebar_state::compile_filter(&sb.playlist_filter_regex).is_none();
    let regex_box = column![
        text_input(crate::i18n::tr("E.g. ^daily mix.*"), &sb.playlist_filter_regex)
            .on_input(move |s| set(SidebarSetting::FilterRegex(s)))
            .padding(8)
            .width(220)
            .style(style::input(t)),
    ]
    .extend(filter_bad.then(|| -> Element<'_, Message> {
        label("Not a valid expression: nothing is hidden", metrics::font_xs(), t.error).into()
    }))
    .spacing(4);

    let mut rows: Vec<Element<'_, Message>> = vec![
        line(
            state,
            "Sidebar configuration",
            "Select the items and order in which they appear in the sidebar",
            row![].into(),
        ),
        container(items).padding(Padding::from([0.0, 8.0])).into(),
        line(
            state,
            "Sidebar playlist list",
            "Show or hide the playlist list in the sidebar",
            toggle(sb.show_playlists, SidebarSetting::PlaylistList),
        ),
        line(
            state,
            "Playlist filter regex",
            "Hide playlists in the sidebar that match this regular expression",
            regex_box.into(),
        ),
        line(
            state,
            "Sidebar playlist sorting",
            "Allows manual playlist sorting in the sidebar using drag and drop instead of the default server order",
            toggle(sb.playlist_sorting, SidebarSetting::Sorting),
        ),
        line(
            state,
            "Sidebar playlist mode",
            "How each playlist is displayed in the sidebar list",
            pick_list(PlaylistMode::ALL, Some(sb.playlist_mode), move |v| set(SidebarSetting::Mode(v)))
                .width(200)
                .into(),
        ),
        line(
            state,
            "Enable folders",
            "Create a folder view for playlists that include the configured separator in the name",
            toggle(sb.playlist_folders, SidebarSetting::Folders),
        ),
    ];
    if sb.playlist_folders {
        let separator = state.sb.separator_input.as_deref().unwrap_or(&sb.playlist_folder_separator);
        rows.push(line(
            state,
            "Folder separator",
            "Character (or string) that separates folder levels in a playlist name",
            text_input(crate::i18n::tr("/"), separator)
                .on_input(move |s| set(SidebarSetting::Separator(s)))
                .padding(8)
                .width(120)
                .style(style::input(t))
                .into(),
        ));
        rows.push(line(
            state,
            "Folder view",
            "How folders are displayed in the sidebar",
            pick_list(FolderView::ALL, Some(sb.playlist_folder_view), move |v| {
                set(SidebarSetting::FolderView(v))
            })
            .width(200)
            .into(),
        ));
        if sb.playlist_folder_view == FolderView::Tree {
            rows.push(line(
                state,
                "Tree indent",
                "Pixels each tree level is indented",
                row![
                    slider(0.0..=64.0, f32::from(sb.folder_tree_indent), move |v| set(
                        SidebarSetting::TreeIndent(v.round() as u16)
                    ))
                    .width(160),
                    label(format!("{} px", sb.folder_tree_indent), metrics::font_md(), t.muted),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center)
                .into(),
            ));
            rows.push(line(
                state,
                "Tree line color",
                "Color of the connecting tree lines (leave empty for theme default), as #rrggbb",
                text_input(crate::i18n::tr("#rrggbb"), &sb.folder_tree_line_color)
                    .on_input(move |s| set(SidebarSetting::LineColor(s)))
                    .padding(8)
                    .width(120)
                    .style(style::input(t))
                    .into(),
            ));
        }
    }
    rows.push(line(
        state,
        "Sidebar (collapsed) navigation",
        "Show or hide the navigation in the collapsed sidebar",
        toggle(sb.collapsed_navigation, SidebarSetting::CollapsedNavigation),
    ));
    section(state, "Sidebar", rows)
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}
