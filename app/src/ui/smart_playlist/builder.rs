//! Smart-playlist rule editor (Feishin `QueryBuilder` + `QueryBuilderOption`): groups with
//! "match all / any", their rules and sub-groups, then sort entries and the limit.

use crate::app::smart_playlist::{Editor, GroupEdit, PlaylistChoice, RuleEdit, SmartMsg, SortEdit};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, pick_list, row, space, text_input};
use iced::{Alignment, Element, Length, Padding};
use listen_source::nsp::GroupKind;
use listen_source::nsp::fields::operators;
use listen_source::nsp::query::{FieldOpt, Input, LimitMode, QGroup, QRule};
use std::fmt;

fn msg(m: SmartMsg) -> Message {
    Message::Smart(m)
}

fn small<'a>(state: &Listen, text: &'static str, on: SmartMsg) -> Element<'a, Message> {
    let t = state.tokens;
    button(label(text, metrics::font_sm(), t.foreground))
        .padding(Padding::from([3.0, 8.0]))
        .style(style::default_button(t))
        .on_press(msg(on))
        .into()
}

fn remove<'a>(state: &Listen, on: Option<SmartMsg>) -> Element<'a, Message> {
    let t = state.tokens;
    crate::ui::tip::tip(
        state,
        button(icon(Icon::LuX, metrics::font_md(), t.muted))
            .padding(5)
            .style(style::icon_button(t, false))
            .on_press_maybe(on.map(msg)),
        "Remove",
        None,
        crate::ui::tip::Position::Left,
    )
}

fn input<'a>(
    state: &Listen,
    placeholder: &'static str,
    value: &'a str,
    on: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    text_input(placeholder, value)
        .font(crate::ui::font_for(value, crate::appearance::current_font()))
        .on_input(on)
        .size(metrics::font_sm())
        .padding(Padding::from([5.0, 8.0]))
        .style(style::input(state.tokens))
        .width(Length::FillPortion(2))
        .into()
}

/// A plain choice for the boolean and order pick lists.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Choice(&'static str);

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The value input of a rule: which one depends on the field's type and the operator.
fn value<'a>(
    state: &'a Listen,
    e: &'a Editor,
    path: &[usize],
    i: usize,
    r: &'a QRule,
) -> Element<'a, Message> {
    let at = |edit: fn(String) -> RuleEdit| {
        let path = path.to_vec();
        move |text: String| msg(SmartMsg::Rule(path.clone(), i, edit(text)))
    };
    match r.input() {
        Input::Text => input(state, "Value", &r.a, at(RuleEdit::A)),
        Input::Number => input(state, "Number", &r.a, at(RuleEdit::A)),
        Input::Date => input(state, "YYYY-MM-DD", &r.a, at(RuleEdit::A)),
        Input::NumberRange => {
            row![input(state, "From", &r.a, at(RuleEdit::A)), input(state, "To", &r.b, at(RuleEdit::B))]
                .spacing(metrics::space_xs())
                .width(Length::FillPortion(2))
                .into()
        }
        Input::DateRange => row![
            input(state, "From (YYYY-MM-DD)", &r.a, at(RuleEdit::A)),
            input(state, "To (YYYY-MM-DD)", &r.b, at(RuleEdit::B))
        ]
        .spacing(metrics::space_xs())
        .width(Length::FillPortion(2))
        .into(),
        Input::Bool => {
            let path = path.to_vec();
            let selected = Choice(if r.a == "false" { "false" } else { "true" });
            pick_list([Choice("true"), Choice("false")], Some(selected), move |c: Choice| {
                msg(SmartMsg::Rule(path.clone(), i, RuleEdit::A(c.0.to_string())))
            })
            .text_size(metrics::font_sm())
            .width(Length::FillPortion(2))
            .into()
        }
        Input::Playlist => {
            let mut choices: Vec<PlaylistChoice> = state
                .library
                .playlists
                .iter()
                .filter(|p| e.id.as_deref() != Some(&p.id))
                .map(|p| PlaylistChoice { id: p.id.clone(), name: p.name.clone() })
                .collect();
            let selected = (!r.a.is_empty()).then(|| {
                choices.iter().find(|c| c.id == r.a).cloned().unwrap_or_else(|| {
                    // A playlist that is not in the list (yet): show its id.
                    let c = PlaylistChoice { id: r.a.clone(), name: r.a.clone() };
                    choices.push(c.clone());
                    c
                })
            });
            let path = path.to_vec();
            pick_list(choices, selected, move |c: PlaylistChoice| {
                msg(SmartMsg::Rule(path.clone(), i, RuleEdit::A(c.id)))
            })
            .placeholder("Playlist")
            .text_size(metrics::font_sm())
            .width(Length::FillPortion(2))
            .into()
        }
    }
}

fn rule_row<'a>(
    state: &'a Listen,
    e: &'a Editor,
    path: &[usize],
    i: usize,
    r: &'a QRule,
) -> Element<'a, Message> {
    let field_path = path.to_vec();
    let selected = (!r.field.is_empty()).then(|| {
        e.fields.iter().find(|f| f.key == r.field).cloned().unwrap_or_else(|| FieldOpt {
            label: r.field.clone(),
            key: r.field.clone(),
            kind: r.kind,
            custom: true,
        })
    });
    let field = pick_list(e.fields.clone(), selected, move |f: FieldOpt| {
        msg(SmartMsg::Rule(field_path.clone(), i, RuleEdit::Field(f)))
    })
    .placeholder("Field")
    .text_size(metrics::font_sm())
    .width(Length::FillPortion(2));

    let ops = if r.field.is_empty() { &[][..] } else { operators(r.kind) };
    let op_path = path.to_vec();
    let op = pick_list(ops, ops.iter().find(|o| o.id == r.op).copied(), move |o| {
        msg(SmartMsg::Rule(op_path.clone(), i, RuleEdit::Op(o)))
    })
    .placeholder("Operator")
    .text_size(metrics::font_sm())
    .width(Length::FillPortion(2));

    let rm = remove(state, Some(SmartMsg::Rule(path.to_vec(), i, RuleEdit::Remove)));
    let value: Element<'a, Message> = if r.field.is_empty() {
        // Like Feishin: a disabled box until a field is picked.
        text_input(crate::i18n::tr(""), "")
            .size(metrics::font_sm())
            .padding(Padding::from([5.0, 8.0]))
            .style(style::input(state.tokens))
            .width(Length::FillPortion(2))
            .into()
    } else {
        value(state, e, path, i, r)
    };
    row![field, op, value, rm].spacing(metrics::space_sm()).align_y(Alignment::Center).into()
}

/// A group box: header, its rules, then its sub-groups.
pub fn group<'a>(state: &'a Listen, e: &'a Editor, g: &'a QGroup, path: Vec<usize>) -> Element<'a, Message> {
    let t = state.tokens;
    let root = path.is_empty();
    let kind_path = path.clone();
    let kind = pick_list(GroupKind::BOTH, Some(g.kind), move |k| {
        msg(SmartMsg::Group(kind_path.clone(), GroupEdit::Kind(k)))
    })
    .text_size(metrics::font_sm())
    .width(150);
    let mut header = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center).push(kind);
    header = header.push(small(state, "Add rule", SmartMsg::Group(path.clone(), GroupEdit::AddRule)));
    header = header.push(small(state, "Add rule group", SmartMsg::Group(path.clone(), GroupEdit::AddGroup)));
    header = header.push(space::horizontal());
    if root {
        header = header.push(small(state, "Reset to default", SmartMsg::Reset));
        header = header.push(small(state, "Clear filters", SmartMsg::Clear));
    } else {
        header =
            header.push(small(state, "Remove rule group", SmartMsg::Group(path.clone(), GroupEdit::Remove)));
    }
    let mut col = Column::new().spacing(metrics::space_sm()).push(header);
    for (i, r) in g.rules.iter().enumerate() {
        col = col.push(rule_row(state, e, &path, i, r));
    }
    for (i, sub) in g.groups.iter().enumerate() {
        let mut p = path.clone();
        p.push(i);
        col = col.push(group(state, e, sub, p));
    }
    container(col)
        .padding(metrics::space_md())
        .width(Length::Fill)
        .style(move |_| container::Style {
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
            ..Default::default()
        })
        .into()
}

/// Sort entries (field + order, several allowed) and the limit (count or percent).
pub fn sort_and_limit<'a>(state: &'a Listen, e: &'a Editor) -> Element<'a, Message> {
    let t = state.tokens;
    let last = e.query.sort.len().saturating_sub(1);
    let mut sorts = Column::new().spacing(metrics::space_xs());
    sorts = sorts.push(label_w("Sort", metrics::font_sm(), t.muted, Weight::Medium));
    for (i, s) in e.query.sort.iter().enumerate() {
        let selected =
            (!s.field.is_empty()).then(|| e.sorts.iter().find(|f| f.key == s.field).cloned()).flatten();
        let field = pick_list(e.sorts.clone(), selected, move |f: FieldOpt| {
            msg(SmartMsg::Sort(SortEdit::Field(i, f)))
        })
        .placeholder("Field")
        .text_size(metrics::font_sm())
        .width(220);
        let order = pick_list(
            [Choice("Ascending"), Choice("Descending")],
            Some(Choice(if s.desc { "Descending" } else { "Ascending" })),
            move |c: Choice| msg(SmartMsg::Sort(SortEdit::Desc(i, c.0 == "Descending"))),
        )
        .text_size(metrics::font_sm())
        .width(140);
        let mut line =
            Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center).push(field).push(order);
        if e.query.sort.len() > 1 {
            line = line.push(remove(state, Some(SmartMsg::Sort(SortEdit::Remove(i)))));
        }
        if i == last {
            line = line.push(small(state, "Add sort", SmartMsg::Sort(SortEdit::Add)));
        }
        sorts = sorts.push(line);
    }

    let (mode, text) = match e.query.limit_mode {
        LimitMode::Count => ("#", &e.query.limit),
        LimitMode::Percent => ("%", &e.query.limit_percent),
    };
    let toggle = |name: &'static str, to: LimitMode, active: bool| {
        button(label(name, metrics::font_sm(), t.foreground))
            .padding(Padding::from([3.0, 8.0]))
            .style(style::icon_button(t, active))
            .on_press(msg(SmartMsg::Sort(SortEdit::Mode(to))))
    };
    let limit = column![
        row![
            label_w("Limit", metrics::font_sm(), t.muted, Weight::Medium),
            toggle("#", LimitMode::Count, mode == "#"),
            toggle("%", LimitMode::Percent, mode == "%"),
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center),
        text_input(if mode == "#" { "No limit" } else { "0 - 100" }, text)
            .on_input(|v| msg(SmartMsg::Sort(SortEdit::Limit(v))))
            .size(metrics::font_sm())
            .padding(Padding::from([5.0, 8.0]))
            .style(style::input(t))
            .width(110),
    ]
    .spacing(metrics::space_xs());
    row![sorts, space::horizontal(), limit].align_y(Alignment::End).into()
}
