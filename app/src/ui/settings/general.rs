//! General tab: servers, appearance, Feishin import.

use super::appearance as rows;
use super::{line, section, switch};
use crate::app::servers::ServersMsg;
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{button, column, row, slider, text_input};
use iced::{Alignment, Element, Length, Padding};
use listen_core::config::SecretStore;

pub(super) fn servers(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let _s = &state.config.settings;
    if state.demo {
        return None;
    }
    let mut rows: Vec<Element<'_, Message>> = Vec::new();
    for server in &state.config.servers {
        let active = state.config.active_server.as_ref() == Some(&server.id);
        let detail = match server.kind {
            listen_core::model::SourceKind::YouTubeMusic => server.kind.label().to_string(),
            listen_core::model::SourceKind::Local => format!("{} — {}", server.kind.label(), server.url),
            _ => format!("{} — {} as {}", server.kind.label(), server.url, server.username),
        };
        let switch_button = button(label_w(
            if active { "Browsing" } else { "Switch" },
            metrics::font_md(),
            if active { t.muted } else { iced::Color::WHITE },
            Weight::Semibold,
        ))
        .padding(Padding::from([8.0, 16.0]))
        .style(style::primary_button(t))
        .on_press_maybe((!active).then(|| Message::SwitchServer(server.id.clone())));
        let rescan_button: Option<Element<'_, Message>> =
            (server.kind == listen_core::model::SourceKind::Local).then(|| {
                button(label_w("Rescan", metrics::font_md(), t.foreground, Weight::Semibold))
                    .padding(Padding::from([8.0, 16.0]))
                    .style(style::default_button(t))
                    .on_press(Message::RescanServer(server.id.clone()))
                    .into()
            });
        let edit_button: Element<'_, Message> =
            button(label_w("Edit", metrics::font_md(), t.foreground, Weight::Semibold))
                .padding(Padding::from([8.0, 16.0]))
                .style(style::default_button(t))
                .on_press(Message::Servers(ServersMsg::Edit(server.id.clone())))
                .into();
        let remove_button: Element<'_, Message> = if state.confirm_remove.as_ref() == Some(&server.id) {
            crate::ui::server_remove(state, &server.id)
        } else {
            button(label_w("Remove", metrics::font_md(), t.foreground, Weight::Semibold))
                .padding(Padding::from([8.0, 16.0]))
                .style(style::default_button(t))
                .on_press(Message::ConfirmRemoveServer(Some(server.id.clone())))
                .into()
        };
        rows.push(
            row![
                column![
                    label_w(server.name.clone(), metrics::font_md(), t.foreground, Weight::Medium),
                    label(detail, metrics::font_sm(), t.muted),
                ]
                .spacing(2)
                .width(Length::Fill),
                switch_button,
            ]
            .extend(rescan_button)
            .push(edit_button)
            .push(remove_button)
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        );
    }
    rows.push(
        button(label_w("Add server", metrics::font_md(), iced::Color::WHITE, Weight::Semibold))
            .padding(Padding::from([8.0, 16.0]))
            .style(style::primary_button(t))
            .on_press(Message::AddServer)
            .into(),
    );
    rows.push(secret_store_row(state));
    Some(section(state, "Servers", rows))
}

/// Where passwords are kept: the system keyring or the 0600 file (Feishin's safeStorage selector).
fn secret_store_row(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let store = state.config.secret_store;
    let described = state
        .secret_store_description()
        .map_or_else(|| "Not available.".to_string(), |d| format!("Passwords are kept in {d}."));
    let pick = |title: &'static str, value: SecretStore| {
        let active = store == value;
        let b = button(label_w(
            title,
            metrics::font_md(),
            if active { iced::Color::WHITE } else { t.foreground },
            Weight::Semibold,
        ))
        .padding(Padding::from([8.0, 16.0]))
        .on_press_maybe((!active).then_some(Message::Servers(ServersMsg::SetSecretStore(value))));
        if active { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) }
    };
    line(
        state,
        "Password storage",
        described,
        row![pick("System keyring", SecretStore::Auto), pick("File only", SecretStore::File)]
            .spacing(metrics::space_sm())
            .into(),
    )
}

pub(super) fn appearance(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings;
    let a = &s.appearance;
    let desktop_hint = match (&state.desktop_theme, a.follow_desktop_theme) {
        (Some(t), true) => format!("Using the desktop's {} colours; changes apply within a second.", t.name),
        (None, true) => {
            "No desktop theme found (KDE colour scheme / theme switcher); using the theme below.".into()
        }
        (_, false) => "Use the KDE colour scheme / theme switcher colours and follow changes live.".into(),
    };
    let mut appearance = vec![];
    // KDE colour schemes / theme switchers: nothing to follow on Windows.
    if !cfg!(windows) {
        appearance.push(line(
            state,
            "Follow desktop theme",
            desktop_hint,
            switch(a.follow_desktop_theme, Setting::FollowDesktop),
        ));
    }
    appearance.push(line(
        state,
        "Follow system theme",
        "Use the dark/light theme below depending on the desktop.",
        switch(a.follow_system_theme, Setting::FollowSystem),
    ));
    if a.follow_system_theme {
        appearance.push(line(
            state,
            "Dark theme",
            "Used when the desktop is dark.",
            rows::theme_pick(state, &a.theme_dark, Setting::ThemeDark),
        ));
        appearance.push(line(
            state,
            "Light theme",
            "Used when the desktop is light.",
            rows::theme_pick(state, &a.theme_light, Setting::ThemeLight),
        ));
    } else {
        appearance.push(line(
            state,
            "Theme",
            "Feishin's theme collection, the desktop rice themes and your own (themes folder).",
            rows::theme_pick(state, &a.theme, Setting::Theme),
        ));
    }
    appearance.push(line(
        state,
        "Use the theme's accent colour",
        "Otherwise your custom accent below is used.",
        switch(a.use_theme_accent, Setting::UseThemeAccent),
    ));
    appearance.extend(rows::colour_rows(state));
    let speed = (a.scroll_speed * 100.0).round() as u16;
    appearance.push(line(
        state,
        "Touchpad scroll speed",
        "100 % scrolls as far as Chromium browsers (Ecosia, Discord) for the same swipe.",
        row![
            slider(25.0..=200.0, f32::from(speed), |v| Message::Setting(Setting::ScrollSpeed(v as u16)))
                .step(5.0_f32)
                .width(160),
            label(format!("{speed} %"), metrics::font_md(), t.muted),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .into(),
    ));
    section(state, "Appearance", appearance)
}

/// General > Application: font, zoom, header backgrounds and cover options.
pub(super) fn application(state: &Listen) -> Element<'_, Message> {
    section(state, "Application", rows::application_rows(state))
}

pub(super) fn import_feishin(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let _s = &state.config.settings;
    section(
        state,
        "Import from Feishin",
        vec![line(
            state,
            "Feishin settings file",
            "Path to a Feishin settings export (JSON). Passwords are ignored.",
            row![
                text_input(crate::i18n::tr("~/Downloads/feishin-settings.json"), &state.import_path)
                    .on_input(|v| Message::Setting(Setting::ImportPath(v)))
                    .on_submit(Message::Setting(Setting::ImportFeishin))
                    .style(style::input(t))
                    .width(320),
                button(label_w("Import", metrics::font_md(), iced::Color::WHITE, Weight::Semibold))
                    .padding(Padding::from([8.0, 16.0]))
                    .style(style::primary_button(t))
                    .on_press(Message::Setting(Setting::ImportFeishin)),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        )],
    )
}
