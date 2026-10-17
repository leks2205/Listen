//! General tab: the default expiry of new share links (Feishin `shareExpiration`).

use super::{line, section, switch};
use crate::app::item::share::SharingSetting;
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use iced::Alignment;
use iced::Element;
use iced::Padding;
use iced::widget::{pick_list, row, text_input};
use listen_core::settings::ExpiryUnit;

fn change(c: SharingSetting) -> Message {
    Message::Setting(Setting::Sharing(c))
}

pub(super) fn sharing(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let e = &state.config.settings.sharing.expiration;
    let rows = vec![
        line(
            state,
            "Use the server's default expiration",
            "New share links get no expiry date from Listen; the server decides",
            switch(e.use_server_default, |v| Setting::Sharing(SharingSetting::UseServerDefault(v))),
        ),
        line(
            state,
            "Default share expiration",
            "How long a new share link works (you can change it for each share)",
            row![
                text_input(crate::i18n::tr("1"), &e.amount.to_string())
                    .on_input(|v| change(SharingSetting::Amount(v.trim().parse().unwrap_or(1))))
                    .size(metrics::font_md())
                    .padding(Padding::from([6.0, 10.0]))
                    .style(style::input(t))
                    .width(80),
                pick_list(ExpiryUnit::ALL, Some(e.unit), |u| change(SharingSetting::Unit(u))).width(120),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        ),
    ];
    section(state, "Sharing", rows)
}
