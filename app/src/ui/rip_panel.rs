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
use iced::widget::{Column, button, column, container, image, progress_bar, row, space};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_rip::pipeline::Stage;

const WARN: Color = Color::from_rgb(0.95, 0.62, 0.22);

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

fn small_button<'a>(state: &'a Listen, text: &'a str, primary: bool, m: RipMsg) -> Element<'a, Message> {
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

fn card<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let id = job.id;
    let cover: Element<'_, Message> = match &job.cover {
        Some(h) => image(h.clone()).width(56).height(56).into(),
        None => super::art((60, 60, 70), 56.0, metrics::radius_md(), Icon::LuDisc3),
    };
    let (status, color): (String, Color) = match &job.stage {
        Stage::Ripping => {
            let text = if job.activity.contains("track") || job.track == 0 {
                format!("Ripping · {}", job.activity)
            } else {
                format!("Ripping track {} of {} · {}", job.track, job.total, job.activity)
            };
            (text, t.muted)
        }
        Stage::Tagging => (format!("Tagging · {}", job.activity), t.muted),
        Stage::Publishing => ("Copying to the music folder…".into(), t.muted),
        Stage::NeedsDecision(why) => (format!("{why}. Publish the rest anyway?"), WARN),
        Stage::WaitingForDestination(why) => (format!("Waiting for the music folder: {why}"), WARN),
        Stage::Done => (
            job.report
                .as_ref()
                .map(|r| format!("Filed in {} · {}", r.folder.display(), r.accuraterip_line()))
                .unwrap_or_else(|| "Done".into()),
            t.foreground,
        ),
        Stage::Failed(why) => (format!("Failed: {why}"), WARN),
        Stage::Cancelled => ("Cancelled — nothing was copied to the music folder.".into(), t.muted),
    };
    let mut info = column![
        label_w(job.title(), metrics::font_md(), t.foreground, Weight::Semibold),
        label(status, metrics::font_sm(), color),
    ]
    .spacing(4)
    .width(Length::Fill);
    if matches!(job.stage, Stage::Ripping | Stage::Tagging) {
        let value = if job.total > 0 {
            (job.track.saturating_sub(1) as f32 + job.percent as f32 / 100.0) / job.total as f32 * 100.0
        } else {
            0.0
        };
        info = info.push(progress_bar(0.0..=100.0, value).girth(6).style(move |_| progress_bar::Style {
            background: t.border.into(),
            bar: t.primary.into(),
            border: iced::Border { radius: 3.0.into(), ..Default::default() },
        }));
    }
    for w in job.warnings.iter().rev().take(2) {
        info = info.push(label(w.as_str(), metrics::font_xs(), WARN));
    }
    if !job.candidates.is_empty() && !job.stage.finished() && job.stage != Stage::Publishing {
        info = info.push(picker(state, job));
    }

    let mut actions = row![].spacing(metrics::space_sm()).align_y(Alignment::Center);
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
            actions = actions.push(small_button(state, "Cancel", false, RipMsg::Cancel(id)))
        }
        _ => {
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
    }
    container(row![cover, info, actions].spacing(metrics::space_md()).align_y(Alignment::Center))
        .padding(metrics::space_md())
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.surface)),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
            ..Default::default()
        })
        .into()
}

/// Several releases match: pick the version (best one pre-selected).
fn picker<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let head = match job.seconds_left() {
        Some(s) => format!("Which version is this CD? Taking the highlighted one in {s} s."),
        None => "Version (you can still change it until tagging starts):".into(),
    };
    let mut col = column![label(head, metrics::font_sm(), t.muted)].spacing(2);
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
