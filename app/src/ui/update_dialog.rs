//! The "update available" dialog: version, changelog and the choices
//! Download and install / Remind later / Skip this version.

use crate::app::updater::{InstallMode, Phase, UpdateMsg, Version, detect_install};
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::overlay::ModalParts;
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{button, column, container, scrollable};
use iced::{Element, Length, Padding};

/// Release notes without the Markdown noise (headings, bold, list dashes).
pub fn plain_notes(markdown: &str) -> String {
    let mut out = Vec::new();
    for line in markdown.lines() {
        let l = line.trim_end();
        let l = l.trim_start_matches('#').trim_start();
        let l = if let Some(rest) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            format!("\u{2022} {rest}")
        } else {
            l.to_string()
        };
        out.push(l.replace("**", "").replace('`', ""));
    }
    let text = out.join("\n");
    let text = text.trim();
    if text.is_empty() { "No release notes.".into() } else { text.to_string() }
}

pub fn modal(state: &Listen) -> Option<ModalParts<'_>> {
    let t = state.tokens;
    let u = &state.adv.update;
    let rel = u.available.as_ref()?;
    let button_for = |text: &'static str, msg: Option<UpdateMsg>, primary: bool| -> Element<'_, Message> {
        let b = button(label_w(
            text,
            metrics::font_md(),
            if primary { iced::Color::WHITE } else { t.foreground },
            Weight::Semibold,
        ))
        .padding(Padding::from([8.0, 16.0]))
        .on_press_maybe(msg.map(Message::Update));
        if primary {
            b.style(style::primary_button(t)).into()
        } else {
            b.style(style::default_button(t)).into()
        }
    };
    let mode = detect_install();
    let how = match &mode {
        InstallMode::Rpm => "Installs the RPM with dnf; you are asked for your password.".to_string(),
        InstallMode::Local(path) => format!("Replaces {} and restarts.", path.display()),
        InstallMode::WindowsInstaller => {
            "Runs the new installer in the background and restarts Listen.".to_string()
        }
        InstallMode::Unsupported(why) => why.clone(),
    };
    let status: Element<'_, Message> = match &u.phase {
        Phase::Idle => label(how, metrics::font_sm(), t.muted).into(),
        Phase::Installing => label("Downloading and verifying...", metrics::font_sm(), t.primary).into(),
        Phase::Done(_) => {
            label("Installed. Restart Listen to use the new version.", metrics::font_sm(), t.primary).into()
        }
        Phase::Failed(e) => {
            crate::ui::t_text(format!("Update failed: {e}")).size(metrics::font_sm()).color(t.error).into()
        }
    };
    let notes = container(scrollable(
        crate::ui::t_text(plain_notes(&rel.body)).size(metrics::font_sm()).color(t.foreground),
    ))
    .padding(metrics::space_md())
    .height(Length::Fixed(220.0))
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(t.surface.into()),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        ..Default::default()
    });
    let body = column![
        label(
            format!(
                "Listen {} is available (you have {}).{}",
                rel.version,
                Version::current(),
                if rel.prerelease { " This is a pre-release." } else { "" }
            ),
            metrics::font_md(),
            t.foreground
        ),
        notes,
        status,
    ]
    .spacing(metrics::space_md());
    let can_install = !matches!(mode, InstallMode::Unsupported(_));
    let actions = match &u.phase {
        Phase::Installing => vec![button_for("Installing...", None, true)],
        Phase::Done(_) => vec![
            button_for("Later", Some(UpdateMsg::Dismiss), false),
            button_for("Restart now", Some(UpdateMsg::Restart), true),
        ],
        Phase::Idle | Phase::Failed(_) => vec![
            button_for("Skip this version", Some(UpdateMsg::SkipVersion), false),
            button_for("Remind later", Some(UpdateMsg::RemindLater), false),
            button_for("Download and install", can_install.then_some(UpdateMsg::Install), true),
        ],
    };
    Some(("Update available", body.into(), actions))
}
