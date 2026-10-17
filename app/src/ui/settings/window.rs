//! Window tab: window bar, tray, close behaviour, sleep, Discord.

use super::{line, section, switch};
use crate::app::integrations::IntegrationsMsg;
use crate::app::settings_shell::ShellMsg;
use crate::app::{Listen, Message, Setting, WindowSetting};
use crate::style;
use iced::Element;
use iced::widget::{pick_list, text_input, toggler};
use listen_core::settings::WindowBarStyle;

fn window_switch<'a>(value: bool, f: fn(bool) -> WindowSetting) -> Element<'a, Message> {
    toggler(value).on_toggle(move |v| Message::Setting(Setting::Window(f(v)))).size(20).into()
}

pub(super) fn integrations(state: &Listen) -> Element<'_, Message> {
    let _t = state.tokens;
    let s = &state.config.settings;
    let w = &s.window;
    let mut rows = vec![
        line(
            state,
            "Private mode",
            "Nothing is scrobbled, shown on Discord or saved in the history, and the window title says so. \
             Off again when Listen restarts.",
            iced::widget::toggler(state.private_mode())
                .on_toggle(|v| Message::Integrations(IntegrationsMsg::SetPrivate(v)))
                .size(20)
                .into(),
        ),
        line(
            state,
            "Window bar style",
            "Windows or macOS buttons in Listen's own bar, or the desktop's native frame.",
            pick_list(WindowBarStyle::ALL, Some(w.bar_style()), |style| {
                Message::Setting(Setting::Window(WindowSetting::BarStyle(style)))
            })
            .into(),
        ),
        line(
            state,
            "Track info in window title",
            "Show the playing song in the window bar and title.",
            window_switch(w.title_bar_track_info, WindowSetting::TrackInfo),
        ),
        line(state, "Tray icon", "Show Listen in the system tray.", switch(w.tray, Setting::Tray)),
    ];
    if w.tray {
        rows.push(line(
            state,
            "Minimize to tray",
            "Minimizing hides the window in the tray; click the icon to show it again.",
            window_switch(w.minimize_to_tray, WindowSetting::MinimizeToTray),
        ));
        rows.push(line(
            state,
            "Close to tray",
            "Closing the window keeps Listen playing in the tray.",
            switch(w.exit_to_tray, Setting::ExitToTray),
        ));
        rows.push(line(
            state,
            "Start minimized",
            "Start with only the tray icon.",
            window_switch(w.start_minimized, WindowSetting::StartMinimized),
        ));
    }
    rows.push(line(
        state,
        "Prevent display sleep while playing",
        "Keeps the screen on and the system awake while music plays.",
        window_switch(w.prevent_display_sleep, WindowSetting::PreventDisplaySleep),
    ));
    let system: Element<'_, Message> = if w.prevent_display_sleep {
        toggler(w.prevent_system_sleep).size(20).into()
    } else {
        window_switch(w.prevent_system_sleep, WindowSetting::PreventSystemSleep)
    };
    rows.push(line(
        state,
        "Prevent system sleep while playing",
        "Keeps the system from suspending while music plays; the screen may still turn off.",
        system,
    ));
    rows.push(line(
        state,
        "Discord Rich Presence",
        "Show what you're listening to on Discord.",
        switch(w.discord_rpc, Setting::Discord),
    ));
    rows.push(line(
        state,
        "Discord application ID",
        "The application ID for Discord rich presence (defaults to 1560790551337640127). Press Enter to apply.",
        text_input(crate::i18n::tr("1560790551337640127"), state.discord_id_input.as_deref().unwrap_or(&w.discord_client_id))
            .on_input(|v| Message::Shell(ShellMsg::DiscordIdInput(v)))
            .on_submit(Message::Shell(ShellMsg::DiscordIdSave))
            .style(style::input(state.tokens))
            .width(220)
            .into(),
    ));
    rows.extend(super::integrations::discord_rows(state));
    section(state, "Window & integrations", rows)
}
