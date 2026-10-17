//! The command palette panel (`app/palette.rs`), shown by the overlay modal.

use super::{cover, ellipsize, icon, label, label_w};
use crate::app::palette::{Cmd, INPUT_ID, PaletteMsg, Row as PRow};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, space, text_input};
use iced::{Alignment, Color, Element, Length, Padding};

const WIDTH: f32 = 640.0;
const LIST_HEIGHT: f32 = 380.0;

fn key_hint<'a>(state: &Listen, text: &'static str) -> Element<'a, Message> {
    let t = state.tokens;
    container(label_w(text, metrics::font_xs(), t.muted, Weight::Semibold))
        .padding(Padding::from([2.0, 6.0]))
        .style(move |_| container::Style {
            background: Some(t.surface.into()),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_sm().into() },
            ..Default::default()
        })
        .into()
}

pub fn panel(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let p = &state.palette;

    let mut input = row![
        icon(Icon::LuSearch, metrics::font_lg(), t.muted),
        text_input(crate::i18n::tr("Search"), &p.query)
            .id(iced::widget::Id::new(INPUT_ID))
            .font(super::font_for(&p.query, crate::appearance::current_font()))
            .size(metrics::font_md())
            .padding(Padding::from([8.0, 10.0]))
            .style(style::input(t))
            .on_input(|s| Message::Palette(PaletteMsg::Input(s)))
            .on_submit(Message::Palette(PaletteMsg::Activate)),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    if !p.query.is_empty() {
        input = input.push(crate::ui::tip::tip(
            state,
            button(icon(Icon::LuX, metrics::font_md(), t.muted))
                .padding(4)
                .style(style::icon_button(t, false))
                .on_press(Message::Palette(PaletteMsg::Clear)),
            "Clear",
            None,
            crate::ui::tip::Position::Bottom,
        ));
    }

    let mut list = Column::new().spacing(2);
    let mut at = 0usize;
    for r in state.palette_rows() {
        match r {
            PRow::Heading(group, count) => {
                let mut head = row![].spacing(metrics::space_sm()).align_y(Alignment::Center);
                if group.collapsible() {
                    let folded = p.collapsed.contains(&group);
                    head = head.push(icon(
                        if folded { Icon::LuChevronRight } else { Icon::LuChevronDown },
                        metrics::font_md(),
                        t.muted,
                    ));
                }
                head = head.push(label_w(group.title(), metrics::font_sm(), t.muted, Weight::Semibold));
                if let Some(n) = count {
                    head = head.push(label(format!("{n}"), metrics::font_sm(), t.muted));
                }
                if group.collapsible() {
                    head = head.push(space::horizontal()).push(
                        button(label_w("View all", metrics::font_xs(), t.foreground, Weight::Medium))
                            .padding(Padding::from([2.0, 8.0]))
                            .style(style::default_button(t))
                            .on_press(Message::Palette(PaletteMsg::ViewAll(group))),
                    );
                    list = list.push(
                        button(head)
                            .width(Length::Fill)
                            .padding(Padding::from([6.0, 8.0]))
                            .style(style::row_button(t, false))
                            .on_press(Message::Palette(PaletteMsg::Toggle(group))),
                    );
                } else {
                    list = list.push(container(head).padding(Padding {
                        top: 8.0,
                        right: 8.0,
                        bottom: 2.0,
                        left: 8.0,
                    }));
                }
            }
            PRow::Item(item) => {
                let selected = at == p.selected;
                let lead: Option<Element<'_, Message>> = match &item.cmd {
                    Cmd::Album(a) => {
                        Some(cover(state, a.image.as_ref(), 36.0, metrics::radius_sm(), Icon::LuDisc3))
                    }
                    Cmd::Artist(a) => Some(cover(state, a.image.as_ref(), 36.0, 18.0, Icon::LuUser)),
                    Cmd::Song(s) => {
                        Some(cover(state, s.image.as_ref(), 36.0, metrics::radius_sm(), Icon::LuMusic))
                    }
                    Cmd::Playlist(x) => {
                        Some(cover(state, x.image.as_ref(), 36.0, metrics::radius_sm(), Icon::LuListMusic))
                    }
                    _ => None,
                };
                let mut text = column![label_w(
                    ellipsize(&item.label, 70),
                    metrics::font_md(),
                    t.foreground,
                    Weight::Medium
                )];
                if !item.sub.is_empty() {
                    text = text.push(label(ellipsize(&item.sub, 80), metrics::font_sm(), t.muted));
                }
                let mut content = row![].spacing(metrics::space_md()).align_y(Alignment::Center);
                if let Some(l) = lead {
                    content = content.push(l);
                }
                content = content.push(text);
                list = list.push(
                    button(content)
                        .width(Length::Fill)
                        .padding(Padding::from([6.0, 8.0]))
                        .style(move |theme, status| {
                            let mut s = style::row_button(t, false)(theme, status);
                            if selected {
                                s.background = Some(style::alpha(t.primary, 0.25).into());
                            }
                            s
                        })
                        .on_press(Message::Palette(PaletteMsg::Choose(at))),
                );
                at += 1;
            }
        }
    }
    if at == 0 {
        list = list.push(container(label("No results", metrics::font_md(), t.muted)).padding(12));
    }
    if p.loading {
        list = list.push(
            container(label("Searching…", metrics::font_sm(), t.muted)).padding(Padding::from([4.0, 8.0])),
        );
    }

    let mut crumbs = row![].spacing(2).align_y(Alignment::Center);
    for (i, page) in p.pages.iter().enumerate() {
        if i > 0 {
            crumbs = crumbs.push(icon(Icon::LuChevronRight, metrics::font_sm(), t.muted));
        }
        crumbs = crumbs.push(
            button(label_w(page.title().to_uppercase(), metrics::font_xs(), t.foreground, Weight::Semibold))
                .padding(Padding::from([2.0, 6.0]))
                .style(style::icon_button(t, i + 1 == p.pages.len()))
                .on_press(Message::Palette(PaletteMsg::Crumb(i))),
        );
    }
    let footer = row![
        crumbs,
        space::horizontal(),
        row![key_hint(state, "ESC"), key_hint(state, "↑"), key_hint(state, "↓"), key_hint(state, "⏎")]
            .spacing(metrics::space_xs()),
    ]
    .align_y(Alignment::Center);

    container(
        column![
            input,
            super::divider_h(t),
            scrollable(list).style(style::scrollbar(t)).height(Length::Fixed(LIST_HEIGHT)),
            super::divider_h(t),
            footer,
        ]
        .spacing(metrics::space_sm()),
    )
    .width(WIDTH)
    .padding(metrics::space_md())
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
