//! Playback tab: the Milkdrop visualizer (type, preset folder, picking,
//! cycling and blending, like Feishin's butterchurn options).

use super::{line, section, stepper, switch};
use crate::app::milkdrop::{MilkdropMsg, MilkdropSetting};
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use crate::ui::label;
use iced::font::Weight;
use iced::widget::{button, column, row, text_input};
use iced::{Alignment, Element, Padding};
use listen_core::settings::VisualizerKind;

/// Most matches the preset search lists.
const MATCHES: usize = 8;

fn change(c: MilkdropSetting) -> Message {
    Message::Setting(Setting::Milkdrop(c))
}

fn small<'a>(state: &Listen, text: impl Into<String>, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    button(crate::ui::label_w(text.into(), metrics::font_sm(), t.foreground, Weight::Semibold))
        .padding(Padding::from([6.0, 10.0]))
        .style(style::default_button(t))
        .on_press_maybe(msg)
        .into()
}

/// The presets whose name contains `query` (case-insensitive), first `MATCHES`.
pub fn matches<'a>(names: impl Iterator<Item = &'a str>, query: &str) -> Vec<&'a str> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    names.filter(|n| n.to_lowercase().contains(&q)).take(MATCHES).collect()
}

pub(super) fn milkdrop(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let m = &state.config.settings.milkdrop;
    let mk = &state.milk;
    let available = crate::milkdrop::availability();
    let kind = |k: VisualizerKind, name: &'static str, enabled: bool| {
        let on = m.kind == k;
        let b = button(crate::ui::label_w(name, metrics::font_sm(), t.foreground, Weight::Semibold))
            .padding(Padding::from([6.0, 12.0]));
        let b: iced::widget::Button<'_, Message> =
            if on { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) };
        let b = b.on_press_maybe((enabled && !on).then(|| change(MilkdropSetting::Kind(k))));
        Element::from(b)
    };
    let kinds: Element<'_, Message> = row![
        kind(VisualizerKind::Spectrum, "Spectrum", true),
        kind(VisualizerKind::Milkdrop, "Milkdrop", available.is_ok()),
    ]
    .spacing(metrics::space_xs())
    .into();
    let type_hint = match &available {
        Ok(()) => "Spectrum (audioMotion) or Milkdrop presets (libprojectM) in the visualizer.".to_string(),
        Err(e) => e.clone(),
    };
    let mut rows = vec![line(state, "Visualizer type", type_hint, kinds)];
    if available.is_ok() {
        let folder = state.milkdrop_folder().map(|p| p.display().to_string()).unwrap_or_default();
        let typed = mk.folder_input.clone().unwrap_or_else(|| m.preset_dir.clone());
        let count = if mk.scanning {
            "Reading the folder…".to_string()
        } else if mk.scanned.is_some() {
            format!("{} presets in {folder}", mk.presets.len())
        } else {
            folder.clone()
        };
        rows.push(line(
            state,
            "Preset folder",
            count,
            row![
                text_input("Default folder", &typed)
                    .size(metrics::font_md())
                    .padding(Padding::from([6.0, 8.0]))
                    .style(style::input(t))
                    .width(260)
                    .on_input(|v| Message::Milkdrop(MilkdropMsg::FolderInput(v)))
                    .on_submit(Message::Milkdrop(MilkdropMsg::FolderSave)),
                small(state, "Browse…", Some(Message::Milkdrop(MilkdropMsg::PickFolder))),
                small(state, "Rescan", Some(Message::Milkdrop(MilkdropMsg::Rescan))),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center)
            .into(),
        ));
        // Picking a preset: a search box, since packs have thousands.
        let found = matches(mk.presets.iter().map(|p| p.name.as_str()), &mk.query);
        let mut pick = column![
            text_input("Search presets", &mk.query)
                .size(metrics::font_md())
                .padding(Padding::from([6.0, 8.0]))
                .style(style::input(t))
                .width(260)
                .on_input(|v| Message::Milkdrop(MilkdropMsg::Query(v))),
        ]
        .spacing(metrics::space_xs());
        for name in found {
            pick = pick.push(
                button(label(
                    crate::ui::ellipsize_px(name, 240.0, metrics::font_sm()),
                    metrics::font_sm(),
                    t.foreground,
                ))
                .padding(Padding::from([4.0, 8.0]))
                .width(260)
                .style(style::default_button(t))
                .on_press(Message::Milkdrop(MilkdropMsg::Pick(name.to_string()))),
            );
        }
        rows.push(line(
            state,
            "Preset",
            mk.preset
                .clone()
                .unwrap_or_else(|| "Search to pick one; the next button is on the picture.".into()),
            pick.into(),
        ));
        rows.push(line(
            state,
            "Next preset",
            "Switch now (also the button on the picture).",
            small(state, "Next", Some(Message::Milkdrop(MilkdropMsg::Next))),
        ));
        rows.push(line(
            state,
            "Cycle presets",
            "Switch to another preset every few seconds.",
            switch(m.cycle_presets, |v| Setting::Milkdrop(MilkdropSetting::Cycle(v))),
        ));
        rows.push(line(
            state,
            "Cycle time",
            "Seconds a preset stays.",
            stepper(state, 1.0..=300.0, m.cycle_time as f32, format!("{} s", m.cycle_time), |v| {
                Setting::Milkdrop(MilkdropSetting::CycleTime(v as u32))
            }),
        ));
        rows.push(line(
            state,
            "Blend time",
            "Seconds a preset takes to blend into the next one.",
            stepper(state, 0.0..=10.0, m.blend_time, format!("{:.1} s", m.blend_time), |v| {
                Setting::Milkdrop(MilkdropSetting::BlendTime(v))
            }),
        ));
        rows.push(line(
            state,
            "Random next preset",
            "Pick the next preset at random (off: the next in the list).",
            switch(m.randomize, |v| Setting::Milkdrop(MilkdropSetting::Randomize(v))),
        ));
        rows.push(line(
            state,
            "Include all presets",
            "Cycle through every preset (off: only the selected ones).",
            switch(m.include_all, |v| Setting::Milkdrop(MilkdropSetting::IncludeAll(v))),
        ));
        rows.push(line(
            state,
            "Selected presets",
            format!("{} selected; the preset on screen can be added or removed.", m.selected.len()),
            row![
                small(state, "Add / remove current", Some(change(MilkdropSetting::ToggleSelected))),
                small(
                    state,
                    "Clear",
                    (!m.selected.is_empty()).then(|| change(MilkdropSetting::ClearSelected))
                ),
            ]
            .spacing(metrics::space_xs())
            .into(),
        ));
        rows.push(line(
            state,
            "Ignored presets",
            format!("{} ignored; they are never picked.", m.ignored.len()),
            row![
                small(state, "Ignore current", Some(change(MilkdropSetting::IgnoreCurrent))),
                small(state, "Clear", (!m.ignored.is_empty()).then(|| change(MilkdropSetting::ClearIgnored))),
            ]
            .spacing(metrics::space_xs())
            .into(),
        ));
        rows.push(line(
            state,
            "Max FPS",
            "Frames a second (0 = 30).",
            stepper(state, 0.0..=144.0, m.max_fps as f32, m.max_fps.to_string(), |v| {
                Setting::Milkdrop(MilkdropSetting::MaxFps(v as u32))
            }),
        ));
        rows.push(line(
            state,
            "Opacity",
            "How strongly the picture shows.",
            stepper(state, 0.0..=1.0, m.opacity, format!("{:.0} %", m.opacity * 100.0), |v| {
                Setting::Milkdrop(MilkdropSetting::Opacity(v))
            }),
        ));
    }
    section(state, "Milkdrop", rows)
}
