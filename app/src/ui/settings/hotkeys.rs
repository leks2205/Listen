//! Hotkeys tab: the application hotkey editor (one row per Feishin action) and the
//! system-wide options.

use super::{label_w, line, section, switch_with};
use crate::app::hotkeys::HotkeysMsg;
use crate::app::{Listen, Message};
use crate::hotkeys::{duplicates, is_duplicate};
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{button, checkbox, container, row};
use iced::{Alignment, Border, Element, Length};

/// The whole tab: application hotkeys and the system-wide options.
pub fn global_shortcuts(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings;
    let linux = cfg!(target_os = "linux");
    let dups = duplicates(&s.hotkeys);

    let mut rows: Vec<Element<'_, Message>> = vec![
        crate::ui::t_text(
            "Click a field and press the keys to record a hotkey. Esc cancels, the cross clears it.",
        )
        .font(crate::appearance::current_font())
        .size(metrics::font_sm())
        .color(t.muted)
        .into(),
    ];
    if linux {
        rows.push(
            crate::ui::t_text(
                "A checked \"global\" action works from any window through the desktop's shortcut portal \
                 (KDE Plasma, GNOME) instead of only inside Listen. The desktop has the final say on the key.",
            )
            .font(crate::appearance::current_font())
            .size(metrics::font_sm())
            .color(t.muted)
            .into(),
        );
        if let Some(e) = &state.global_error {
            rows.push(
                crate::ui::t_text(format!("The desktop refused system-wide shortcuts: {e}"))
                    .font(crate::appearance::current_font())
                    .size(metrics::font_sm())
                    .color(t.error)
                    .into(),
            );
        }
    }
    for a in listen_core::settings::HOTKEY_ACTIONS {
        let hotkey = s.hotkeys.get(a.id).map_or("", String::as_str);
        let recording = state.hotkey_recording.as_deref() == Some(a.id);
        let duplicate = !recording && is_duplicate(&dups, hotkey);
        let shown = if recording { "Press keys\u{2026}" } else { hotkey };
        let field = button(
            crate::ui::t_text(shown.to_string())
                .font(crate::appearance::current_font())
                .size(metrics::font_md())
                .color(if hotkey.is_empty() && !recording { t.muted } else { t.foreground }),
        )
        .width(220)
        .padding([6, 10])
        .on_press(Message::Hotkeys(HotkeysMsg::Record(a.id.to_string())))
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(t.surface)),
            text_color: t.foreground,
            border: Border {
                color: if duplicate {
                    t.error
                } else if recording {
                    t.primary
                } else {
                    t.border
                },
                width: 1.0,
                radius: metrics::radius_sm().into(),
            },
            ..Default::default()
        });
        let mut r = row![
            label_w(a.label, metrics::font_md(), t.foreground, Weight::Medium).width(Length::Fill),
            field
        ]
        .spacing(metrics::space_md())
        .align_y(Alignment::Center);
        if linux {
            let global = s.hotkeys_global.contains(a.id);
            let mut cb = checkbox(global).size(18);
            if !hotkey.is_empty() {
                let id = a.id.to_string();
                cb = cb.on_toggle(move |on| Message::Hotkeys(HotkeysMsg::SetGlobal(id.clone(), on)));
            }
            r = r.push(container(if a.allow_global { Some(cb) } else { None }).width(28));
        }
        let clear: Element<'_, Message> = if hotkey.is_empty() {
            iced::widget::Space::new().width(28).into()
        } else {
            crate::ui::tip::tip(
                state,
                button(
                    crate::ui::t_text("\u{2715}")
                        .font(crate::appearance::current_font())
                        .size(metrics::font_md())
                        .color(t.error),
                )
                .width(28)
                .padding([4, 0])
                .on_press(Message::Hotkeys(HotkeysMsg::Clear(a.id.to_string())))
                .style(style::bare_button(t)),
                "Clear the hotkey",
                None,
                crate::ui::tip::Position::Left,
            )
        };
        rows.push(r.push(clear).into());
    }
    let mut out = vec![section(state, "Application hotkeys", rows)];

    if linux {
        let mut sys = vec![line(
            state,
            "Global media hotkeys",
            "Let Listen grab the media keys through the desktop portal. Off: only MPRIS (your desktop's own media-key handling) controls Listen. On KDE, GNOME, Cinnamon and Budgie the desktop already sends the keys over MPRIS, so Listen does not bind them again (that would fire twice).",
            switch_with(s.global_media_keys, |on| Message::Hotkeys(HotkeysMsg::MediaKeys(on))),
        )];
        if !state.global_triggers.is_empty() {
            let active = state
                .global_triggers
                .iter()
                .map(|(id, trigger)| format!("{}: {trigger}", shortcut_label(id)))
                .collect::<Vec<_>>()
                .join("   ");
            sys.push(
                crate::ui::t_text(format!("Active in the desktop: {active}"))
                    .font(crate::appearance::current_font())
                    .size(metrics::font_sm())
                    .color(t.muted)
                    .into(),
            );
        }
        sys.push(
            crate::ui::t_text("To change a key the desktop already bound, open System Settings \u{2192} Shortcuts \u{2192} Listen.")
                .font(crate::appearance::current_font())
                .size(metrics::font_sm())
                .color(t.muted)
                .into(),
        );
        out.push(section(state, "System-wide", sys));
    }
    iced::widget::Column::with_children(out).spacing(metrics::space_xl()).into()
}

#[cfg(target_os = "linux")]
fn shortcut_label(id: &str) -> String {
    crate::global_shortcuts::MEDIA_KEYS
        .iter()
        .find(|(m, ..)| *m == id)
        .map(|(_, label, ..)| (*label).to_string())
        .or_else(|| listen_core::settings::hotkey_action(id).map(|a| a.label.to_string()))
        .unwrap_or_else(|| id.to_string())
}

#[cfg(not(target_os = "linux"))]
fn shortcut_label(id: &str) -> String {
    id.to_string()
}
