//! The rip panel above the player bar: one card per disc with the cover,
//! album, progress and Cancel; the release picker when several MusicBrainz
//! releases match (auto-confirms), and the questions a rip can ask
//! (unreadable tracks, destination not available).

use super::{icon, label, label_w};
use crate::app::rip::{RipJob, RipMsg};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, button, column, container, image, progress_bar, row, space};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_rip::pipeline::Stage;

pub(super) const WARN: Color = Color::from_rgb(0.95, 0.62, 0.22);

pub fn view(state: &Listen) -> Option<Element<'_, Message>> {
    if state.rip.jobs.is_empty() {
        return None;
    }
    let mut col = Column::new().spacing(metrics::space_sm());
    for job in &state.rip.jobs {
        col = col.push(card(state, job));
    }
    Some(container(col).padding(Padding::from([metrics::space_sm(), metrics::space_lg()])).into())
}

pub(super) fn small_button<'a>(
    state: &'a Listen,
    text: &'a str,
    primary: bool,
    m: RipMsg,
) -> Element<'a, Message> {
    let t = state.tokens;
    let b = button(label_w(
        text,
        metrics::font_sm(),
        if primary { Color::WHITE } else { t.foreground },
        Weight::Semibold,
    ))
    .padding(Padding::from([6.0, 12.0]))
    .on_press(Message::Rip(m));
    if primary { b.style(style::primary_button(t)).into() } else { b.style(style::default_button(t)).into() }
}

/// The cover, or a disc where there is none.
pub(super) fn cover<'a>(job: &RipJob, size: f32) -> Element<'a, Message> {
    match &job.cover {
        Some(h) => image(h.clone()).width(size).height(size).into(),
        None => super::art((60, 60, 70), size, metrics::radius_md(), Icon::LuDisc3),
    }
}

/// The status line (translated) and its colour.
pub(super) fn status(state: &Listen, job: &RipJob) -> (String, Color) {
    let t = state.tokens;
    let activity = crate::i18n::tr_cow(job.activity.as_str().into());
    match &job.stage {
        Stage::Ripping => {
            // listen-rip's per-track activities ("Reading track 3 of 12") carry the counter themselves;
            // keep that wording in listen-rip's pipeline or the counter shows twice.
            let text = if job.activity.contains("track") || job.track == 0 {
                crate::i18n::trf("Ripping · {}", &[&activity])
            } else {
                crate::i18n::trf("Ripping track {} of {} · {}", &[&job.track, &job.total, &activity])
            };
            (text, t.muted)
        }
        Stage::Tagging => (crate::i18n::trf("Tagging · {}", &[&activity]), t.muted),
        Stage::Publishing => (crate::i18n::tr("Copying to the music folder…").to_string(), t.muted),
        Stage::NeedsDecision(why) => (crate::i18n::trf("{}. Publish the rest anyway?", &[why]), WARN),
        Stage::WaitingForDestination(why) => {
            (crate::i18n::trf("Waiting for the music folder: {}", &[why]), WARN)
        }
        Stage::Done => (
            job.report
                .as_ref()
                .map(|r| {
                    let ar = crate::i18n::tr_cow(r.accuraterip_line().into()).into_owned();
                    crate::i18n::trf("Filed in {} · {}", &[&r.folder.display(), &ar])
                })
                .unwrap_or_else(|| crate::i18n::tr("Done").to_string()),
            t.foreground,
        ),
        Stage::Failed(why) => (crate::i18n::trf("Failed: {}", &[why]), WARN),
        Stage::Cancelled => {
            (crate::i18n::tr("Cancelled — nothing was copied to the music folder.").to_string(), t.muted)
        }
    }
}

/// The disc's progress bar (0-100).
pub(super) fn bar<'a>(state: &Listen, value: f32) -> Element<'a, Message> {
    let t = state.tokens;
    progress_bar(0.0..=100.0, value)
        .girth(6)
        .style(move |_| progress_bar::Style {
            background: t.border.into(),
            bar: t.primary.into(),
            border: iced::Border { radius: 3.0.into(), ..Default::default() },
        })
        .into()
}

fn card<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let (status, color) = status(state, job);
    let mut info = column![
        label_w(job.title(), metrics::font_md(), t.foreground, Weight::Semibold),
        // Built translated above; error texts and paths inside stay as they are.
        crate::ui::data_label(status, metrics::font_sm(), color)
            .width(Length::Fill)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(4)
    .width(Length::Fill);
    if matches!(job.stage, Stage::Ripping | Stage::Tagging) {
        let value = if job.total > 0 {
            (job.track.saturating_sub(1) as f32 + job.percent as f32 / 100.0) / job.total as f32 * 100.0
        } else {
            0.0
        };
        info = info.push(bar(state, value));
    }
    for w in job.warnings.iter().rev().take(2) {
        // A warning names an album or a path and is longer than the card is wide.
        info = info.push(
            label(w.as_str(), metrics::font_xs(), WARN).width(Length::Fill).wrapping(Wrapping::WordOrGlyph),
        );
    }
    if !job.candidates.is_empty() && !job.stage.finished() && job.stage != Stage::Publishing {
        info = info.push(picker(state, job));
    }

    let actions = actions(state, job, true);
    container(row![cover(job, 56.0), info, actions].spacing(metrics::space_md()).align_y(Alignment::Center))
        .padding(metrics::space_md())
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.surface)),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
            ..Default::default()
        })
        .into()
}

/// What can be done with a rip right now; `details` adds the button that opens the rip window.
pub(super) fn actions<'a>(state: &'a Listen, job: &'a RipJob, details: bool) -> Row<'a, Message> {
    let t = state.tokens;
    let id = job.id;
    let mut actions = row![].spacing(metrics::space_sm()).align_y(Alignment::Center);
    if details {
        actions = actions.push(
            button(label_w("Details", metrics::font_sm(), t.foreground, Weight::Semibold))
                .padding(Padding::from([6.0, 12.0]))
                .style(style::default_button(t))
                .on_press(Message::Navigate(crate::app::Route::Rips)),
        );
    }
    match &job.stage {
        Stage::NeedsDecision(_) => {
            actions = actions
                .push(small_button(state, "Publish anyway", true, RipMsg::PublishAnyway(id)))
                .push(small_button(state, "Discard", false, RipMsg::Discard(id)));
        }
        Stage::WaitingForDestination(_) => {
            actions = actions
                .push(small_button(state, "Choose folder…", true, RipMsg::BrowseDestination))
                .push(small_button(state, "Retry", false, RipMsg::Retry(id)))
                .push(small_button(state, "Discard", false, RipMsg::Discard(id)));
        }
        s if !s.finished() => {
            if *s == Stage::Ripping
                && let Some(switch) = &job.mode
            {
                actions = actions.push(mode_toggle(state, id, switch));
            }
            actions = actions.push(small_button(state, "Cancel", false, RipMsg::Cancel(id)))
        }
        _ if details => {
            actions = actions.push(crate::ui::tip::tip(
                state,
                button(icon(Icon::LuX, metrics::font_md(), t.muted))
                    .padding(4)
                    .style(style::icon_button(t, false))
                    .on_press(Message::Rip(RipMsg::Dismiss(id))),
                "Dismiss",
                None,
                crate::ui::tip::Position::Bottom,
            ))
        }
        _ => {}
    }
    actions
}

/// Secure / Fast for this rip; only active until the first track is being read.
pub(super) fn mode_toggle<'a>(
    state: &'a Listen,
    id: u64,
    switch: &listen_rip::ModeSwitch,
) -> Element<'a, Message> {
    use listen_core::settings::RipMode;
    let t = state.tokens;
    let (current, locked) = (switch.mode(), switch.locked());
    let segment = |mode: RipMode| {
        let selected = mode == current;
        let b = button(label_w(
            mode.label(),
            metrics::font_sm(),
            if selected { Color::WHITE } else { t.foreground },
            Weight::Semibold,
        ))
        .padding(Padding::from([6.0, 12.0]))
        .on_press_maybe((!locked && !selected).then_some(Message::Rip(RipMsg::Mode(id, mode))));
        if selected { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) }
    };
    row![segment(RipMode::Secure), segment(RipMode::Fast)].spacing(2).into()
}

/// Several releases match: pick the version (best one pre-selected).
pub(super) fn picker<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let head = match job.seconds_left() {
        Some(s) => format!("Which version is this CD? Taking the highlighted one in {s} s."),
        None => "Version (you can still change it until tagging starts):".into(),
    };
    let mut col =
        column![label(head, metrics::font_sm(), t.muted).width(Length::Fill).wrapping(Wrapping::WordOrGlyph)]
            .spacing(2);
    for (i, c) in job.candidates.iter().enumerate().take(8) {
        let chosen = i == job.chosen;
        let text =
            if c.details.is_empty() { c.title.clone() } else { format!("{} — {}", c.title, c.details) };
        col = col.push(
            button(
                row![
                    label(
                        if chosen { "●" } else { "○" },
                        metrics::font_sm(),
                        if chosen { t.primary } else { t.muted }
                    ),
                    label(text, metrics::font_sm(), if chosen { t.foreground } else { t.muted }),
                    space::horizontal(),
                ]
                .spacing(metrics::space_sm()),
            )
            .width(Length::Fill)
            .padding(Padding::from([3.0, 6.0]))
            .style(style::row_button(t, chosen))
            .on_press(Message::Rip(RipMsg::Choose(job.id, i))),
        );
    }
    col.into()
}
