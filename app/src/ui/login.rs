//! Sign-in page: Navidrome (OpenSubsonic), Jellyfin, YouTube Music or a local folder.

use super::{icon, label, label_w};
use crate::app::{Listen, LoginMsg, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, text_input};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::SourceKind;

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let f = &state.login;

    if state.connecting {
        let mut page = column![label("Connecting…", metrics::font_lg(), t.muted)]
            .spacing(metrics::space_lg())
            .align_x(Alignment::Center);
        if let Some(offline) = offline_button(state) {
            page = page.push(offline);
        }
        return container(page).center(Length::Fill).into();
    }

    if let Some(failure) = state.action_required() {
        return super::servers::action_required(state, failure);
    }

    let field = |placeholder: &'static str, value: &str, on: fn(String) -> LoginMsg, secure: bool| {
        text_input(crate::i18n::tr(placeholder), value)
            .font(crate::appearance::current_font())
            .size(metrics::font_md())
            .padding(Padding::from([9.0, 12.0]))
            .style(style::input(t))
            .secure(secure)
            .on_input(move |v| Message::Login(on(v)))
            .on_submit(Message::Login(LoginMsg::Submit))
    };
    let caption = |s: &'static str| label_w(s, metrics::font_sm(), t.muted, Weight::Medium);
    // Full-width button (primary or default style).
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
    let para = |s: &'static str| {
        crate::ui::t_text(s).font(crate::appearance::current_font()).size(metrics::font_sm()).color(t.muted)
    };

    let tab = |kind: SourceKind, title: &'static str| {
        let active = f.kind == kind;
        button(
            container(label_w(
                title,
                metrics::font_md(),
                if active { t.foreground } else { t.muted },
                Weight::Semibold,
            ))
            .center_x(Length::Fill),
        )
        .width(Length::Fill)
        .padding(Padding::from([8.0, 6.0]))
        .style(style::nav_item(t, active))
        .on_press(Message::Login(LoginMsg::Kind(kind)))
    };
    let tabs = row![
        tab(SourceKind::Subsonic, "Navidrome"),
        tab(SourceKind::Jellyfin, "Jellyfin"),
        tab(SourceKind::YouTubeMusic, "YouTube Music"),
        tab(SourceKind::Local, "Local")
    ]
    .spacing(metrics::space_sm());

    let mut form: Column<'_, Message> = match f.kind {
        SourceKind::YouTubeMusic => {
            let mut c = column![
                row![icon(Icon::LuMusic, 22.0, t.primary), label_w("Connect YouTube Music", 22.0, t.foreground, Weight::Bold)]
                    .spacing(metrics::space_md())
                    .align_y(Alignment::Center),
                para(
                    "Listen uses the YouTube sign-in of a browser on this computer (Ecosia, Chrome, Chromium, \
                     Brave, Edge, Vivaldi or Firefox) and reads it again on every start, so it stays signed in \
                     as long as the browser does."
                ),
            ]
            .spacing(metrics::space_lg());
            match &f.browser {
                None => c = c.push(label("Looking for a signed-in browser…", metrics::font_sm(), t.muted)),
                Some(Ok(profile)) if !f.channels.is_empty() => {
                    c = c.push(label_w(
                        format!("Which {} channel has your music?", profile.label()),
                        metrics::font_md(),
                        t.foreground,
                        Weight::Semibold,
                    ));
                    for (i, channel) in f.channels.iter().enumerate() {
                        let (color, style_primary) = if i == 0 { (iced::Color::WHITE, true) } else { (t.foreground, false) };
                        let b = button(container(label_w(channel.label(), metrics::font_md(), color, Weight::Semibold)).center_x(Length::Fill))
                            .width(Length::Fill)
                            .height(metrics::CONTROL)
                            .padding(Padding::from([9.0, 18.0]))
                            .on_press_maybe((!f.busy).then_some(Message::Login(LoginMsg::PickChannel(i))));
                        c = c.push(if style_primary { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) });
                    }
                }
                Some(Ok(profile)) => {
                    let title = if f.busy && !f.show_cookies {
                        "Connecting…".to_string()
                    } else {
                        format!("Sign in with {}", profile.label())
                    };
                    c = c.push(
                        button(
                            container(label_w(title, metrics::font_md(), iced::Color::WHITE, Weight::Semibold))
                                .center_x(Length::Fill),
                        )
                        .width(Length::Fill)
                        .height(metrics::CONTROL)
                        .padding(Padding::from([9.0, 18.0]))
                        .style(style::primary_button(t))
                        .on_press_maybe((!f.busy).then_some(Message::Login(LoginMsg::UseBrowser))),
                    );
                }
                Some(Err(e)) => {
                    c = c
                        .push(crate::ui::t_text(e.clone()).font(crate::appearance::current_font()).size(metrics::font_sm()).color(t.error))
                        .push(wide("Look again", false, Some(Message::Login(LoginMsg::FindBrowser))));
                }
            }
            // Another browser may hold a different Google account (or the right one).
            let current = match &f.browser {
                Some(Ok(p)) => Some(p),
                _ => None,
            };
            let others: Vec<_> = f.browsers.iter().enumerate().filter(|(_, (p, _))| Some(p) != current).collect();
            if !others.is_empty() {
                let chevron = if f.show_browsers { Icon::LuChevronDown } else { Icon::LuChevronRight };
                c = c.push(
                    button(
                        row![
                            icon(chevron, metrics::font_sm(), t.muted),
                            label_w("Use another browser", metrics::font_sm(), t.muted, Weight::Medium)
                        ]
                        .spacing(metrics::space_xs())
                        .align_y(Alignment::Center),
                    )
                    .padding(0)
                    .style(style::bare_button(t))
                    .on_press(Message::Login(LoginMsg::ToggleBrowsers)),
                );
            }
            if f.show_browsers {
                for (i, (profile, usable)) in others {
                    // Several Firefox profiles (or Chrome's "Profile 2") need their folder name.
                    let same_label = f.browsers.iter().filter(|(p, _)| p.label() == profile.label()).count() > 1;
                    let name = if same_label {
                        format!("{} ({})", profile.label(), profile.profile)
                    } else {
                        profile.label()
                    };
                    let status = match usable {
                        Ok(()) => "signed in to YouTube".to_string(),
                        Err(e) => e.clone(),
                    };
                    let status_color = if usable.is_ok() { t.success } else { t.muted };
                    c = c.push(
                        button(
                            column![
                                label_w(name, metrics::font_md(), t.foreground, Weight::Semibold),
                                label(status, metrics::font_sm(), status_color)
                                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                                    .width(Length::Fill),
                            ]
                            .spacing(2.0)
                            .width(Length::Fill),
                        )
                        .width(Length::Fill)
                        .padding(Padding::from([8.0, 14.0]))
                        .style(style::default_button(t))
                        .on_press_maybe((!f.busy).then_some(Message::Login(LoginMsg::OtherBrowser(i)))),
                    );
                }
            }
            let (toggle, chevron) = if f.show_cookies {
                ("Other ways to connect", Icon::LuChevronDown)
            } else {
                ("Other ways to connect (cookies file, guest)", Icon::LuChevronRight)
            };
            c = c.push(
                button(
                    row![icon(chevron, metrics::font_sm(), t.muted), label_w(toggle, metrics::font_sm(), t.muted, Weight::Medium)]
                        .spacing(metrics::space_xs())
                        .align_y(Alignment::Center),
                )
                    .padding(0)
                    .style(style::bare_button(t))
                    .on_press(Message::Login(LoginMsg::ToggleCookies)),
            );
            if f.show_cookies {
                c = c
                    .push(para(
                        "Cookies: export them from music.youtube.com with an extension like “Get cookies.txt LOCALLY” \
                         (JSON or cookies.txt), then choose the file, drop it onto this window or paste it.",
                    ))
                    .push(
                        column![
                            caption("Cookies file"),
                            row![
                                field("~/Downloads/music.youtube.com_cookies.json", &f.url, LoginMsg::Url, false),
                                button(label_w("Browse…", metrics::font_md(), t.foreground, Weight::Semibold))
                                    .height(metrics::CONTROL)
                                    .padding(Padding::from([9.0, 14.0]))
                                    .style(style::default_button(t))
                                    .on_press(Message::Login(LoginMsg::Browse)),
                            ]
                            .spacing(metrics::space_sm()),
                        ]
                        .spacing(metrics::space_xs()),
                    )
                    .push(match &f.info {
                        Some(info) => Element::from(crate::ui::t_text(info.clone()).font(crate::appearance::current_font()).size(metrics::font_sm()).color(t.success)),
                        None => Element::from(field("or paste cookies here", &f.password, LoginMsg::Password, true)),
                    })
                    .push(para("Leave both empty to connect as a guest: search, albums, artists and radio work, but not your library."));
            }
            c.push(para(
                "Unofficial: this uses YouTube's internal API (like ytmusicapi) and plays audio via yt-dlp. \
                 It can break when YouTube changes things.",
            ))
        }
        SourceKind::Local => column![
            row![
                icon(Icon::RiFolder3Line, 22.0, t.primary),
                label_w("Add a music folder", 22.0, t.foreground, Weight::Bold)
            ]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
            para(
                "Listen reads the tags and cover art of the music files in this folder and its subfolders \
                 (MP3, FLAC, Ogg, Opus, M4A, WAV…). Large folders take a moment to scan."
            ),
            column![
                caption("Music folder"),
                row![
                    field("~/Music", &f.url, LoginMsg::Url, false),
                    button(label_w("Browse…", metrics::font_md(), t.foreground, Weight::Semibold))
                        .height(metrics::CONTROL)
                        .padding(Padding::from([9.0, 14.0]))
                        .style(style::default_button(t))
                        .on_press(Message::Login(LoginMsg::BrowseFolder)),
                ]
                .spacing(metrics::space_sm()),
            ]
            .spacing(metrics::space_xs()),
        ],
        SourceKind::Jellyfin => column![
            row![icon(Icon::LuServer, 22.0, t.primary), label_w("Connect to Jellyfin", 22.0, t.foreground, Weight::Bold)]
                .spacing(metrics::space_md())
                .align_y(Alignment::Center),
            para("Your Jellyfin server and account (music libraries only)."),
            column![caption("Server URL"), field("https://jellyfin.example.com", &f.url, LoginMsg::Url, false)]
                .spacing(metrics::space_xs()),
            column![caption("Username"), field("username", &f.username, LoginMsg::Username, false)]
                .spacing(metrics::space_xs()),
            column![caption("Password"), field("password", &f.password, LoginMsg::Password, true)]
                .spacing(metrics::space_xs()),
        ]
        .push(super::servers::quick_connect(state))
        .extend(super::servers::option_rows(t, SourceKind::Jellyfin, f.options, super::servers::LOGIN)),
        _ => column![
            row![icon(Icon::LuServer, 22.0, t.primary), label_w("Connect to Navidrome", 22.0, t.foreground, Weight::Bold)]
                .spacing(metrics::space_md())
                .align_y(Alignment::Center),
            para("Any OpenSubsonic-compatible server works (Navidrome, Gonic, Airsonic…)."),
            column![caption("Server URL"), field("https://music.example.com", &f.url, LoginMsg::Url, false)]
                .spacing(metrics::space_xs()),
            column![caption("Username"), field("username", &f.username, LoginMsg::Username, false)]
                .spacing(metrics::space_xs()),
            column![caption("Password"), field("password", &f.password, LoginMsg::Password, true)]
                .spacing(metrics::space_xs()),
        ]
        .extend(super::servers::option_rows(t, SourceKind::Subsonic, f.options, super::servers::LOGIN)),
    }
    .spacing(metrics::space_lg());

    if let Some(error) = &f.error {
        form = form.push(
            crate::ui::t_text(error.clone())
                .font(crate::appearance::current_font())
                .size(metrics::font_sm())
                .color(t.error),
        );
    }
    let ytm = f.kind == SourceKind::YouTubeMusic;
    if !ytm || f.show_cookies {
        let submit_label = match (f.busy, ytm) {
            (true, _) => "Connecting…",
            (false, true) => "Connect with cookies / as guest",
            (false, false) => "Connect",
        };
        form = form.push(wide(submit_label, !ytm, (!f.busy).then_some(Message::Login(LoginMsg::Submit))));
    }
    if f.kind != SourceKind::Local {
        form =
            form.push(label("Credentials are stored in your system keyring.", metrics::font_xs(), t.muted));
    }
    if state.source.is_some() || state.adding_server {
        form = form.push(
            button(
                container(label_w("Cancel", metrics::font_md(), t.foreground, Weight::Semibold))
                    .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .height(metrics::CONTROL)
            .padding(Padding::from([9.0, 18.0]))
            .style(style::default_button(t))
            .on_press(Message::CancelAddServer),
        );
    }

    let card =
        container(column![tabs, form].spacing(metrics::space_xl())).width(460).padding(32).style(move |_| {
            container::Style {
                background: Some(iced::Background::Color(t.background_alt)),
                border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
                ..Default::default()
            }
        });
    // Saved servers, so a broken one can be removed from here too.
    let mut page = column![].spacing(metrics::space_lg()).width(460);
    if matches!(f.kind, SourceKind::Subsonic | SourceKind::Jellyfin)
        && let Some(found) = super::servers::discovered(state)
    {
        page = page.push(found);
    }
    page = page.push(card);
    if let Some(offline) = offline_button(state) {
        page = page.push(offline);
    }
    if !state.config.servers.is_empty() {
        let mut list = column![label_w("Your servers", metrics::font_sm(), t.muted, Weight::Semibold)]
            .spacing(metrics::space_sm());
        for server in &state.config.servers {
            let connected = state.sources.contains_key(&server.id);
            let status = if connected { server.kind.label() } else { "not connected" };
            let mut entry = row![
                column![
                    label_w(
                        super::ellipsize(&server.name, 40),
                        metrics::font_md(),
                        t.foreground,
                        Weight::Medium
                    ),
                    label(status, metrics::font_xs(), t.muted),
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center);
            if connected {
                entry = entry.push(
                    button(label_w("Open", metrics::font_sm(), t.foreground, Weight::Semibold))
                        .padding(Padding::from([4.0, 10.0]))
                        .style(style::default_button(t))
                        .on_press(Message::SwitchServer(server.id.clone())),
                );
            }
            entry = entry.push(
                button(label_w("Edit", metrics::font_sm(), t.foreground, Weight::Semibold))
                    .padding(Padding::from([4.0, 10.0]))
                    .style(style::default_button(t))
                    .on_press(Message::Servers(crate::app::servers::ServersMsg::Edit(server.id.clone()))),
            );
            list = list.push(entry.push(super::server_remove(state, &server.id)));
        }
        page = page.push(container(list).padding(Padding::from([0.0, 32.0])));
    }
    container(scrollable(container(page).center_x(Length::Fill).padding(Padding::from([32.0, 0.0]))))
        .center(Length::Fill)
        .into()
}

/// "Play downloads" for when no server is reachable; only if something is downloaded.
pub(super) fn offline_button(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    if state.adding_server || state.offline_store().is_none_or(|s| s.entries().is_empty()) {
        return None;
    }
    Some(
        button(
            container(label_w(
                "Play downloaded songs (offline)",
                metrics::font_md(),
                t.foreground,
                Weight::Semibold,
            ))
            .center_x(Length::Fill),
        )
        .width(Length::Fill)
        .height(metrics::CONTROL)
        .padding(Padding::from([9.0, 18.0]))
        .style(style::default_button(t))
        .on_press(Message::Navigate(crate::app::Route::Downloads))
        .into(),
    )
}
