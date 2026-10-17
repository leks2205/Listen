//! Sign-in extras and server dialogs: option toggles, LAN discovery, Jellyfin Quick Connect, the
//! edit-server dialog and the action-required page.

use super::overlay::ModalParts;
use super::{icon, label, label_w};
use crate::app::servers::{EditForm, Failure, ServersMsg as S, failure_text};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, Tokens, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, text_input, toggler};
use iced::{Alignment, Element, Length, Padding};
use listen_core::config::ServerOptions;
use listen_core::model::SourceKind;

/// The messages an option toggle sends (sign-in page or edit dialog).
#[derive(Clone, Copy)]
pub struct OptionMsgs {
    pub legacy: fn(bool) -> S,
    pub ssl: fn(bool) -> S,
    pub mix: fn(bool) -> S,
}

pub const LOGIN: OptionMsgs =
    OptionMsgs { legacy: S::LegacyAuth, ssl: S::IgnoreSsl, mix: S::PreferInstantMix };
pub const EDIT: OptionMsgs =
    OptionMsgs { legacy: S::EditLegacyAuth, ssl: S::EditIgnoreSsl, mix: S::EditPreferInstantMix };

fn small_text<'a>(s: &'a str, color: iced::Color) -> Element<'a, Message> {
    crate::ui::t_text(s).font(crate::appearance::current_font()).size(metrics::font_sm()).color(color).into()
}

fn toggle<'a>(
    t: Tokens,
    title: &'a str,
    hint: &'a str,
    value: bool,
    msg: fn(bool) -> S,
) -> Element<'a, Message> {
    row![
        column![label_w(title, metrics::font_md(), t.foreground, Weight::Medium), small_text(hint, t.muted)]
            .spacing(2)
            .width(Length::Fill),
        toggler(value).on_toggle(move |v| Message::Servers(msg(v))).size(20),
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::Center)
    .into()
}

/// Option toggles for `kind`: legacy auth (Subsonic), ignore SSL errors, prefer instant mix (Jellyfin).
pub fn option_rows<'a>(
    t: Tokens,
    kind: SourceKind,
    options: ServerOptions,
    msgs: OptionMsgs,
) -> Vec<Element<'a, Message>> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    match kind {
        SourceKind::Subsonic => rows.push(toggle(
            t,
            "Legacy authentication",
            "Send the password (hex-encoded) instead of a token and salt, for old servers that need it.",
            options.legacy_auth,
            msgs.legacy,
        )),
        SourceKind::Jellyfin => rows.push(toggle(
            t,
            "Prefer instant mix",
            "Build radio from Jellyfin's instant mix instead of asking for similar songs first.",
            options.prefer_instant_mix,
            msgs.mix,
        )),
        _ => return rows,
    }
    rows.push(toggle(
        t,
        "Ignore certificate errors",
        "Accept an invalid or self-signed certificate from this server only.",
        options.ignore_ssl,
        msgs.ssl,
    ));
    if options.ignore_ssl {
        rows.push(small_text(
            "Warning: the server's identity is not checked, so someone on the network could read your \
             password. Use this only for a server you run yourself.",
            t.error,
        ));
    }
    rows
}

/// "Found on your network": Jellyfin servers that answered the broadcast.
pub fn discovered<'a>(state: &'a Listen) -> Option<Element<'a, Message>> {
    let t = state.tokens;
    let known: Vec<&str> = state.config.servers.iter().map(|s| s.url.as_str()).collect();
    let found: Vec<_> = state
        .servers
        .discovered
        .iter()
        .enumerate()
        .filter(|(_, f)| !known.contains(&f.url.as_str()))
        .collect();
    if found.is_empty() && !state.servers.discovering {
        return None;
    }
    let status = if state.servers.discovering { "Looking…" } else { "Scan again" };
    let scan = button(label_w(status, metrics::font_sm(), t.foreground, Weight::Semibold))
        .padding(Padding::from([4.0, 10.0]))
        .style(style::default_button(t))
        .on_press_maybe((!state.servers.discovering).then_some(Message::Servers(S::Discover)));
    let mut list = column![
        row![
            label_w("Found on your network", metrics::font_sm(), t.muted, Weight::Semibold),
            container(scan).width(Length::Fill).align_x(iced::alignment::Horizontal::Right),
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(metrics::space_sm());
    for (i, f) in found {
        list = list.push(
            row![
                column![
                    label_w(super::ellipsize(&f.name, 36), metrics::font_md(), t.foreground, Weight::Medium),
                    label(super::ellipsize(&f.url, 48), metrics::font_xs(), t.muted),
                ]
                .spacing(2)
                .width(Length::Fill),
                label("Jellyfin", metrics::font_xs(), t.muted),
                button(label_w("Use", metrics::font_sm(), iced::Color::WHITE, Weight::Semibold))
                    .padding(Padding::from([4.0, 12.0]))
                    .style(style::primary_button(t))
                    .on_press(Message::Servers(S::UseDiscovered(i))),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
    }
    Some(
        container(list)
            .padding(Padding::from([metrics::space_md(), 32.0]))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.surface)),
                border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
                ..Default::default()
            })
            .into(),
    )
}

/// Jellyfin Quick Connect: the button, then the code while waiting.
pub fn quick_connect<'a>(state: &'a Listen) -> Element<'a, Message> {
    let t = state.tokens;
    let wide = |title: &'static str, primary: bool, msg: Option<Message>| {
        let color = if primary { iced::Color::WHITE } else { t.foreground };
        let b = button(
            container(label_w(title, metrics::font_md(), color, Weight::Semibold)).center_x(Length::Fill),
        )
        .width(Length::Fill)
        .height(metrics::CONTROL)
        .padding(Padding::from([9.0, 18.0]))
        .on_press_maybe(msg);
        if primary { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) }
    };
    let Some(qc) = &state.servers.quick_connect else {
        return wide(
            "Sign in with Quick Connect",
            false,
            (!state.login.busy).then_some(Message::Servers(S::QuickConnectStart)),
        )
        .into();
    };
    let Some(code) = &qc.code else {
        return label("Asking the server for a code…", metrics::font_sm(), t.muted).into();
    };
    column![
        small_text("Open Jellyfin on a signed-in device, go to Quick Connect and enter this code:", t.muted),
        container(label_w(code, 28.0, t.foreground, Weight::Bold)).center_x(Length::Fill),
        wide("Open the approval page", false, Some(Message::Servers(S::OpenQuickConnectPage))),
        label("Waiting for approval…", metrics::font_sm(), t.muted),
        wide("Cancel Quick Connect", false, Some(Message::Servers(S::QuickConnectCancel))),
    ]
    .spacing(metrics::space_sm())
    .into()
}

fn field<'a>(
    t: Tokens,
    placeholder: &'static str,
    value: &str,
    on: fn(String) -> S,
    secure: bool,
) -> Element<'a, Message> {
    text_input(placeholder, value)
        .font(crate::ui::font_for(value, crate::appearance::current_font()))
        .font(crate::appearance::current_font())
        .size(metrics::font_md())
        .padding(Padding::from([9.0, 12.0]))
        .style(style::input(t))
        .secure(secure)
        .on_input(move |v| Message::Servers(on(v)))
        .on_submit(Message::Servers(S::EditSave))
        .into()
}

fn captioned<'a>(t: Tokens, caption: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    column![label_w(caption, metrics::font_sm(), t.muted, Weight::Medium), input]
        .spacing(metrics::space_xs())
        .into()
}

/// The edit-server dialog (`Modal::Custom(EDIT_MODAL)`).
pub fn edit_modal(state: &Listen) -> Option<ModalParts<'_>> {
    let t = state.tokens;
    let form: &EditForm = state.servers.edit.as_ref()?;
    let mut body: Column<'_, Message> =
        column![captioned(t, "Name", field(t, "name", &form.name, S::EditName, false))]
            .spacing(metrics::space_md())
            .width(Length::Fill);
    match form.kind {
        SourceKind::YouTubeMusic => {
            body = body.push(small_text(
                "Listen reads the browser's YouTube sign-in again on every start. Pick another channel, or \
                 read the cookies again now.",
                t.muted,
            ));
            if form.ytm_channels.is_empty() {
                body = body.push(
                    button(label_w(
                        if form.busy { "Looking…" } else { "Change channel…" },
                        metrics::font_md(),
                        t.foreground,
                        Weight::Semibold,
                    ))
                    .padding(Padding::from([8.0, 16.0]))
                    .style(style::default_button(t))
                    .on_press_maybe((!form.busy).then_some(Message::Servers(S::EditYtmLook))),
                );
            } else {
                if let Some(current) = &form.ytm_profile {
                    body = body.push(label_w(
                        format!("Channels signed in with {}:", current.label()),
                        metrics::font_sm(),
                        t.muted,
                        Weight::Normal,
                    ));
                }
                for (i, channel) in form.ytm_channels.iter().enumerate() {
                    body = body.push(
                        button(
                            container(label_w(
                                channel.label(),
                                metrics::font_md(),
                                t.foreground,
                                Weight::Semibold,
                            ))
                            .center_x(Length::Fill),
                        )
                        .width(Length::Fill)
                        .padding(Padding::from([8.0, 16.0]))
                        .style(style::default_button(t))
                        .on_press(Message::Servers(S::EditYtmPick(i))),
                    );
                }
                // Another signed-in browser may hold a different Google account.
                for (i, other) in form
                    .ytm_browsers
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| Some(*p) != form.ytm_profile.as_ref())
                {
                    body = body.push(
                        button(label_w(
                            format!("Use {} instead", other.label()),
                            metrics::font_md(),
                            t.foreground,
                            Weight::Semibold,
                        ))
                        .padding(Padding::from([8.0, 16.0]))
                        .style(style::default_button(t))
                        .on_press_maybe((!form.busy).then_some(Message::Servers(S::EditYtmBrowser(i)))),
                    );
                }
            }
            body = body.push(
                button(label_w(
                    "Reconnect (read the cookies again)",
                    metrics::font_md(),
                    t.foreground,
                    Weight::Semibold,
                ))
                .padding(Padding::from([8.0, 16.0]))
                .style(style::default_button(t))
                .on_press(Message::Servers(S::Reconnect(form.id.clone()))),
            );
        }
        SourceKind::Local => {
            body = body.push(captioned(t, "Music folder", field(t, "~/Music", &form.url, S::EditUrl, false)));
        }
        kind => {
            body = body
                .push(captioned(
                    t,
                    "Server URL",
                    field(t, "https://music.example.com", &form.url, S::EditUrl, false),
                ))
                .push(captioned(t, "Username", field(t, "username", &form.username, S::EditUsername, false)))
                .push(captioned(
                    t,
                    "Password",
                    field(t, "leave empty to keep the saved one", &form.password, S::EditPassword, true),
                ));
            for r in option_rows(t, kind, form.options, EDIT) {
                body = body.push(r);
            }
            if kind == SourceKind::Subsonic {
                body = body
                    .push(small_text(
                        "Show in file manager: where this server's music folder is mounted on this \
                         computer (for example your NAS). Leave empty to hide it for this server.",
                        t.muted,
                    ))
                    .push(captioned(
                        t,
                        "Server music folder (optional)",
                        field(t, "/music", &form.music_server, S::EditMusicServer, false),
                    ))
                    .push(captioned(
                        t,
                        "Local music folder",
                        field(t, "/mnt/nas/Music", &form.music_local, S::EditMusicLocal, false),
                    ));
            }
        }
    }
    if let Some(e) = &form.error {
        body = body.push(
            crate::ui::t_text(e.clone())
                .font(crate::appearance::current_font())
                .size(metrics::font_sm())
                .color(t.error),
        );
    }
    let cancel = button(label("Cancel", metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal));
    let save =
        button(label(if form.busy { "Checking…" } else { "Save" }, metrics::font_md(), iced::Color::WHITE))
            .padding(Padding::from([6.0, 16.0]))
            .style(style::primary_button(t))
            .on_press_maybe((!form.busy).then_some(Message::Servers(S::EditSave)));
    Some(("Edit server", scrollable(body).into(), vec![cancel.into(), save.into()]))
}

/// The page shown when the server to browse needs attention.
pub fn action_required<'a>(state: &'a Listen, failure: &'a Failure) -> Element<'a, Message> {
    let t = state.tokens;
    let name = state
        .config
        .servers
        .iter()
        .find(|s| s.id == *failure.id())
        .map_or_else(|| "this server".to_string(), |s| s.name.clone());
    let (title, message) = failure_text(failure, &name);
    let wide = |title: &'static str, primary: bool, msg: Message| {
        let color = if primary { iced::Color::WHITE } else { t.foreground };
        let b = button(
            container(label_w(title, metrics::font_md(), color, Weight::Semibold)).center_x(Length::Fill),
        )
        .width(Length::Fill)
        .height(metrics::CONTROL)
        .padding(Padding::from([9.0, 18.0]))
        .on_press(msg);
        if primary { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) }
    };
    let id = failure.id().clone();
    let mut card = column![
        row![icon(Icon::LuServer, 22.0, t.error), label_w(title, 22.0, t.foreground, Weight::Bold)]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
        crate::ui::t_text(message)
            .font(crate::appearance::current_font())
            .size(metrics::font_sm())
            .color(t.muted),
    ]
    .spacing(metrics::space_lg());
    card = match failure {
        Failure::Credentials { .. } => card
            .push(wide("Enter the password", true, Message::Servers(S::Edit(id.clone()))))
            .push(wide("Try again", false, Message::Servers(S::Retry(id.clone())))),
        Failure::Unreachable { .. } => card
            .push(wide("Try again", true, Message::Servers(S::Retry(id.clone()))))
            .push(wide("Edit server", false, Message::Servers(S::Edit(id.clone())))),
    };
    card = card.push(wide("Add a server", false, Message::AddServer));
    if let Some(offline) = super::login::offline_button(state) {
        card = card.push(offline);
    }
    let card = container(card).width(460).padding(32).style(move |_| container::Style {
        background: Some(iced::Background::Color(t.background_alt)),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        ..Default::default()
    });
    let mut page = column![card].spacing(metrics::space_lg()).width(460);
    let others: Vec<_> = state.config.servers.iter().filter(|s| s.id != id).collect();
    if !others.is_empty() {
        let mut list = column![label_w("Your other servers", metrics::font_sm(), t.muted, Weight::Semibold)]
            .spacing(metrics::space_sm());
        for s in others {
            list = list.push(
                row![
                    column![
                        label_w(
                            super::ellipsize(&s.name, 40),
                            metrics::font_md(),
                            t.foreground,
                            Weight::Medium
                        ),
                        label(s.kind.label(), metrics::font_xs(), t.muted),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(label_w("Use", metrics::font_sm(), t.foreground, Weight::Semibold))
                        .padding(Padding::from([4.0, 10.0]))
                        .style(style::default_button(t))
                        .on_press(Message::SwitchServer(s.id.clone())),
                    super::server_remove(state, &s.id),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center),
            );
        }
        page = page.push(container(list).padding(Padding::from([0.0, 32.0])));
    }
    page = page.push(
        container(
            row![
                super::server_remove(state, &id),
                label(
                    if state.confirm_remove.as_ref() == Some(&id) { "" } else { "Remove this server" },
                    metrics::font_xs(),
                    t.muted
                ),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0.0, 32.0])),
    );
    container(scrollable(container(page).center_x(Length::Fill).padding(Padding::from([32.0, 0.0]))))
        .center(Length::Fill)
        .into()
}
