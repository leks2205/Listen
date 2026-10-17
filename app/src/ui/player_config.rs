//! The player-config popover (Feishin's `player-config.tsx`) and the volume button's device menu.

use super::{label, label_w};
use crate::app::audio::{
    AudioMsg, DeviceChoice, Popup, SPEED_MAX, SPEED_MIN, device_choices, speed_to_pitch,
};
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{
    Column, button, column, container, mouse_area, opaque, pick_list, row, slider, space, stack, toggler,
};
use iced::{Alignment, Background, Element, Length, Padding, Shadow, Vector, border};

/// Width of the popover (Feishin: 540–720 px).
pub const WIDTH: f32 = 480.0;
const MENU_WIDTH: f32 = 300.0;
const ITEM: f32 = 32.0;

fn act(m: AudioMsg) -> Message {
    Message::Audio(m)
}

/// The open popup over `root`. `root` stays the stack's first child so scrollables keep their state.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    if state.needs_login() {
        return stack![root].into();
    }
    match state.audio.popup {
        None => stack![root].into(),
        Some(Popup::Config) => {
            let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(act(AudioMsg::ClosePopup))
                .on_right_press(act(AudioMsg::ClosePopup));
            // Above the right end of the player bar.
            let placed = container(opaque(config_panel(state)))
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
        Some(Popup::Devices(at)) => {
            let choices = device_choices(&state.audio.devices, &state.config.settings.playback.audio_device);
            let height = choices.len() as f32 * ITEM + 44.0;
            let at = super::popover::place(at, iced::Size::new(MENU_WIDTH, height), state.window_size);
            super::popover::over(root, device_menu(state, choices), at, act(AudioMsg::ClosePopup))
        }
    }
}

fn panel_style(state: &Listen) -> impl Fn(&iced::Theme) -> container::Style + use<> {
    let t = state.tokens;
    move |_| container::Style {
        background: Some(Background::Color(t.background_alt)),
        border: border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: Shadow {
            color: style::alpha(iced::Color::BLACK, 0.45),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    }
}

/// The output devices: "System default" and mpv's list, the current one highlighted.
fn device_menu<'a>(state: &'a Listen, choices: Vec<DeviceChoice>) -> Element<'a, Message> {
    let t = state.tokens;
    let current = &state.config.settings.playback.audio_device;
    let mut col = Column::new().push(
        container(label_w("Audio output", metrics::font_sm(), t.muted, Weight::Semibold))
            .padding(Padding::from([6.0, metrics::space_md()])),
    );
    for c in choices {
        let selected = &c.id == current || (c.id == "auto" && current.is_empty());
        col = col.push(
            button(
                container(label_w(
                    c.label.clone(),
                    metrics::font_md(),
                    if selected { t.primary } else { t.foreground },
                    if selected { Weight::Bold } else { Weight::Medium },
                ))
                .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(ITEM)
            .padding(Padding::from([0.0, metrics::space_md()]))
            .style(style::row_button(t, selected))
            .on_press(act(AudioMsg::SetDevice(c.id))),
        );
    }
    if let Some(e) = &state.audio.devices_error {
        col = col.push(
            container(label(e.clone(), metrics::font_sm(), t.error))
                .padding(Padding::from([6.0, metrics::space_md()])),
        );
    }
    opaque(container(col).width(MENU_WIDTH).padding(4).style(panel_style(state)))
}

/// One labelled row of a card: label left, control right.
fn field<'a>(state: &Listen, name: &'static str, control: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    row![
        container(label_w(name, metrics::font_md(), t.foreground, Weight::Medium)).width(150),
        container(control).width(Length::Fill),
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::Center)
    .into()
}

fn card<'a>(state: &Listen, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let t = state.tokens;
    container(Column::with_children(rows).spacing(metrics::space_md()))
        .padding(metrics::space_lg())
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface)),
            border: border::rounded(metrics::radius_md()),
            ..Default::default()
        })
        .into()
}

fn small_button<'a>(state: &Listen, text: String, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(
        text,
        metrics::font_sm(),
        if msg.is_some() { t.foreground } else { t.muted },
        Weight::Medium,
    ))
    .padding(Padding::from([4.0, 10.0]))
    .style(style::default_button(t))
    .on_press_maybe(msg)
    .into()
}

/// Gapless / Crossfade segmented control, as in Feishin; Crossfade is disabled.
fn segmented<'a>(state: &Listen) -> Element<'a, Message> {
    let t = state.tokens;
    let segment = |text: &'static str, selected: bool, enabled: bool| {
        button(
            container(label_w(
                text,
                metrics::font_md(),
                if !enabled {
                    style::alpha(t.muted, 0.6)
                } else if selected {
                    t.foreground
                } else {
                    t.muted
                },
                Weight::Medium,
            ))
            .center_x(Length::Fill),
        )
        .width(Length::Fill)
        .padding(Padding::from([6.0, 10.0]))
        .style(move |_, _| button::Style {
            background: selected.then_some(Background::Color(t.background_alt)),
            text_color: t.foreground,
            border: border::rounded(metrics::radius_sm()),
            snap: true,
            ..Default::default()
        })
    };
    container(row![segment("Gapless", true, true), segment("Crossfade", false, false)].spacing(2))
        .padding(2)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.background)),
            border: border::rounded(metrics::radius_sm()).color(t.border).width(1),
            ..Default::default()
        })
        .into()
}

/// The speed readout of Feishin's slider label: `1.25 x`, with the BPM of the song when known.
pub fn speed_text(speed: f32, bpm: Option<u32>) -> String {
    match bpm.filter(|b| *b > 0) {
        Some(b) => format!("{speed:.2} x / {:.1} BPM", b as f32 * speed),
        None => format!("{speed:.2} x"),
    }
}

/// `1.00x ±0.00st` (Feishin's pitch readout).
pub fn pitch_text(speed: f32) -> String {
    let st = speed_to_pitch(speed);
    let sign = if st.abs() < 0.005 {
        "±"
    } else if st > 0.0 {
        "+"
    } else {
        ""
    };
    format!("{speed:.2}x {sign}{st:.2}st")
}

fn config_panel<'a>(state: &'a Listen) -> Element<'a, Message> {
    let t = state.tokens;
    let p = &state.config.settings.playback;
    let locked = state.speed_locked();
    let speed = state.effective_speed();

    // Audio player and device.
    let player = row![
        container(label_w("MPV", metrics::font_md(), t.foreground, Weight::Medium))
            .padding(Padding::from([6.0, 12.0]))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(t.background)),
                border: border::rounded(metrics::radius_sm()).color(t.border).width(1),
                ..Default::default()
            }),
        small_button(state, "Reload".into(), Some(act(AudioMsg::ReloadMpv))),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let choices = device_choices(&state.audio.devices, &p.audio_device);
    let selected = choices.iter().find(|c| c.id == p.audio_device).cloned();
    let device = row![
        pick_list(choices, selected, |c| act(AudioMsg::SetDevice(c.id))).width(Length::Fill),
        small_button(state, "Refresh".into(), Some(act(AudioMsg::RefreshDevices))),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let audio = card(
        state,
        vec![field(state, "Audio player", player.into()), field(state, "Audio device", device.into())],
    );

    // Playback style: mpv only does gapless (Feishin's crossfade is Web Audio).
    let style_note = label(
        "Crossfade is only available with the web player. Listen plays gaplessly through mpv; \
         the DJ transitions (Settings → Playback) blend songs the Auto-DJ or AI DJ picks.",
        metrics::font_sm(),
        t.muted,
    )
    .wrapping(iced::widget::text::Wrapping::Word);
    let transition = card(state, vec![field(state, "Playback style", segmented(state)), style_note.into()]);

    // Speed, pitch.
    let bpm = state.queue.current().and_then(|i| i.song.bpm);
    let speed_slider = slider(SPEED_MIN..=SPEED_MAX, speed, |v| act(AudioMsg::SetSpeed(v)))
        .step(0.01_f32)
        .width(Length::Fill)
        .style(style::bar_slider(t));
    // Locked: the handler ignores the drag (the slider has no disabled state).
    let marks = row(["0.5", "0.75", "1", "1.25", "1.5", "1.75", "2"].map(|m| {
        container(label(m, metrics::font_xs(), t.muted)).width(Length::Fill).center_x(Length::Fill).into()
    }));
    let speed_row = column![
        row![
            container(label_w("Playback speed", metrics::font_md(), t.foreground, Weight::Medium))
                .width(Length::Fill),
            label(speed_text(speed, bpm), metrics::font_md(), t.muted),
            small_button(state, "1x".into(), (!locked).then(|| act(AudioMsg::ResetSpeed))),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
        speed_slider,
        marks,
    ]
    .spacing(metrics::space_xs());
    let mut playback = vec![speed_row.into()];
    if locked {
        playback.push(
            label(
                "Disabled while DJ transitions are on: they change the songs' speed to match tempos. \
                 Turn off DJ transitions (or the Auto-DJ and AI DJ) to change the speed.",
                metrics::font_sm(),
                t.muted,
            )
            .wrapping(iced::widget::text::Wrapping::Word)
            .into(),
        );
    }
    if !p.preserve_pitch {
        let step = |text: &'static str, semis: f32| {
            let msg = (!locked).then(|| act(AudioMsg::PitchStep(semis)));
            button(
                container(label_w(text, metrics::font_sm(), t.foreground, Weight::Medium))
                    .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .padding(Padding::from([4.0, 6.0]))
            .style(style::default_button(t))
            .on_press_maybe(msg)
        };
        let mut pitch = row![].spacing(metrics::space_sm()).align_y(Alignment::Center);
        pitch = pitch.push(step("-1st", -1.0));
        if p.microtonal_pitch {
            pitch = pitch.push(step("-10ct", -0.1));
        }
        pitch = pitch.push(label(pitch_text(speed), metrics::font_md(), t.foreground));
        if p.microtonal_pitch {
            pitch = pitch.push(step("+10ct", 0.1));
        }
        pitch = pitch.push(step("+1st", 1.0));
        playback.push(pitch.into());
    }
    playback.push(field(
        state,
        "Preserve pitch",
        toggler(p.preserve_pitch).on_toggle(|v| act(AudioMsg::PreservePitch(v))).size(20).into(),
    ));
    let playback = card(state, playback);

    // Sidebar (what Listen has of Feishin's four toggles) and the settings link.
    let sidebar = card(
        state,
        vec![
            field(
                state,
                "Show queue",
                toggler(state.show_queue).on_toggle(|_| Message::ToggleQueue).size(20).into(),
            ),
            field(
                state,
                "Visualizer in queue",
                toggler(state.config.settings.visualizer.in_queue_panel)
                    .on_toggle(|v| Message::Setting(crate::app::Setting::QueueVisualizer(v)))
                    .size(20)
                    .into(),
            ),
            row![
                space().width(Length::Fill),
                small_button(state, "All audio settings".into(), Some(act(AudioMsg::OpenSettings))),
            ]
            .into(),
        ],
    );

    opaque(
        container(column![audio, transition, playback, sidebar].spacing(metrics::space_sm()))
            .width(WIDTH.min((state.window_size.width - 2.0 * metrics::space_lg()).max(240.0)))
            .padding(metrics::space_sm())
            .style(panel_style(state)),
    )
}
