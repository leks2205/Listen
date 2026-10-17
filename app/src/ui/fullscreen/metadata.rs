//! Title, artists, album (links) and metadata badges under the cover
//! (Feishin's `shared-full-screen-player-metadata.tsx`).

use crate::app::{Listen, Message};
use crate::style::metrics;
use crate::ui::{label, song_links};
use iced::alignment::Horizontal;
use iced::font::Weight;
use iced::widget::{Row, column, container};
use iced::{Element, Length};
use listen_core::model::Song;
use listen_core::settings::TitleDisplay;

const TITLE: f32 = 32.0;

/// Scroll id of the one-line title (the marquee drives it).
pub const TITLE_ID: &str = "fs-title-marquee";
/// Marquee speed in px per second, and the rest at both ends in seconds.
const MARQUEE_SPEED: f32 = 45.0;
const MARQUEE_REST: f32 = 2.0;

/// The width the title gets under the cover: the cover's width, at least 240.
pub fn title_width(window: iced::Size, cover_pct: u8, panel_open: bool) -> f32 {
    let w = if panel_open { (window.width - 48.0) / 2.0 } else { window.width };
    let art = (w * f32::from(cover_pct) / 100.0).min((window.height - 190.0).max(0.0)).max(120.0).min(w);
    art.max(240.0).min(w)
}

/// How far the title is cut off in `width` px (0: it fits, no marquee).
pub fn overflow_px(title: &str, width: f32) -> f32 {
    (crate::ui::text_px(title, TITLE) * 1.04 - width).max(0.0)
}

/// Relative scroll position (0..1) `t` seconds into the marquee of a title
/// that overflows by `overflow` px: rest, scroll to the end, rest, back.
pub fn marquee_pos(t: f32, overflow: f32) -> f32 {
    if overflow <= 0.0 {
        return 0.0;
    }
    let travel = overflow / MARQUEE_SPEED;
    let period = 2.0 * (MARQUEE_REST + travel);
    let t = t.rem_euclid(period);
    if t < MARQUEE_REST {
        0.0
    } else if t < MARQUEE_REST + travel {
        (t - MARQUEE_REST) / travel
    } else if t < 2.0 * MARQUEE_REST + travel {
        1.0
    } else {
        1.0 - (t - 2.0 * MARQUEE_REST - travel) / travel
    }
}

/// The badge texts for the badge ids in `ids` (in that order). Ids the song
/// has no data for are skipped, as is `release_year` next to `year`.
pub fn badges(song: &Song, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    for id in ids {
        match id.as_str() {
            "codec" => {
                if let Some(c) = song.codec.as_deref().or(song.suffix.as_deref()).filter(|c| !c.is_empty()) {
                    push(c.to_uppercase());
                }
            }
            "bit_rate" => {
                song.bit_rate.filter(|b| *b > 0).into_iter().for_each(|b| push(format!("{b} kbps")))
            }
            "bit_depth" => {
                song.bit_depth.filter(|b| *b > 0).into_iter().for_each(|b| push(format!("{b} bit")))
            }
            "sample_rate" => {
                if let Some(r) = song.sample_rate.filter(|r| *r > 0) {
                    push(format!("{} kHz", (f64::from(r) / 1000.0 * 10.0).round() / 10.0));
                }
            }
            "bpm" => song.bpm.filter(|b| *b > 0).into_iter().for_each(|b| push(format!("{b} BPM"))),
            // The model has a single year for all four year / date badges.
            "year" | "release_year" | "date" | "release_date" => {
                song.year.filter(|y| *y > 0).into_iter().for_each(|y| push(y.to_string()));
            }
            "genres" => song.genres.iter().take(2).for_each(|g| push(g.clone())),
            "disc_number" => song.disc_number.into_iter().for_each(|d| push(format!("Disc {d}"))),
            "track_number" => song.track_number.into_iter().for_each(|n| push(format!("Track {n}"))),
            _ => {}
        }
    }
    out
}

pub fn view<'a>(state: &'a Listen, song: &'a Song, width: f32, align: Horizontal) -> Element<'a, Message> {
    let t = state.tokens;
    let s = &state.config.settings.fullscreen;
    let x = match align {
        Horizontal::Left => iced::Alignment::Start,
        Horizontal::Center => iced::Alignment::Center,
        Horizontal::Right => iced::Alignment::End,
    };
    // One line that scrolls to and fro when too long (the marquee), or wrapped
    // over up to `title_lines` lines.
    let title: Element<'a, Message> = match s.title_display {
        TitleDisplay::Scroll => {
            let line = crate::ui::label_w(song.title.clone(), TITLE, t.foreground, Weight::Black);
            if overflow_px(&song.title, width) > 0.0 {
                iced::widget::scrollable(line)
                    .id(iced::widget::Id::new(TITLE_ID))
                    .direction(iced::widget::scrollable::Direction::Horizontal(
                        iced::widget::scrollable::Scrollbar::new().width(0).scroller_width(0),
                    ))
                    .width(Length::Fill)
                    .into()
            } else {
                line.into()
            }
        }
        TitleDisplay::MultiLine => container(
            crate::ui::text_in(song.title.clone(), crate::ui::font(Weight::Black))
                .size(TITLE)
                .line_height(1.2)
                .color(t.foreground)
                .width(Length::Fill)
                .align_x(align),
        )
        .max_height(TITLE * 1.2 * f32::from(s.title_lines))
        .clip(true)
        .into(),
    };
    let artists = song_links::artists(state, &song.artists, metrics::font_xl(), t.muted, width);
    let album = song_links::album(state, song, metrics::font_xl(), t.muted, width);

    let mut col = column![
        container(title).width(Length::Fill).align_x(align),
        container(artists).width(Length::Fill).align_x(align),
        container(album).width(Length::Fill).align_x(align),
    ]
    .spacing(metrics::space_sm())
    .align_x(x)
    .width(width);

    let texts = badges(song, &state.config.settings.player_bar.info_items);
    if !texts.is_empty() {
        let mut row = Row::new().spacing(metrics::space_sm());
        for text in texts {
            row = row.push(
                container(label(text, metrics::font_sm(), t.foreground))
                    .padding(iced::Padding::from([2.0, 10.0]))
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(crate::style::alpha(t.foreground, 0.12))),
                        border: iced::border::rounded(20),
                        ..Default::default()
                    }),
            );
        }
        col = col.push(container(row.wrap()).width(Length::Fill).align_x(align));
    }
    col.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.codec = Some("flac".into());
        s.bit_rate = Some(1011);
        s.bit_depth = Some(24);
        s.sample_rate = Some(96_000);
        s.bpm = Some(124);
        s.year = Some(1999);
        s.genres = vec!["Rock".into(), "Pop".into(), "Jazz".into()];
        s.disc_number = Some(2);
        s.track_number = Some(5);
        s
    }

    #[test]
    fn marquee_rests_scrolls_and_returns() {
        assert_eq!(marquee_pos(5.0, 0.0), 0.0);
        let over = 90.0; // 2 s of travel
        assert_eq!(marquee_pos(0.0, over), 0.0);
        assert_eq!(marquee_pos(1.9, over), 0.0);
        assert!((marquee_pos(3.0, over) - 0.5).abs() < 1e-3);
        assert_eq!(marquee_pos(4.5, over), 1.0);
        assert!((marquee_pos(7.0, over) - 0.5).abs() < 1e-3);
        // One period is 2 * (2 + 2) = 8 s.
        assert_eq!(marquee_pos(8.5, over), 0.0);
    }

    #[test]
    fn short_titles_do_not_overflow() {
        assert_eq!(overflow_px("Hi", 300.0), 0.0);
        assert!(overflow_px(&"Very long title ".repeat(6), 300.0) > 0.0);
        let w = title_width(iced::Size::new(1600.0, 900.0), 75, true);
        assert!((240.0..=800.0).contains(&w));
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn badge_texts_follow_feishin() {
        let all = ids(&[
            "codec",
            "bit_rate",
            "bit_depth",
            "sample_rate",
            "bpm",
            "year",
            "genres",
            "disc_number",
            "track_number",
        ]);
        assert_eq!(
            badges(&song(), &all),
            ["FLAC", "1011 kbps", "24 bit", "96 kHz", "124 BPM", "1999", "Rock", "Pop", "Disc 2", "Track 5"]
        );
    }

    #[test]
    fn missing_data_and_duplicate_years_are_skipped() {
        let mut s = song();
        s.bit_rate = None;
        s.sample_rate = Some(44_100);
        assert_eq!(
            badges(&s, &ids(&["bit_rate", "sample_rate", "year", "release_year"])),
            ["44.1 kHz", "1999"]
        );
        assert!(badges(&s, &ids(&["nope"])).is_empty());
    }
}
