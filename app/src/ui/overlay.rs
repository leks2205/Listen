//! Toasts (stacked bottom-right above the player bar), the generic modal and
//! the confirm dialog (centred panel over a dimmed backdrop).

use super::{icon, label, label_w};
use crate::app::overlay::{Confirm, Modal, OverlayMsg, Toast, ToastKind};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, Tokens, metrics};
use iced::font::Weight;
use iced::widget::{button, column, container, mouse_area, opaque, row, space, stack};
use iced::{Alignment, Color, Element, Length, Padding};

const TOAST_WIDTH: f32 = 340.0;
const MODAL_WIDTH: f32 = 440.0;

fn accent(t: Tokens, kind: ToastKind) -> Color {
    match kind {
        ToastKind::Success => t.success,
        ToastKind::Error => t.error,
        ToastKind::Info => t.primary_shade,
        ToastKind::Warning => Color::from_rgb8(0xfa, 0xb0, 0x05),
    }
}

/// Feishin's default toast title: the kind's name.
fn kind_title(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Success => "Success",
        ToastKind::Warning => "Warning",
        ToastKind::Error => "Error",
        ToastKind::Info => "Info",
    }
}

fn toast_card<'a>(t: Tokens, toast: &'a Toast, width: f32) -> Element<'a, Message> {
    let color = accent(t, toast.kind);
    let mut text = column![].spacing(2).width(Length::Fill);
    let title = toast.title.as_deref().unwrap_or_else(|| kind_title(toast.kind));
    text = text.push(label_w(title, metrics::font_md(), t.foreground, Weight::Semibold));
    text = text.push(
        super::text_in(&toast.message, super::font(Weight::Normal)).size(metrics::font_sm()).color(t.muted),
    );
    if let Some((label, msg)) = &toast.action {
        text = text.push(
            button(label_w(label, metrics::font_sm(), t.foreground, Weight::Semibold))
                .padding(Padding::from([4.0, 12.0]))
                .style(style::default_button(t))
                .on_press((**msg).clone()),
        );
    }
    let close = super::tip::tip_plain(
        t,
        button(icon(Icon::LuX, metrics::font_md(), t.muted))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(Message::Overlay(OverlayMsg::CloseToast(toast.id))),
        "Close",
        super::tip::Position::Left,
    );
    let accent_bar = container(space().width(4.0).height(Length::Fill))
        .style(move |_| container::Style { background: Some(color.into()), ..Default::default() });
    let id = toast.id;
    mouse_area(
        container(
            row![
                accent_bar,
                container(row![text, close].spacing(metrics::space_sm()).align_y(Alignment::Start))
                    .padding(Padding::from([metrics::space_sm(), metrics::space_md()]))
            ]
            .height(Length::Shrink),
        )
        .width(width)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(t.surface.into()),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_sm().into() },
            shadow: iced::Shadow {
                color: Color { a: 0.35, ..Color::BLACK },
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        }),
    )
    .on_enter(Message::Overlay(OverlayMsg::Hover(id, true)))
    .on_exit(Message::Overlay(OverlayMsg::Hover(id, false)))
    .into()
}

/// The toast stack, or `None` without toasts.
pub fn toasts(state: &Listen) -> Option<Element<'_, Message>> {
    if state.overlay.toasts.is_empty() {
        return None;
    }
    let t = state.tokens;
    // Narrow windows (compact layout) get a narrower toast.
    let width = TOAST_WIDTH.min((state.window_size.width - 2.0 * metrics::space_lg()).max(200.0));
    let mut col = column![].spacing(metrics::space_sm()).align_x(iced::alignment::Horizontal::Right);
    for toast in &state.overlay.toasts {
        col = col.push(toast_card(t, toast, width));
    }
    Some(
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(Padding {
                right: metrics::space_lg(),
                bottom: metrics::PLAYER_BAR_HEIGHT + metrics::space_lg(),
                ..Padding::ZERO
            })
            .into(),
    )
}

/// A modal panel: title, `body` and a right-aligned row of `actions`.
pub fn modal_panel<'a>(
    t: Tokens,
    title: &'a str,
    body: Element<'a, Message>,
    actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    modal_panel_sized(t, title, body, actions, MODAL_WIDTH)
}

/// [`modal_panel`] with its own width.
pub fn modal_panel_sized<'a>(
    t: Tokens,
    title: &'a str,
    body: Element<'a, Message>,
    actions: Vec<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    let mut buttons = row![space::horizontal()].spacing(metrics::space_sm()).align_y(Alignment::Center);
    for a in actions {
        buttons = buttons.push(a);
    }
    container(
        column![label_w(title, metrics::font_lg(), t.foreground, Weight::Semibold), body, buttons]
            .spacing(metrics::space_lg()),
    )
    .width(width)
    .padding(metrics::space_lg())
    .style(move |_| container::Style {
        background: Some(t.background_alt.into()),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        shadow: iced::Shadow {
            color: Color { a: 0.5, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    })
    .into()
}

fn confirm_panel<'a>(t: Tokens, c: &'a Confirm) -> Element<'a, Message> {
    let cancel = button(label(&c.cancel_label, metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Overlay(OverlayMsg::CloseModal));
    let danger = c.danger;
    let confirm = button(label(&c.confirm_label, metrics::font_md(), Color::WHITE))
        .padding(Padding::from([6.0, 16.0]))
        .style(move |theme, status| {
            let mut s = style::primary_button(t)(theme, status);
            if danger {
                s.background = Some(t.error.into());
            }
            s
        })
        .on_press(Message::Overlay(OverlayMsg::Confirmed));
    modal_panel(
        t,
        &c.title,
        super::text_in(&c.text, super::font(Weight::Normal)).size(metrics::font_md()).color(t.muted).into(),
        vec![cancel.into(), confirm.into()],
    )
}

/// Title, body and actions of a custom modal.
pub type ModalParts<'a> = (&'a str, Element<'a, Message>, Vec<Element<'a, Message>>);

/// Body of a `Modal::Custom(key)`: (title, body, actions). Features add an arm.
fn custom_modal<'a>(state: &'a Listen, key: &str) -> Option<ModalParts<'a>> {
    match key {
        "list-filters" => super::list::header::filters_modal(state),
        crate::app::fullscreen_state::SETTINGS_MODAL => super::fullscreen::settings::modal(state),
        crate::app::playlists::FILTERS_MODAL => super::playlist_page::filters::modal(state),
        crate::app::playlists::FORM_MODAL => super::playlist_page::forms::modal(state),
        crate::app::viz::MODAL => super::visualizer::settings_modal(state),
        crate::app::radio::FORM_MODAL => super::radio::form_modal(state),
        crate::app::servers::EDIT_MODAL => super::servers::edit_modal(state),
        crate::app::updater::MODAL => super::update_dialog::modal(state),
        crate::app::helpers::MODAL => super::onboarding::helpers_modal(state),
        _ => None,
    }
}

/// `panel` centred over a dimmed backdrop; clicking the backdrop sends `close`.
pub fn modal_over<'a>(
    root: Element<'a, Message>,
    panel: Element<'a, Message>,
    close: Message,
) -> Element<'a, Message> {
    let backdrop = mouse_area(
        container(space().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color { a: 0.55, ..Color::BLACK }.into()),
                ..Default::default()
            }),
    )
    .on_press(close);
    let centred = container(opaque(panel)).width(Length::Fill).height(Length::Fill).center(Length::Fill);
    stack![root, opaque(backdrop), centred].into()
}

/// `root` with the open modal and the toasts on top.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    let mut root = root;
    if let Some(modal) = &state.overlay.modal {
        let close = Message::Overlay(OverlayMsg::CloseModal);
        let panel = match modal {
            Modal::Confirm(c) => Some(confirm_panel(t, c)),
            Modal::Custom(crate::app::palette::MODAL_KEY) => Some(super::palette::panel(state)),
            Modal::Custom(crate::app::smart_playlist::MODAL_KEY) => Some(super::smart_playlist::panel(state)),
            Modal::Custom(key) if super::item::is_modal(key) => super::item::panel(state, key),
            Modal::Custom(key) if super::lyrics::is_modal(key) => super::lyrics::panel(state, key),
            Modal::Custom(key) => {
                custom_modal(state, key).map(|(title, body, actions)| {
                    // The visualizer options need more room than the usual dialog.
                    let width = if *key == crate::app::viz::MODAL {
                        620.0
                    } else if *key == crate::app::updater::MODAL {
                        560.0
                    } else {
                        MODAL_WIDTH
                    };
                    modal_panel_sized(t, title, body, actions, width)
                })
            }
        };
        if let Some(panel) = panel {
            root = modal_over(root, panel, close);
        }
    }
    match toasts(state) {
        Some(toasts) => stack![root, toasts].into(),
        None => root,
    }
}
