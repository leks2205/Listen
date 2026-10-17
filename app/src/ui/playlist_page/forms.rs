//! The create / edit playlist dialog (Feishin `create-playlist-form.tsx`,
//! `update-playlist-form.tsx`).

use crate::app::overlay::OverlayMsg;
use crate::app::playlists::Form;
use crate::app::playlists::PlaylistPageMsg as P;
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::overlay::ModalParts;
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{Column, button, checkbox, column, row, text_input};
use iced::{Alignment, Color, Padding};

fn msg(m: P) -> Message {
    Message::PlaylistPage(m)
}

pub fn modal(state: &Listen) -> Option<ModalParts<'_>> {
    let t = state.tokens;
    let form: &Form = state.playlist_page.form.as_ref()?;
    let field = |title: &'static str, hint: &'static str, value: &str, make: fn(String) -> P| {
        column![
            label_w(title, metrics::font_sm(), t.muted, Weight::Semibold),
            text_input(crate::i18n::tr(hint), value)
                .font(crate::ui::font_for(value, crate::appearance::current_font()))
                .size(metrics::font_md())
                .padding(Padding::from([8.0, 10.0]))
                .style(style::input(t))
                .on_input_maybe((!form.busy).then_some(move |s| msg(make(s))))
                .on_submit_maybe(form.submittable().then(|| msg(P::FormSubmit))),
        ]
        .spacing(metrics::space_xs())
    };
    let mut body = Column::new().spacing(metrics::space_md());
    body = body.push(field("Name", "Playlist name", &form.name, P::FormName));
    if form.has_details() {
        body = body.push(field("Description", "What is it for?", &form.comment, P::FormComment));
        body = body.push(
            checkbox(form.public)
                .label("Public")
                .size(16)
                .text_size(metrics::font_md())
                .on_toggle_maybe((!form.busy).then_some(|v| msg(P::FormPublic(v)))),
        );
    }
    if form.has_image() {
        let status = match (&form.image, form.remove_image) {
            (Some(image), _) => format!("New cover: {}", image.name),
            (None, true) => "The uploaded cover will be removed.".to_string(),
            (None, false) => "Cover".to_string(),
        };
        body = body.push(
            row![
                label(status, metrics::font_sm(), t.muted),
                iced::widget::space::horizontal(),
                button(label("Upload…", metrics::font_sm(), t.foreground))
                    .padding(Padding::from([4.0, 10.0]))
                    .style(style::default_button(t))
                    .on_press_maybe((!form.busy).then(|| msg(P::FormPickImage))),
                button(label(
                    if form.remove_image { "Keep cover" } else { "Remove cover" },
                    metrics::font_sm(),
                    t.foreground
                ))
                .padding(Padding::from([4.0, 10.0]))
                .style(style::default_button(t))
                .on_press_maybe((!form.busy).then(|| msg(P::FormRemoveImage))),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
    }
    if !form.is_create() && form.source == listen_core::model::SourceKind::YouTubeMusic {
        body = body.push(label(
            "YouTube Music does not tell the current description or visibility; only changes you make are sent.",
            metrics::font_xs(),
            t.muted,
        ));
    }
    let cancel = button(label("Cancel", metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Overlay(OverlayMsg::CloseModal));
    let submit = button(label(
        if form.busy {
            "Saving…"
        } else if form.is_create() {
            "Create"
        } else {
            "Save"
        },
        metrics::font_md(),
        Color::WHITE,
    ))
    .padding(Padding::from([6.0, 16.0]))
    .style(style::primary_button(t))
    .on_press_maybe(form.submittable().then(|| msg(P::FormSubmit)));
    let title = if form.is_create() { "Create playlist" } else { "Edit playlist" };
    Some((title, body.into(), vec![cancel.into(), submit.into()]))
}
