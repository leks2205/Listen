//! The DLNA cast popover (Feishin's `dlna-cast-button.tsx`): "This computer"
//! and the renderers found on the network, a refresh button, and while
//! casting why the DJ features are off.

use super::{icon, label, label_w};
use crate::app::dlna::DlnaMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, Row, button, container, mouse_area, opaque, row, space, stack};
use iced::{Alignment, Background, Element, Length, Padding, Shadow, Vector, border};

pub const WIDTH: f32 = 340.0;

fn act(m: DlnaMsg) -> Message {
    Message::Dlna(m)
}

/// `root` with the popover above the right end of the player bar while it is open.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    if !state.dlna.open || state.needs_login() {
        return stack![root].into();
    }
    let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
        .on_press(act(DlnaMsg::Close))
        .on_right_press(act(DlnaMsg::Close));
    let placed = container(opaque(panel(state)))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(Padding {
            right: metrics::space_lg(),
            bottom: metrics::PLAYER_BAR_HEIGHT + metrics::space_sm(),
            ..Padding::ZERO
        });
    stack![root, backdrop, placed].into()
}

/// One choice: a name, a second line, and a check when it is the one in use.
fn choice<'a>(
    state: &Listen,
    name: String,
    detail: String,
    active: bool,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let color = if active { t.primary } else { t.foreground };
    let mut text = Column::new().spacing(2).width(Length::Fill);
    text = text.push(label_w(name, metrics::font_md(), color, Weight::Medium));
    if !detail.is_empty() {
        text = text.push(label(detail, metrics::font_sm(), t.muted));
    }
    let mut r = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center).push(text);
    if active {
        r = r.push(icon(Icon::LuCheck, metrics::font_lg(), t.primary));
    }
    button(r)
        .width(Length::Fill)
        .padding(Padding::from([8.0, 10.0]))
        .style(style::icon_button(t, active))
        .on_press_maybe(msg)
        .into()
}

pub fn panel(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let d = &state.dlna;
    let casting = state.dlna_casting();

    let mut list = Column::new().spacing(2).width(Length::Fill);
    list = list.push(choice(
        state,
        "This computer".into(),
        String::new(),
        !casting,
        casting.then(|| act(DlnaMsg::Disconnect)),
    ));
    for (i, r) in d.renderers.iter().enumerate() {
        let active = d.cast.as_ref().is_some_and(|c| c.renderer == *r);
        let busy = d.connecting.as_deref() == Some(r.name.as_str());
        let detail = if busy { "Connecting…".to_string() } else { r.model.clone() };
        list = list.push(choice(
            state,
            r.name.clone(),
            detail,
            active,
            (!active && d.connecting.is_none()).then(|| act(DlnaMsg::Connect(i))),
        ));
    }
    if d.renderers.is_empty() {
        let text = if d.scanning {
            "Searching for devices…"
        } else {
            "No devices found. Make sure the speaker or TV is on and on the same network."
        };
        list = list
            .push(container(label(text, metrics::font_sm(), t.muted)).padding(Padding::from([6.0, 10.0])));
    }

    let refresh = button(
        container(
            row![
                icon(Icon::LuRefreshCw, metrics::font_md(), t.foreground),
                label_w(
                    if d.scanning { "Searching…" } else { "Refresh" },
                    metrics::font_md(),
                    t.foreground,
                    Weight::Medium
                ),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        )
        .center_x(Length::Fill),
    )
    .width(Length::Fill)
    .padding(Padding::from([8.0, 12.0]))
    .style(style::default_button(t))
    .on_press_maybe((!d.scanning).then(|| act(DlnaMsg::Scan)));

    let mut content = Column::new().spacing(metrics::space_sm());
    content = content.push(label_w("Cast to a device", metrics::font_md(), t.foreground, Weight::Semibold));
    content = content.push(list);
    content = content.push(refresh);
    if let Some(name) = state.dlna_device() {
        // Why the DJ features sit out, and where the volume went.
        content = content.push(label(
            format!(
                "Casting to {name}. The volume control now sets the device's volume. \
                 DJ transitions and the AI DJ's voice need this computer's audio, so they are off while casting; \
                 the Auto-DJ still adds songs."
            ),
            metrics::font_sm(),
            t.muted,
        ));
    }

    container(content)
        .width(WIDTH)
        .padding(metrics::space_md())
        .style(move |_| container::Style {
            background: Some(Background::Color(t.background_alt)),
            border: border::rounded(metrics::radius_md()).color(t.border).width(1),
            shadow: Shadow {
                color: style::alpha(iced::Color::BLACK, 0.45),
                offset: Vector::new(0.0, 6.0),
                blur_radius: 18.0,
            },
            ..Default::default()
        })
        .into()
}

/// The cast button of the player bar.
pub fn button_view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let casting = state.dlna_casting();
    let active = casting || state.dlna.open;
    let tip = match state.dlna_device() {
        Some(name) => crate::i18n::trf("Casting to {}", &[&name]),
        None => crate::i18n::tr("Cast to a device").to_string(),
    };
    crate::ui::tip::tip_raw(
        state,
        button(
            container(icon(Icon::LuCast, metrics::font_lg(), if active { t.primary } else { t.foreground }))
                .center(Length::Fill),
        )
        .width(metrics::CONTROL)
        .height(metrics::CONTROL)
        .padding(0)
        .style(style::icon_button(t, active))
        .on_press(act(DlnaMsg::Toggle)),
        &tip,
        None,
        crate::ui::tip::Position::Top,
    )
}
