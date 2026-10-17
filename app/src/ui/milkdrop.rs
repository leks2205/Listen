//! The Milkdrop picture: the render thread's last frame with the preset name and a "next preset" button,
//! or the reason there is no picture.

use crate::app::milkdrop::MilkdropMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{button, container, image, row, space, stack};
use iced::{Alignment, ContentFit, Element, Length, Padding};

/// Why there is no picture (yet), as one or two lines.
pub fn status(state: &Listen) -> String {
    let m = &state.milk;
    if let Some(e) = &m.error {
        return e.clone();
    }
    let folder = state.milkdrop_folder().map(|p| p.display().to_string()).unwrap_or_default();
    if m.scanned.is_some() && m.presets.is_empty() {
        return format!("No Milkdrop presets (.milk) in {folder}");
    }
    if m.empty {
        return format!("No Milkdrop presets (.milk) in {folder}");
    }
    "Starting Milkdrop…".into()
}

/// The picture filling its space; `controls` adds the name and the next button.
pub fn surface(state: &Listen, controls: bool) -> Element<'_, Message> {
    let t = state.tokens;
    let Some(frame) = &state.milk.frame else {
        return container(label(status(state), metrics::font_md(), t.muted)).center(Length::Fill).into();
    };
    let picture = image(frame.clone())
        .content_fit(ContentFit::Cover)
        .width(Length::Fill)
        .height(Length::Fill)
        .opacity(state.config.settings.milkdrop.opacity.clamp(0.0, 1.0));
    let picture = container(picture).width(Length::Fill).height(Length::Fill).clip(true);
    if !controls {
        return picture.into();
    }
    let name = state.milk.preset.clone().unwrap_or_default();
    let next = button(icon(Icon::LuSkipForward, 18.0, iced::Color::WHITE))
        .padding(8)
        .style(style::art_overlay_button(t))
        .on_press(Message::Milkdrop(MilkdropMsg::Next));
    let bar = row![
        label(
            crate::ui::ellipsize_px(&name, 520.0, metrics::font_sm()),
            metrics::font_sm(),
            iced::Color::WHITE
        ),
        space::horizontal(),
        crate::ui::tip::tip(state, next, "Next preset", None, iced::widget::tooltip::Position::Top),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    stack![
        picture,
        container(bar)
            .padding(Padding::from([8.0, 12.0]))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Left)
            .align_y(Vertical::Bottom),
    ]
    .into()
}
