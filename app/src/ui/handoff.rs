//! The strip over the player bar while another device plays: what plays there, and "Play here".

use crate::app::handoff::HandoffMsg;
use crate::app::{Listen, Message};
use crate::i18n::{tr, trf};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{ellipsize_px, icon, label, label_w};
use iced::font::Weight;
use iced::widget::{button, container, row, space};
use iced::{Alignment, Element, Length, Padding};

pub fn strip(state: &Listen) -> Option<Element<'_, Message>> {
    let t = state.tokens;
    let remote = state.handoff_elsewhere().into_iter().next()?;
    let (title, artists) = remote.title()?;
    let what = if artists.is_empty() { title } else { format!("{title} · {artists}") };
    let text = trf("Playing on {}: {}", &[&remote.label(), &what]);
    let width = (state.window_size.width - 260.0).max(120.0);
    let play = button(label_w(tr("Play here"), metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(Padding::from([4.0, 12.0]))
        .style(style::default_button(t))
        .on_press(Message::Handoff(HandoffMsg::PlayHere(remote.device.clone())));
    let line = row![
        icon(Icon::LuHeadphones, metrics::font_md(), t.primary),
        label(ellipsize_px(&text, width, metrics::font_md()), metrics::font_md(), t.foreground),
        space().width(Length::Fill),
        play,
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    Some(
        container(line)
            .padding(Padding::from([4.0, metrics::space_md()]))
            .width(Length::Fill)
            .style(style::player_bar(t))
            .into(),
    )
}
