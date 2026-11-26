//! The rip window: a page with every rip (several drives can rip at once). Per rip the header
//! (cover, album, where the metadata came from, drive and offset, mode, destination), the actions the
//! rip card has, the disc's progress with elapsed time, time left and read speed, a table with one row
//! per track of the TOC, and the ripper's log. A finished rip stays as the record of what happened.

use super::rip_panel::{self, WARN};
use super::{data_label, label, label_w};
use crate::app::rip::{RipJob, TrackAr, TrackProgress, TrackState};
use crate::app::{Listen, Message};
use crate::style::metrics;
use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, row, scrollable};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_rip::pipeline::Stage;
use listen_rip::tags::MetaSource;
use std::time::Duration;

/// Width of the "#", length, state and AccurateRip columns.
const NUMBER_W: f32 = 28.0;
const LENGTH_W: f32 = 52.0;
const STATE_W: f32 = 96.0;
const AR_W: f32 = 112.0;
/// Below this page width the table has no Progress and AccurateRip columns: the bar goes under the
/// title, the AccurateRip result under the state.
const NARROW_PAGE: f32 = 760.0;
/// The log shows about this many lines and scrolls for the rest.
const LOG_LINES: f32 = 10.0;

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let mut page = Column::new()
        .spacing(metrics::space_lg())
        .push(label_w("CD ripping", 36.0, t.foreground, Weight::Bold).width(Length::Fill));
    if state.rip.jobs.is_empty() {
        page = page.push(label("No CD is being ripped.", metrics::font_md(), t.muted));
    }
    for job in &state.rip.jobs {
        page = page.push(job_view(state, job));
    }
    scrollable(container(page).width(Length::Fill).padding(Padding {
        top: 24.0,
        right: metrics::PAGE_PADDING_X,
        bottom: metrics::space_xl(),
        left: metrics::PAGE_PADDING_X,
    }))
    .height(Length::Fill)
    .into()
}

/// "3:07", "1:02:45".
fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Text that can be longer than the window is wide.
fn wrap<'a>(t: iced::widget::Text<'a>) -> iced::widget::Text<'a> {
    t.width(Length::Fill).wrapping(Wrapping::WordOrGlyph)
}

fn job_view<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let (status, color) = rip_panel::status(state, job);
    let mut col = column![
        header(state, job),
        rip_panel::actions(state, job, false).wrap(),
        // Translated by `status`; error texts and paths inside stay as they are.
        wrap(data_label(status, metrics::font_sm(), color)),
    ]
    .spacing(metrics::space_md());
    for w in &job.warnings {
        col = col.push(wrap(data_label(w.as_str(), metrics::font_sm(), WARN)));
    }
    if !job.candidates.is_empty() && !job.stage.finished() && job.stage != Stage::Publishing {
        col = col.push(rip_panel::picker(state, job));
    }
    col = col.push(overall(state, job));
    if !job.tracks.is_empty() {
        col = col.push(table(state, job));
    }
    if !job.log.is_empty() {
        col = col.push(log(state, job));
    }
    container(col)
        .padding(metrics::space_lg())
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.surface)),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
            ..Default::default()
        })
        .into()
}

fn header<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let small = |s: String| wrap(data_label(s, metrics::font_sm(), t.muted));
    let mut info =
        column![wrap(super::data_label_w(job.title(), metrics::font_xl(), t.foreground, Weight::Bold))]
            .spacing(metrics::space_xs())
            .width(Length::Fill);
    let mut first = Vec::new();
    if let Some(source) = job.source {
        let name = match source {
            MetaSource::MusicBrainz => "MusicBrainz".to_string(),
            MetaSource::CdText => "CD-Text".to_string(),
            MetaSource::Unknown => crate::i18n::tr("Unknown").to_string(),
            #[allow(unreachable_patterns)]
            other => format!("{other:?}"),
        };
        first.push(crate::i18n::trf("Metadata from {}", &[&name]));
    }
    if let Some(mode) = &job.mode {
        first.push(crate::i18n::tr(mode.mode().label()).to_string());
    }
    if !first.is_empty() {
        info = info.push(small(first.join(" · ")));
    }
    if !job.drive_label.is_empty() {
        let offset = match job.offset {
            Some((o, from)) => {
                crate::i18n::trf("read offset {} ({})", &[&format!("{o:+}"), &crate::i18n::tr(from)])
            }
            None => crate::i18n::tr("read offset unknown — enter it or use “Verify offset”").to_string(),
        };
        info = info.push(small(crate::i18n::trf(
            "{} at {} — {}",
            &[&job.drive_label, &job.device.display(), &offset],
        )));
    }
    if !job.destination.as_os_str().is_empty() {
        info = info.push(small(crate::i18n::trf("Rips go to {}.", &[&job.destination.display()])));
    }
    row![rip_panel::cover(job, 96.0), info].spacing(metrics::space_lg()).align_y(Alignment::Start).into()
}

/// The disc's bar, elapsed, time left and the read speed.
fn overall<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let mut parts = vec![crate::i18n::trf("Elapsed {}", &[&clock(job.clock.elapsed)])];
    if let Some(left) = job.time_left() {
        parts.push(crate::i18n::trf("{} left", &[&clock(left)]));
    }
    if let Some(speed) = job.read_speed() {
        parts.push(crate::i18n::trf("{}× real time", &[&format!("{speed:.1}")]));
    }
    column![
        rip_panel::bar(state, job.overall() * 100.0),
        wrap(data_label(parts.join(" · "), metrics::font_sm(), t.muted)),
    ]
    .spacing(metrics::space_xs())
    .into()
}

fn table<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let head = |s: &'static str, w: Length| {
        container(label_w(s, metrics::font_xs(), t.muted, Weight::Semibold).wrapping(Wrapping::WordOrGlyph))
            .width(w)
    };
    let narrow = state.page_width() < NARROW_PAGE;
    let mut heads = row![
        head("#", Length::Fixed(NUMBER_W)),
        head("Title", Length::FillPortion(3)),
        head("Duration", Length::Fixed(LENGTH_W)),
        head("Status", Length::Fixed(STATE_W)),
    ]
    .spacing(metrics::space_sm());
    if !narrow {
        heads = heads
            .push(head("Progress", Length::FillPortion(1)))
            .push(head("AccurateRip", Length::Fixed(AR_W)));
    }
    let mut rows = Column::new()
        .spacing(2)
        .push(container(heads).padding(Padding::from([metrics::space_xs(), metrics::space_sm()])));
    for (i, track) in job.tracks.iter().enumerate() {
        rows = rows.push(track_row(state, track, job.current == Some(i), narrow));
    }
    rows.into()
}

fn track_row<'a>(
    state: &'a Listen,
    track: &'a TrackProgress,
    current: bool,
    narrow: bool,
) -> Element<'a, Message> {
    let t = state.tokens;
    let size = metrics::font_sm();
    let (state_text, state_color): (String, Color) = match track.state {
        TrackState::Tagged => (format!("{} ✓", crate::i18n::tr("Tagged")), t.success),
        TrackState::Failed => (crate::i18n::tr("Failed").to_string(), t.error),
        TrackState::Reading | TrackState::Verifying | TrackState::Encoding => {
            (crate::i18n::tr(track.state.label()).to_string(), t.foreground)
        }
        s => (crate::i18n::tr(s.label()).to_string(), t.muted),
    };
    let progress: Element<'_, Message> = match track.state {
        TrackState::Failed | TrackState::Skipped => iced::widget::space::horizontal().into(),
        _ => rip_panel::bar(state, track.percent as f32),
    };
    let (ar_text, ar_color): (String, Color) = match track.accuraterip {
        Some(TrackAr::Verified(c)) => (format!("✓ {c}"), t.success),
        Some(TrackAr::Mismatch) => (crate::i18n::tr("Mismatch").to_string(), WARN),
        Some(TrackAr::NotInDb) => (crate::i18n::tr("Not in database").to_string(), t.muted),
        None => (String::new(), t.muted),
    };
    let weight = if current { Weight::Semibold } else { Weight::Normal };
    let title = wrap(super::data_label_w(track.display_title(), size, t.foreground, weight));
    let state_label = data_label(state_text, size, state_color).wrapping(Wrapping::WordOrGlyph);
    let ar = data_label(ar_text, size, ar_color).wrapping(Wrapping::WordOrGlyph);
    let mut cells =
        row![container(data_label(format!("{:02}", track.number), size, t.muted)).width(NUMBER_W)]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center);
    let length =
        container(data_label(clock(Duration::from_millis(track.length_ms)), size, t.muted)).width(LENGTH_W);
    if narrow {
        let mut state_cell = column![state_label].spacing(2);
        if track.accuraterip.is_some() {
            state_cell = state_cell.push(ar);
        }
        cells = cells
            .push(column![title, progress].spacing(metrics::space_xs()).width(Length::FillPortion(3)))
            .push(length)
            .push(container(state_cell).width(STATE_W));
    } else {
        cells = cells
            .push(container(title).width(Length::FillPortion(3)))
            .push(length)
            .push(container(state_label).width(STATE_W))
            .push(container(progress).width(Length::FillPortion(1)))
            .push(container(ar).width(AR_W));
    }
    container(cells)
        .padding(Padding::from([metrics::space_xs(), metrics::space_sm()]))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: current.then_some(iced::Background::Color(t.nav_hover)),
            border: iced::Border { radius: metrics::radius_sm().into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// The last lines the ripper reported, newest at the bottom.
fn log<'a>(state: &'a Listen, job: &'a RipJob) -> Element<'a, Message> {
    let t = state.tokens;
    let size = metrics::font_xs();
    let mut lines = Column::new().spacing(2).width(Length::Fill);
    for (at, line, warning) in &job.log {
        let text = if *warning {
            data_label(line.as_str(), size, WARN)
        } else {
            label(line.as_str(), size, t.foreground)
        };
        lines = lines.push(
            row![data_label(clock(*at), size, t.muted).width(LENGTH_W), wrap(text)]
                .spacing(metrics::space_sm()),
        );
    }
    column![
        label_w("Log", metrics::font_sm(), t.foreground, Weight::Semibold),
        container(
            scrollable(container(lines).padding(metrics::space_sm()))
                .anchor_bottom()
                .height(Length::Fixed(LOG_LINES * (size * 1.3 + 2.0) + 2.0 * metrics::space_sm())),
        )
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.background)),
            border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_sm().into() },
            ..Default::default()
        }),
    ]
    .spacing(metrics::space_xs())
    .into()
}
