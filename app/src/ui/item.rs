//! The modals of `app::item`: "Get info", "Share item" and the tag editor.

use super::{icon, label, label_w};
use crate::app::item::ItemMsg;
use crate::app::item::details::{self, DetailsMsg, DetailsState, Load, RowKind};
use crate::app::item::share::{self, PRESETS, ShareMsg, ShareState};
use crate::app::item::tags::{self, Step, TagsMsg, TagsState};
use crate::app::overlay::OverlayMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, Tokens, metrics};
use iced::font::Weight;
use iced::widget::{
    Column, Row, button, column, container, pick_list, row, scrollable, space, text_editor, text_input,
};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::ExpiryUnit;
use listen_local::tags::{CoverEdit, TagField};

const DETAILS_WIDTH: f32 = 680.0;
const SHARE_WIDTH: f32 = 480.0;
const TAGS_WIDTH: f32 = 640.0;
const LABEL_WIDTH: f32 = 130.0;

/// Whether `key` is a modal of this package.
pub fn is_modal(key: &str) -> bool {
    matches!(key, details::MODAL | share::MODAL | tags::MODAL)
}

/// The panel of an open modal of this package.
pub fn panel<'a>(state: &'a Listen, key: &str) -> Option<Element<'a, Message>> {
    let t = state.tokens;
    let (title, body, actions, width) = match key {
        details::MODAL => {
            let d = state.item.details.as_ref()?;
            let title = d.current()?.subject.name();
            (title, details_body(state, d), vec![close_button(t, "Close")], DETAILS_WIDTH)
        }
        share::MODAL => {
            let s = state.item.share.as_ref()?;
            (
                "Share item",
                share_body(state, s),
                vec![
                    close_button(t, "Cancel"),
                    primary(
                        t,
                        if s.busy { "Sharing…" } else { "Share" },
                        (!s.busy).then_some(sh(ShareMsg::Submit)),
                    ),
                ],
                SHARE_WIDTH,
            )
        }
        tags::MODAL => {
            let s = state.item.tags.as_ref()?;
            (tag_title(s), tags_body(state, s), tag_actions(t, s), TAGS_WIDTH)
        }
        _ => return None,
    };
    Some(super::overlay::modal_panel_sized(t, title, body, actions, width))
}

fn tag_title(s: &TagsState) -> &'static str {
    if s.songs.len() > 1 { "Edit metadata (several songs)" } else { "Edit metadata" }
}

fn close_button<'a>(t: Tokens, text: &'a str) -> Element<'a, Message> {
    button(label(text, metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Overlay(OverlayMsg::CloseModal))
        .into()
}

fn plain<'a>(t: Tokens, text: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    button(label(text, metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press_maybe(msg)
        .into()
}

fn primary<'a>(t: Tokens, text: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    button(label(text, metrics::font_md(), iced::Color::WHITE))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::primary_button(t))
        .on_press_maybe(msg)
        .into()
}

fn small<'a>(t: Tokens, text: &'a str, active: bool, msg: Message) -> Element<'a, Message> {
    button(label(text, metrics::font_sm(), if active { iced::Color::WHITE } else { t.foreground }))
        .padding(Padding::from([4.0, 10.0]))
        .style(
            move |th, st| {
                if active { style::primary_button(t)(th, st) } else { style::default_button(t)(th, st) }
            },
        )
        .on_press(msg)
        .into()
}

fn sh(m: ShareMsg) -> Message {
    Message::Item(ItemMsg::Share(m))
}

fn tg(m: TagsMsg) -> Message {
    Message::Item(ItemMsg::Tags(m))
}

fn dt(m: DetailsMsg) -> Message {
    Message::Item(ItemMsg::Details(m))
}

// --- Get info -------------------------------------------------------------

fn details_body<'a>(state: &'a Listen, d: &'a DetailsState) -> Element<'a, Message> {
    let t = state.tokens;
    let Some(entry) = d.current() else { return space().into() };
    let mut col = Column::new().spacing(metrics::space_sm());
    if d.entries.len() > 1 {
        let last = d.entries.len() - 1;
        col = col.push(
            row![
                super::tip::tip(
                    state,
                    button(icon(Icon::LuChevronLeft, metrics::font_md(), t.foreground))
                        .padding(4)
                        .style(style::icon_button(t, false))
                        .on_press_maybe((d.selected > 0).then(|| dt(DetailsMsg::Select(d.selected - 1)))),
                    "Previous",
                    None,
                    super::tip::Position::Bottom,
                ),
                label(format!("{} of {}", d.selected + 1, d.entries.len()), metrics::font_sm(), t.muted),
                super::tip::tip(
                    state,
                    button(icon(Icon::LuChevronRight, metrics::font_md(), t.foreground))
                        .padding(4)
                        .style(style::icon_button(t, false))
                        .on_press_maybe((d.selected < last).then(|| dt(DetailsMsg::Select(d.selected + 1)))),
                    "Next",
                    None,
                    super::tip::Position::Bottom,
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
    }
    let mut rows = Column::new().spacing(2.0);
    for r in &entry.rows {
        let mut line = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Start);
        line = line.push(container(label(&r.label, metrics::font_sm(), t.muted)).width(LABEL_WIDTH));
        let value = crate::ui::text_in(&r.value, crate::appearance::current_font())
            .size(metrics::font_md())
            .color(t.foreground)
            .width(Length::Fill);
        line = line.push(value);
        if r.kind == RowKind::Path
            && let Some(path) = &entry.local_path
        {
            line = line.push(tool(
                state,
                Icon::LuFolderOpen,
                dt(DetailsMsg::ShowFile(path.clone())),
                "Show in file manager",
            ));
        }
        if let Some(link) = &r.link {
            line = line.push(tool(
                state,
                Icon::LuExternalLink,
                dt(DetailsMsg::OpenLink(link.clone())),
                "Open link",
            ));
        }
        line = line.push(tool(state, Icon::LuCopy, dt(DetailsMsg::Copy(r.value.clone())), "Copy"));
        rows = rows.push(container(line).padding(Padding::from([4.0, 4.0])).width(Length::Fill));
        rows = rows.push(container(space().height(1)).width(Length::Fill).style(style::divider(t)));
    }
    let max = (state.window_size.height - 260.0).max(160.0);
    col = col.push(
        scrollable(container(rows).padding(Padding { right: 12.0, ..Padding::ZERO }))
            .height(Length::Shrink)
            .width(Length::Fill),
    );
    match &entry.load {
        Load::Loading => col = col.push(label("Loading more details…", metrics::font_sm(), t.muted)),
        Load::Failed(e) => {
            col = col.push(label(format!("Server details unavailable: {e}"), metrics::font_sm(), t.muted))
        }
        _ => {}
    }
    container(col).max_height(max).into()
}

fn tool<'a>(state: &Listen, glyph: Icon, msg: Message, name: &str) -> Element<'a, Message> {
    let t = state.tokens;
    super::tip::tip(
        state,
        button(icon(glyph, metrics::font_md(), t.muted))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(msg),
        name,
        None,
        super::tip::Position::Left,
    )
}

// --- Share ----------------------------------------------------------------

fn preset_active(s: &ShareState, preset: Option<(u32, ExpiryUnit)>) -> bool {
    match preset {
        None => s.expiry.use_server_default,
        Some((n, unit)) => !s.expiry.use_server_default && s.amount == n.to_string() && s.expiry.unit == unit,
    }
}

fn share_body<'a>(state: &'a Listen, s: &'a ShareState) -> Element<'a, Message> {
    let t = state.tokens;
    let mut presets = Row::new().spacing(metrics::space_xs());
    for (name, preset) in PRESETS {
        presets = presets.push(small(t, name, preset_active(s, preset), sh(ShareMsg::Preset(preset))));
    }
    let now = std::time::SystemTime::now();
    let summary = match s.expires_at(now) {
        Ok(None) => "The server decides when the link expires.".to_string(),
        Ok(Some(at)) => {
            format!("The link expires on {}.", details::date_text(&listen_local::tags::iso_utc(at)))
        }
        Err(e) => e,
    };
    let amount = text_input(crate::i18n::tr("1"), &s.amount)
        .on_input(|v| sh(ShareMsg::Amount(v)))
        .size(metrics::font_md())
        .padding(Padding::from([6.0, 10.0]))
        .style(style::input(t))
        .width(80);
    let unit = pick_list(ExpiryUnit::ALL, Some(s.expiry.unit), |u| sh(ShareMsg::Unit(u))).width(120);
    let description = text_input(crate::i18n::tr("Description (optional)"), &s.description)
        .font(crate::ui::font_for(&s.description, crate::appearance::current_font()))
        .on_input(|v| sh(ShareMsg::Description(v)))
        .on_submit(sh(ShareMsg::Submit))
        .size(metrics::font_md())
        .padding(Padding::from([8.0, 10.0]))
        .style(style::input(t));
    let mut col = column![
        label(format!("Sharing: {}", s.title), metrics::font_md(), t.muted),
        label_w("Expiration", metrics::font_md(), t.foreground, Weight::Medium),
        presets,
        row![amount, unit, plain(t, "Make default", Some(sh(ShareMsg::MakeDefault)))]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        label(summary, metrics::font_sm(), t.muted),
        label_w("Description", metrics::font_md(), t.foreground, Weight::Medium),
        description,
    ]
    .spacing(metrics::space_sm());
    if let Some(e) = &s.error {
        col = col.push(label(e, metrics::font_sm(), t.error));
    }
    col.into()
}

// --- Tag editor -----------------------------------------------------------

fn tags_body<'a>(state: &'a Listen, s: &'a TagsState) -> Element<'a, Message> {
    let t = state.tokens;
    let Some(before) = &s.before else {
        return label("Reading the tags…", metrics::font_md(), t.muted).into();
    };
    let mut col = Column::new().spacing(metrics::space_sm());
    match s.step {
        Step::Edit => {
            if s.songs.len() > 1 {
                col = col.push(label(
                    format!(
                        "{} songs. Only the fields you change are written; the others stay as they are.",
                        s.songs.len()
                    ),
                    metrics::font_sm(),
                    t.muted,
                ));
            } else {
                col = col.push(label(s.paths[0].display().to_string(), metrics::font_sm(), t.muted));
            }
            for field in TagField::ALL {
                let Some(edit) = s.fields.get(&field) else { continue };
                let input: Element<'a, Message> = if field == TagField::Lyrics {
                    text_editor(&s.lyrics)
                        .placeholder(if edit.mixed { "(multiple values)" } else { "" })
                        .on_action(|a| tg(TagsMsg::Lyrics(a)))
                        .font(crate::ui::font_for(&s.lyrics.text(), crate::appearance::current_font()))
                        .size(metrics::font_md())
                        .height(90)
                        .into()
                } else {
                    let placeholder = if edit.mixed { "(multiple values)" } else { "" };
                    text_input(placeholder, &edit.value)
                        .font(crate::ui::font_for(&edit.value, crate::appearance::current_font()))
                        .on_input(move |v| tg(TagsMsg::Field(field, v)))
                        .size(metrics::font_md())
                        .padding(Padding::from([6.0, 10.0]))
                        .style(style::input(t))
                        .into()
                };
                let hint = field.multi().then_some("separate with ;");
                let mut line = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
                line = line
                    .push(container(label(field.label(), metrics::font_sm(), t.muted)).width(LABEL_WIDTH));
                line = line.push(container(input).width(Length::Fill));
                if let Some(h) = hint {
                    line = line.push(label(h, metrics::font_xs(), t.muted));
                }
                col = col.push(line);
            }
            col = col.push(cover_row(t, s, before));
        }
        Step::Preview | Step::Writing { .. } => {
            let rows = s.preview();
            col = col.push(label(
                match s.step {
                    Step::Writing { done } => format!("Writing {} of {}…", done + 1, s.paths.len()),
                    _ => "These changes will be written:".to_string(),
                },
                metrics::font_md(),
                t.foreground,
            ));
            let mut table = Column::new().spacing(4);
            for r in rows {
                let files = if s.songs.len() > 1 { format!("{} files", r.files) } else { String::new() };
                table = table.push(
                    row![
                        container(label(r.field, metrics::font_sm(), t.muted)).width(LABEL_WIDTH),
                        container(label(
                            if r.old.is_empty() { "(empty)".into() } else { r.old },
                            metrics::font_md(),
                            t.muted
                        ))
                        .width(Length::FillPortion(1)),
                        label("→", metrics::font_md(), t.muted),
                        container(label(
                            if r.new.is_empty() { "(removed)".into() } else { r.new },
                            metrics::font_md(),
                            t.foreground
                        ))
                        .width(Length::FillPortion(1)),
                        label(files, metrics::font_sm(), t.muted),
                    ]
                    .spacing(metrics::space_sm())
                    .align_y(Alignment::Center),
                );
            }
            col = col.push(scrollable(table).height(Length::Shrink));
        }
    }
    if let Some(e) = &s.error {
        col = col.push(label(e, metrics::font_sm(), t.error));
    }
    container(col).max_height((state.window_size.height - 220.0).max(200.0)).into()
}

fn cover_row<'a>(
    t: Tokens,
    s: &'a TagsState,
    before: &'a [listen_local::tags::FileTags],
) -> Element<'a, Message> {
    let current = match before {
        [one] => one
            .cover
            .as_ref()
            .map_or("No cover".to_string(), |c| format!("{} · {} KB", c.mime, c.bytes.div_ceil(1024))),
        many => {
            let n = many.iter().filter(|f| f.cover.is_some()).count();
            format!("{n} of {} have a cover", many.len())
        }
    };
    let status = match &s.cover {
        CoverEdit::Keep => current,
        CoverEdit::Remove => "Cover will be removed".to_string(),
        CoverEdit::Set(d) => format!("New cover · {} KB", d.len().div_ceil(1024)),
    };
    let path = text_input(crate::i18n::tr("Path of a PNG or JPEG image"), &s.cover_path)
        .on_input(|v| tg(TagsMsg::CoverPath(v)))
        .on_submit(tg(TagsMsg::UseCover))
        .size(metrics::font_md())
        .padding(Padding::from([6.0, 10.0]))
        .style(style::input(t));
    let mut line = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    line = line.push(container(label("Cover", metrics::font_sm(), t.muted)).width(LABEL_WIDTH));
    line = line.push(
        column![
            label(status, metrics::font_sm(), t.foreground),
            row![
                container(path).width(Length::Fill),
                small(t, "Use image", false, tg(TagsMsg::UseCover)),
                small(t, "Remove", matches!(s.cover, CoverEdit::Remove), tg(TagsMsg::RemoveCover)),
                small(t, "Keep", matches!(s.cover, CoverEdit::Keep), tg(TagsMsg::KeepCover)),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        ]
        .spacing(4)
        .width(Length::Fill),
    );
    line.into()
}

fn tag_actions<'a>(t: Tokens, s: &'a TagsState) -> Vec<Element<'a, Message>> {
    match s.step {
        Step::Edit => vec![
            close_button(t, "Cancel"),
            primary(t, "Preview changes", s.before.is_some().then(|| tg(TagsMsg::Preview))),
        ],
        Step::Preview => vec![
            plain(t, "Back", Some(tg(TagsMsg::Back))),
            primary(t, "Write tags", Some(tg(TagsMsg::Write))),
        ],
        Step::Writing { .. } => vec![plain(t, "Writing…", None)],
    }
}
