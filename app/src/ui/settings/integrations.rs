//! Scrobble options, Discord rich presence details and the remote control
//! server (Feishin: general/scrobble-settings, window/discord-settings,
//! window/remote-settings).

use super::{line, section, stepper, switch};
use crate::app::integrations::{Field, IntegrationSetting as I, IntegrationsMsg};
use crate::app::{Listen, Message, Setting};
use crate::style;
use iced::widget::{pick_list, row, text_input};
use iced::{Alignment, Element};
use listen_core::settings::{DiscordDisplay, DiscordLink, ScrobbleMode};

fn set(i: I) -> Message {
    Message::Setting(Setting::Integrations(i))
}

fn input(
    state: &Listen,
    placeholder: &str,
    value: &str,
    edited: Option<&String>,
    field: Field,
    secure: bool,
    width: u16,
) -> Element<'static, Message> {
    text_input(crate::i18n::tr(placeholder), edited.map_or(value, String::as_str))
        .on_input(move |v| Message::Integrations(IntegrationsMsg::Input(field, v)))
        .on_submit(Message::Integrations(IntegrationsMsg::Save(field)))
        .secure(secure)
        .style(style::input(state.tokens))
        .width(f32::from(width))
        .into()
}

/// General > Scrobble.
pub(super) fn scrobble(state: &Listen) -> Element<'_, Message> {
    let s = &state.config.settings;
    let (p, ig) = (&s.playback, &s.integrations);
    let mut rows = vec![
        line(
            state,
            "Scrobble",
            "Report plays to the server (Last.fm etc.). Nothing is reported in private mode.",
            switch(p.scrobble, Setting::Scrobble),
        ),
        line(
            state,
            "Scrobble minimum mode",
            "When a play counts: after a share of the song, after some seconds, or whichever comes first. \
             Skipping ahead does not count as listening.",
            pick_list(ScrobbleMode::ALL, Some(ig.scrobble_mode), |m| set(I::ScrobbleMode(m))).into(),
        ),
    ];
    if ig.scrobble_mode != ScrobbleMode::Seconds {
        rows.push(line(
            state,
            "Minimum scrobble percentage",
            "The share of the song that has to be played (25 to 90 %).",
            stepper(
                state,
                25.0..=90.0,
                f32::from(p.scrobble_at_percent),
                format!("{} %", p.scrobble_at_percent),
                |v| Setting::Integrations(I::ScrobblePercent(v.round() as u8)),
            ),
        ));
    }
    if ig.scrobble_mode != ScrobbleMode::Percentage {
        rows.push(line(
            state,
            "Minimum scrobble seconds",
            "The time that has to be played (0 to 1200 s).",
            stepper(
                state,
                0.0..=1200.0,
                p.scrobble_at_secs as f32,
                format!("{} s", p.scrobble_at_secs),
                |v| Setting::Integrations(I::ScrobbleSecs(((v / 5.0).round() * 5.0) as u32)),
            ),
        ));
    }
    rows.push(line(
        state,
        "Song change notifications",
        "Show a desktop notification with the song, its artists, album and cover when the song changes.",
        switch(ig.notify_song_change, |v| Setting::Integrations(I::Notify(v))),
    ));
    section(state, "Scrobble", rows)
}

/// The Discord rows below "Discord application ID" (Window tab).
pub(super) fn discord_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    let d = &state.config.settings.integrations.discord;
    let mut rows = vec![
        line(
            state,
            "Show presence when paused",
            "Keep the presence while paused, with the paused icon and no timer.",
            switch(d.show_paused, |v| Setting::Integrations(I::DiscordPaused(v))),
        ),
        line(
            state,
            "Show playing icon",
            "Show the small playing icon next to the cover while a song plays.",
            switch(d.state_icon, |v| Setting::Integrations(I::DiscordStateIcon(v))),
        ),
        line(
            state,
            "Show status as listening",
            "Show “Listening to” instead of “Playing”.",
            switch(d.show_as_listening, |v| Setting::Integrations(I::DiscordListening(v))),
        ),
        line(
            state,
            "Discord display type",
            "What your status line shows: the application name, the song or the artist.",
            pick_list(DiscordDisplay::ALL, Some(d.display), |v| set(I::DiscordDisplay(v))).into(),
        ),
        line(
            state,
            "Discord link type",
            "Link the song, artist and cover on Discord to Last.fm or MusicBrainz (MusicBrainz links use \
             the ids your server stores; without them it falls back to Last.fm where that is chosen).",
            pick_list(DiscordLink::ALL, Some(d.link), |v| set(I::DiscordLink(v))).into(),
        ),
        line(
            state,
            "Show server cover image",
            "Send the cover from your server to Discord. Only used when the server is reachable from the \
             internet over https; the link includes your server's access token, so Discord and its users \
             can see it. YouTube Music covers are always shown. Covers from local folders are never sent: \
             Discord can only load images from public web addresses.",
            switch(d.server_image, |v| Setting::Integrations(I::DiscordServerImage(v))),
        ),
    ];
    rows.push(line(
        state,
        "Last.fm API key",
        "Looks up the album cover on Last.fm when there is no other cover for Discord. Press Enter to save.",
        input(
            state,
            "API key",
            &d.lastfm_key,
            state.integ.inputs.lastfm.as_ref(),
            Field::LastfmKey,
            true,
            220,
        ),
    ));
    rows
}

/// Window > Remote control.
pub(super) fn remote(state: &Listen) -> Element<'_, Message> {
    let r = &state.config.settings.integrations.remote;
    let t = state.tokens;
    let inputs = &state.integ.inputs;
    let link = match state.integ.remote_addr() {
        Some(addr) => {
            let host =
                if addr.ip().is_unspecified() { "localhost".to_string() } else { addr.ip().to_string() };
            format!("Running: open http://{host}:{} on the phone (use this computer's address).", addr.port())
        }
        None => match state.integ.remote_error() {
            Some(e) => format!("Not running: {e}"),
            None => format!("Serves a remote control page at http://localhost:{}.", r.port),
        },
    };
    let rows = vec![
        line(
            state,
            "Enable remote control server",
            link,
            switch(r.enabled, |v| Setting::Integrations(I::RemoteEnabled(v))),
        ),
        line(
            state,
            "Remote server port",
            "The port of the server. Press Enter to apply.",
            input(state, "4333", &r.port.to_string(), inputs.port.as_ref(), Field::RemotePort, false, 100),
        ),
        line(
            state,
            "Remote server address",
            "0.0.0.0 lets other devices in your network connect, 127.0.0.1 only this computer. Press Enter to apply.",
            input(state, "0.0.0.0", &r.bind, inputs.bind.as_ref(), Field::RemoteBind, false, 160),
        ),
        line(
            state,
            "Remote server username",
            "The user name to sign in with. Leave both fields empty for no sign-in.",
            input(state, "Username", &r.username, inputs.user.as_ref(), Field::RemoteUser, false, 220),
        ),
        line(
            state,
            "Remote server password",
            if state.integ.remote_password_set() {
                "A password is set (kept in the keyring). Type a new one and press Enter, or clear it with Enter on an empty field."
            } else {
                "The password to sign in with. Press Enter to save; it is kept in the keyring, not in the settings file."
            },
            input(
                state,
                if state.integ.remote_password_set() { "••••••••" } else { "Password" },
                "",
                inputs.password.as_ref(),
                Field::RemotePassword,
                true,
                220,
            ),
        ),
    ];
    let warn: Element<'_, Message> = if r.enabled
        && r.username.is_empty()
        && !state.integ.remote_password_set()
        && r.bind != "127.0.0.1"
    {
        row![
            crate::ui::t_text("Anyone in your network can control Listen. Set a username and password.")
                .size(crate::style::metrics::font_sm())
                .color(t.error)
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        super::hidden()
    };
    let mut all = rows;
    all.push(warn);
    section(state, "Remote control", all)
}

/// Window > MCP server: lets an AI assistant control Listen.
pub(super) fn mcp(state: &Listen) -> Element<'_, Message> {
    use crate::app::mcp::McpMsg;
    use crate::style::metrics;
    let m = &state.config.settings.integrations.mcp;
    let t = state.tokens;
    let small = |text: &'static str, msg: McpMsg| -> Element<'_, Message> {
        iced::widget::button(crate::ui::label_w(
            text,
            metrics::font_md(),
            t.foreground,
            iced::font::Weight::Semibold,
        ))
        .padding(iced::Padding::from([8.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Mcp(msg))
        .into()
    };
    let status = match state.mcp.addr() {
        Some(addr) => format!("Running at http://{addr}/mcp."),
        None => match &state.mcp.error {
            Some(e) => format!("Not running: {e}"),
            None => {
                "Lets an AI assistant (Claude, or any MCP client) play music, manage playlists and change \
                     settings and the AI DJ."
                    .to_string()
            }
        },
    };
    let port = state.mcp.port_input.clone().unwrap_or_else(|| m.port.to_string());
    let mut rows = vec![
        line(
            state,
            "Enable MCP server",
            status,
            super::switch_with(m.enabled, |v| Message::Mcp(McpMsg::Enabled(v))),
        ),
        line(
            state,
            "MCP server port",
            "Press Enter to apply.",
            text_input("4334", &port)
                .on_input(|v| Message::Mcp(McpMsg::PortInput(v)))
                .on_submit(Message::Mcp(McpMsg::PortSave))
                .style(style::input(t))
                .width(100.0)
                .into(),
        ),
        line(
            state,
            "Allow other computers",
            "Off: only programs on this computer can connect. On: anyone in your network who has the token can.",
            super::switch_with(m.allow_lan, |v| Message::Mcp(McpMsg::AllowLan(v))),
        ),
        line(
            state,
            "Access token",
            "Clients send it as \"Authorization: Bearer <token>\". It is kept in the keyring. A new token \
             disconnects every client.",
            row![small("Copy token", McpMsg::CopyToken), small("New token", McpMsg::Regenerate)]
                .spacing(metrics::space_sm())
                .into(),
        ),
        line(
            state,
            "Connect Claude Code",
            "Copies the command that adds Listen to Claude Code. Other clients take the same URL and header.",
            small("Copy command", McpMsg::CopySetup),
        ),
    ];
    if m.enabled && m.allow_lan {
        rows.push(
            row![
                crate::ui::t_text(
                    "Anyone in your network with the token can control Listen and change its settings."
                )
                .size(metrics::font_sm())
                .color(t.error)
            ]
            .align_y(Alignment::Center)
            .into(),
        );
    }
    section(state, "MCP server", rows)
}
