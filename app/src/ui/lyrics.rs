//! The lyrics view (Feishin's `features/lyrics`): highlighted lines, karaoke words, translation and
//! romaji, the controls below, and the search / export / display-settings dialogs.

use crate::app::lyrics_state::{EXPORT_MODAL, LyricsMsg, SEARCH_MODAL, SETTINGS_MODAL};
use crate::app::{Listen, Message};
use crate::style::{self, Tokens, metrics};
use crate::ui::{self, label, label_w};
use iced::alignment::Horizontal;
use iced::font::Weight;
use iced::widget::{
    Column, Row, button, column, container, pick_list, rich_text, row, scrollable, space, span, text_input,
    toggler,
};
use iced::{Alignment, Color, Element, Font, Length, Padding};
use listen_core::model::{LyricLine, Lyrics};
use listen_core::settings::LyricsSettings;

pub const LYRICS_ID: &str = "fullscreen-lyrics";
/// Fallback size before the scrollable reported its own.
const DEFAULT_VIEWPORT: iced::Size = iced::Size::new(640.0, 420.0);
/// Line height factor of the lyric text.
const LINE_HEIGHT: f32 = 1.2;
/// Translation and romaji are smaller than the line they belong to.
const SUB_SCALE: f32 = 0.62;

fn msg(m: LyricsMsg) -> Message {
    Message::Lyrics(m)
}

// ---- layout maths (shared by the view and the follow scrolling) ------------------

fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11FF | 0x2E80..=0xA4CF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE6F | 0xFF00..=0xFF60)
}

/// How many rows `text` takes at `size` in `width` px (a guess; iced does the real wrapping).
pub fn wrapped_rows(text: &str, size: f32, width: f32) -> f32 {
    let w: f32 = text.chars().map(|c| if is_wide(c) { 1.0 } else { 0.52 }).sum::<f32>() * size;
    (w / width.max(80.0)).ceil().max(1.0)
}

struct Metrics {
    size: f32,
    gap: f32,
    /// Width available to the text.
    width: f32,
    viewport: iced::Size,
}

fn metrics_of(state: &Listen, synced: bool) -> Metrics {
    let s = &state.config.settings.lyrics;
    let viewport = state.lyrics_ui.viewport.unwrap_or(DEFAULT_VIEWPORT);
    let pad = side_padding(s.padding_left, viewport.width) + side_padding(s.padding_right, viewport.width);
    Metrics {
        size: if synced { s.font_size } else { s.font_size_unsync },
        gap: if synced { s.gap } else { s.gap_unsync },
        width: viewport.width - pad - 2.0 * SIDE,
        viewport,
    }
}

/// Fixed side margin of every line.
const SIDE: f32 = 16.0;

fn side_padding(percent: f32, width: f32) -> f32 {
    width * percent / 100.0
}

fn sub_size(size: f32) -> f32 {
    (size * SUB_SCALE).max(11.0)
}

fn line_size(s: &LyricsSettings, m: &Metrics, synced: bool, active: bool) -> f32 {
    if !synced || active { m.size } else { (m.size * s.scale_non_active).max(8.0) }
}

/// Height of line `i` with its sub-lines, when line `active` is the sung one.
fn line_height(state: &Listen, lyrics: &Lyrics, m: &Metrics, i: usize, active: Option<usize>) -> f32 {
    let s = &state.config.settings.lyrics;
    let line = &lyrics.lines[i];
    let size = line_size(s, m, lyrics.synced, !lyrics.synced || active == Some(i));
    let text = if line.text.is_empty() { "♪" } else { line.text.as_str() };
    let mut h = wrapped_rows(text, size, m.width) * size * LINE_HEIGHT;
    if furigana_of(s, text).is_some() {
        h += size * RUBY_SCALE * LINE_HEIGHT;
    }
    let sub = sub_size(size);
    if let Some(t) = state.lyrics_ui.translation_of(lyrics, i) {
        h += 2.0 + wrapped_rows(t, sub, m.width) * sub * LINE_HEIGHT;
    }
    if romaji_of(s, line).is_some() {
        h += 2.0 + wrapped_rows(&line.text, sub, m.width) * sub * LINE_HEIGHT;
    }
    h
}

/// Header lines above the lyrics (provider, match): count.
fn header_count(s: &LyricsSettings, lyrics: &Lyrics) -> usize {
    usize::from(s.show_provider && !lyrics.provider.is_empty())
        + usize::from(s.show_match && lyrics.matched.is_some())
}

fn top_spacer(m: &Metrics) -> f32 {
    (m.viewport.height * 0.5).max(60.0)
}

/// Scroll offset that puts line `i` where the follow alignment says.
pub fn follow_offset(state: &Listen, i: usize) -> Option<f32> {
    let lyrics = state.lyrics.as_ref()?;
    if i >= lyrics.lines.len() {
        return None;
    }
    let s = &state.config.settings.lyrics;
    let m = metrics_of(state, lyrics.synced);
    let header = header_count(s, lyrics) as f32 * (sub_size(m.size) * LINE_HEIGHT + m.gap);
    let mut y = top_spacer(&m) + m.gap + header;
    for j in 0..i {
        y += line_height(state, lyrics, &m, j, Some(i)) + m.gap;
    }
    let h = line_height(state, lyrics, &m, i, Some(i));
    let anchor = (0.5 + s.follow_scroll_alignment as f32 / 100.0).clamp(0.0, 1.0);
    Some((y + h / 2.0 - m.viewport.height * anchor).max(0.0))
}

// ---- text helpers --------------------------------------------------------------

fn romaji_of(s: &LyricsSettings, line: &LyricLine) -> Option<String> {
    if !s.enable_romaji {
        return None;
    }
    // Kanji are read with the dictionary first (when the build has one).
    let source = listen_lyrics::furigana::reading(&line.text)
        .filter(|_| listen_lyrics::romaji::has_kana(&line.text))
        .unwrap_or_else(|| line.text.clone());
    listen_lyrics::romaji::romanize(&source).filter(|r| *r != line.text)
}

fn align_of(s: &LyricsSettings, text: &str) -> Horizontal {
    let base = match s.alignment.as_str() {
        "left" => Horizontal::Left,
        "right" => Horizontal::Right,
        _ => Horizontal::Center,
    };
    // Right-to-left lines sit on their reading edge instead of the left one.
    if base == Horizontal::Left && listen_lyrics::rtl::is_rtl(text) { Horizontal::Right } else { base }
}

fn faded(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

/// Ruby text is this fraction of the line's size.
const RUBY_SCALE: f32 = 0.45;

/// The line's furigana segments when the option is on and the line has any.
fn furigana_of(s: &LyricsSettings, text: &str) -> Option<Vec<listen_lyrics::furigana::Segment>> {
    if !s.enable_furigana {
        return None;
    }
    listen_lyrics::furigana::segments(text)
}

/// A line with furigana: one column per segment (reading over text), wrapped like words.
fn ruby_line<'a>(
    segments: Vec<listen_lyrics::furigana::Segment>,
    size: f32,
    color: Color,
    align: Horizontal,
) -> Element<'a, Message> {
    let font = ui::font(Weight::Semibold);
    let ruby_size = (size * RUBY_SCALE).max(8.0);
    let mut r = Row::new();
    for seg in segments {
        let ruby: Element<'a, Message> = match &seg.ruby {
            Some(reading) => ui::label_w(reading.clone(), ruby_size, faded(color, 0.85), Weight::Medium)
                .line_height(LINE_HEIGHT)
                .into(),
            None => space().height(ruby_size * LINE_HEIGHT).into(),
        };
        let base = ui::text_in(seg.text.clone(), ui::font_for(&seg.text, font))
            .size(size)
            .line_height(LINE_HEIGHT)
            .color(color)
            .wrapping(iced::widget::text::Wrapping::None);
        r = r.push(column![ruby, base].align_x(Alignment::Center));
    }
    container(r.wrap().align_x(align)).width(Length::Fill).align_x(align).into()
}

/// One lyric line: plain, or with its words lit up as they are sung.
fn line_text<'a>(
    state: &'a Listen,
    line: &'a LyricLine,
    size: f32,
    color: Color,
    align: Horizontal,
    karaoke: bool,
) -> Element<'a, Message> {
    let font = ui::font(Weight::Semibold);
    if let Some(segments) = furigana_of(&state.config.settings.lyrics, &line.text) {
        return ruby_line(segments, size, color, align);
    }
    if karaoke && !line.words.is_empty() {
        let pos = state.lyrics_ui.pos_ms;
        let spans: Vec<_> = line
            .words
            .iter()
            .map(|w| {
                let sung = w.start.as_millis() as u64 <= pos;
                span::<(), Font>(w.text.clone()).font(ui::font_for(&w.text, font)).color(if sung {
                    color
                } else {
                    faded(color, 0.45)
                })
            })
            .collect();
        return rich_text::<(), Message, _, _>(spans)
            .size(size)
            .line_height(LINE_HEIGHT)
            .width(Length::Fill)
            .align_x(align)
            .into();
    }
    let shown = if line.text.is_empty() { "♪" } else { line.text.as_str() };
    ui::text_in(shown, font)
        .size(size)
        .line_height(LINE_HEIGHT)
        .color(color)
        .width(Length::Fill)
        .align_x(align)
        .into()
}

fn sub_text<'a>(text: String, size: f32, color: Color, align: Horizontal) -> Element<'a, Message> {
    ui::text_in(text, ui::font(Weight::Normal))
        .size(sub_size(size))
        .line_height(LINE_HEIGHT)
        .color(color)
        .width(Length::Fill)
        .align_x(align)
        .into()
}

// ---- the view -------------------------------------------------------------------

/// The lyrics with their controls (what the full-screen Lyrics tab and the panels show).
pub fn view(state: &Listen) -> Element<'_, Message> {
    view_in(state, LYRICS_ID)
}

/// [`view`] with its own scroll id, so the sidebar panel can coexist with the full-screen one.
pub fn view_in<'a>(state: &'a Listen, id: &'static str) -> Element<'a, Message> {
    column![body(state, id), controls(state)].height(Length::Fill).into()
}

fn body<'a>(state: &'a Listen, id: &'static str) -> Element<'a, Message> {
    let t = state.tokens;
    let Some(lyrics) = &state.lyrics else {
        let text =
            if state.lyrics_loading { "Looking for lyrics…" } else { "No lyrics found for this song." };
        return container(label(text, metrics::font_lg(), t.muted)).center(Length::Fill).into();
    };
    let s = &state.config.settings.lyrics;
    let m = metrics_of(state, lyrics.synced);
    let karaoke = listen_lyrics::has_word_sync(lyrics);
    let left = side_padding(s.padding_left, m.viewport.width) + SIDE;
    let right = side_padding(s.padding_right, m.viewport.width) + SIDE;
    let mut col = Column::new().spacing(m.gap).width(Length::Fill);
    col = col.push(space().height(Length::Fixed(top_spacer(&m))));
    let head_align = match s.alignment.as_str() {
        "left" => Horizontal::Left,
        "right" => Horizontal::Right,
        _ => Horizontal::Center,
    };
    let header = |text: String| {
        container(sub_text(text, m.size, t.muted, head_align))
            .padding(Padding::from([0.0, 0.0]).left(left).right(right))
    };
    if s.show_provider && !lyrics.provider.is_empty() {
        col = col.push(header(lyrics.provider.clone()));
    }
    if s.show_match
        && let Some(matched) = &lyrics.matched
    {
        col = col.push(header(matched.clone()));
    }
    let padding = Padding::from([0.0, 0.0]).left(left).right(right);
    for (i, line) in lyrics.lines.iter().enumerate() {
        let active = !lyrics.synced || state.lyric_active == Some(i);
        let size = line_size(s, &m, lyrics.synced, active);
        let alpha = if active { 1.0 } else { s.opacity_non_active };
        let color = faded(t.foreground, alpha);
        let align = align_of(s, &line.text);
        let mut block = Column::new().spacing(2).width(Length::Fill).push(line_text(
            state,
            line,
            size,
            color,
            align,
            karaoke && active && lyrics.synced,
        ));
        if let Some(r) = romaji_of(s, line) {
            block = block.push(sub_text(r, size, faded(t.foreground, alpha * 0.8), align));
        }
        if let Some(tr) = state.lyrics_ui.translation_of(lyrics, i) {
            block = block.push(sub_text(tr.to_string(), size, faded(t.foreground, alpha * 0.8), align));
        }
        let el: Element<'_, Message> = match (lyrics.synced, line.start) {
            (true, Some(start)) => {
                // Seeking to a line: the offset moves the lyric clock, so undo it.
                let offset = state.lyrics_ui.offset_ms(s) as f32 / 1000.0;
                button(block)
                    .padding(padding)
                    .style(style::bare_button(t))
                    .on_press(Message::Seek((start.as_secs_f32() - offset).max(0.0)))
                    .into()
            }
            _ => container(block).padding(padding).into(),
        };
        col = col.push(el);
    }
    col = col.push(space().height(Length::Fixed(top_spacer(&m).max(200.0))));
    scrollable(col)
        .id(iced::widget::Id::new(id))
        .on_scroll(|v| msg(LyricsMsg::Viewport(v.bounds().size())))
        .style(style::scrollbar(t))
        .height(Length::Fill)
        .into()
}

fn text_button<'a>(t: Tokens, text: &'static str, m: Option<Message>) -> Element<'a, Message> {
    button(label_w(
        text,
        metrics::font_sm(),
        if m.is_some() { t.foreground } else { t.muted },
        Weight::Semibold,
    ))
    .padding(Padding::from([4.0, 10.0]))
    .style(style::bare_button(t))
    .on_press_maybe(m)
    .into()
}

fn step_button<'a>(t: Tokens, text: &'static str, delta: i32, enabled: bool) -> Element<'a, Message> {
    button(label_w(text, metrics::font_lg(), t.foreground, Weight::Bold))
        .padding(Padding::from([2.0, 10.0]))
        .style(style::bare_button(t))
        .on_press_maybe(enabled.then(|| msg(LyricsMsg::OffsetStep(delta))))
        .into()
}

/// The buttons under the lyrics.
fn controls(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings.lyrics;
    let ui = &state.lyrics_ui;
    let has_song = state.queue.current().is_some();
    let has_lyrics = state.lyrics.as_ref().is_some_and(|l| !l.lines.is_empty());
    let sources = !s.sources.is_empty();

    // Top row: export, version, translation, settings.
    let mut top = row![].spacing(metrics::space_xs()).align_y(Alignment::Center);
    if has_lyrics {
        top = top.push(text_button(t, "EXPORT", Some(msg(LyricsMsg::ExportOpen))));
    }
    if ui.candidates.len() > 1 {
        let names: Vec<VersionChoice> = ui
            .candidates
            .iter()
            .enumerate()
            .map(|(i, l)| VersionChoice {
                index: i,
                label: format!("{} ({})", l.provider, if l.synced { "synced" } else { "plain" }),
            })
            .collect();
        let selected = names.get(ui.selected).cloned();
        top = top.push(
            pick_list(names, selected, |c| msg(LyricsMsg::Version(c.index)))
                .text_size(metrics::font_sm())
                .padding(Padding::from([3.0, 8.0])),
        );
    }
    let translatable = state.lyrics.as_ref().is_some_and(|l| ui.has_translation(l))
        || (!s.translation_api_provider.is_empty() && !s.translation_api_key.trim().is_empty());
    if has_lyrics && translatable {
        let (text, on) =
            if ui.translating { ("TRANSLATING…", false) } else { ("TRANSLATION", ui.show_translation) };
        let color = if on { t.primary } else { t.foreground };
        top = top.push(crate::ui::tip::tip_plain(
            t,
            button(label_w(text, metrics::font_sm(), color, Weight::Semibold))
                .padding(Padding::from([4.0, 10.0]))
                .style(style::bare_button(t))
                .on_press_maybe((!ui.translating).then(|| msg(LyricsMsg::Translation))),
            if on { "Hide the translation" } else { "Show the translation" },
            crate::ui::tip::Position::Top,
        ));
    }
    top = top.push(crate::ui::tip::tip_plain(
        t,
        text_button(t, "DISPLAY", Some(msg(LyricsMsg::SettingsOpen))),
        "Lyrics display settings",
        crate::ui::tip::Position::Top,
    ));

    // Bottom row: search, offset, clear / refresh.
    let offset = ui.offset_ms(s);
    let shown = ui.offset_text.clone().unwrap_or_else(|| offset.to_string());
    let mut bottom = row![].spacing(metrics::space_xs()).align_y(Alignment::Center);
    if sources {
        bottom = bottom.push(crate::ui::tip::tip_plain(
            t,
            text_button(t, "SEARCH", has_song.then(|| msg(LyricsMsg::SearchOpen))),
            "Search lyrics",
            crate::ui::tip::Position::Top,
        ));
    }
    bottom = bottom
        .push(crate::ui::tip::tip_plain(
            t,
            step_button(t, "−", -50, has_song),
            "Lyrics earlier (−50 ms)",
            crate::ui::tip::Position::Top,
        ))
        .push(
            text_input(crate::i18n::tr("0"), &shown)
                .on_input(|v| msg(LyricsMsg::OffsetInput(v)))
                .size(metrics::font_sm())
                .align_x(Horizontal::Center)
                .width(70)
                .style(style::input(t)),
        )
        .push(crate::ui::tip::tip_plain(
            t,
            step_button(t, "+", 50, has_song),
            "Lyrics later (+50 ms)",
            crate::ui::tip::Position::Top,
        ));
    if sources {
        let (text, m) =
            if has_lyrics { ("CLEAR", LyricsMsg::Clear) } else { ("REFRESH", LyricsMsg::Refresh) };
        bottom = bottom.push(crate::ui::tip::tip_plain(
            t,
            text_button(t, text, has_song.then(|| msg(m))),
            if has_lyrics { "Clear the lyrics" } else { "Fetch the lyrics" },
            crate::ui::tip::Position::Top,
        ));
    }
    container(column![top, bottom].spacing(metrics::space_xs()).align_x(Alignment::Center))
        .width(Length::Fill)
        .padding(Padding::from([6.0, 8.0]))
        .center_x(Length::Fill)
        .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionChoice {
    index: usize,
    label: String,
}

impl std::fmt::Display for VersionChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

// ---- dialogs ----------------------------------------------------------------------

pub fn is_modal(key: &str) -> bool {
    matches!(key, SEARCH_MODAL | EXPORT_MODAL | SETTINGS_MODAL)
}

fn action<'a>(t: Tokens, text: &'static str, m: Option<Message>, primary: bool) -> Element<'a, Message> {
    let b = button(label_w(text, metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(Padding::from([6.0, 16.0]))
        .on_press_maybe(m);
    if primary { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) }.into()
}

/// The panel of an open lyrics dialog.
pub fn panel<'a>(state: &'a Listen, key: &str) -> Option<Element<'a, Message>> {
    let t = state.tokens;
    let (title, body, actions, width) = match key {
        SEARCH_MODAL => {
            let (body, actions) = search_dialog(state)?;
            ("Search lyrics", body, actions, 640.0)
        }
        EXPORT_MODAL => {
            let (body, actions) = export_dialog(state)?;
            ("Export lyrics", body, actions, 560.0)
        }
        SETTINGS_MODAL => {
            let rows = crate::ui::lyrics_settings::display_rows(state, &|name, hint, control| {
                row![
                    column![
                        label_w(name, metrics::font_md(), t.foreground, Weight::Medium),
                        label(hint, metrics::font_xs(), t.muted).wrapping(iced::widget::text::Wrapping::Word),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    control
                ]
                .spacing(metrics::space_md())
                .align_y(Alignment::Center)
                .into()
            });
            let body = scrollable(
                Column::with_children(rows).spacing(metrics::space_sm()).padding(Padding::ZERO.right(12)),
            )
            .style(style::scrollbar(t))
            .height(Length::Fixed(420.0));
            let close = action(
                t,
                "Close",
                Some(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal)),
                false,
            );
            ("Lyrics display", body.into(), vec![close], 560.0)
        }
        _ => return None,
    };
    Some(crate::ui::overlay::modal_panel_sized(t, title, body, actions, width))
}

fn search_dialog(state: &Listen) -> Option<(Element<'_, Message>, Vec<Element<'_, Message>>)> {
    let t = state.tokens;
    let s = state.lyrics_ui.search.as_ref()?;
    let inputs = row![
        text_input(crate::i18n::tr("Title"), &s.name)
            .font(crate::ui::font_for(&s.name, crate::appearance::current_font()))
            .on_input(|v| msg(LyricsMsg::SearchName(v)))
            .on_submit(msg(LyricsMsg::SearchRun))
            .style(style::input(t))
            .width(Length::Fill),
        text_input(crate::i18n::tr("Artist"), &s.artist)
            .font(crate::ui::font_for(&s.artist, crate::appearance::current_font()))
            .on_input(|v| msg(LyricsMsg::SearchArtist(v)))
            .on_submit(msg(LyricsMsg::SearchRun))
            .style(style::input(t))
            .width(Length::Fill),
    ]
    .spacing(metrics::space_sm());
    let mut list = Column::new().spacing(2);
    if s.loading {
        list = list.push(label("Searching…", metrics::font_md(), t.muted));
    } else if s.hits.is_empty() {
        let text = if s.searched { "Nothing found." } else { "Press Enter to search." };
        list = list.push(label(text, metrics::font_md(), t.muted));
    }
    for (i, hit) in s.hits.iter().enumerate() {
        let sync = match hit.synced {
            Some(true) => "synchronized",
            Some(false) => "unsynchronized",
            None => "unknown",
        };
        let id = if hit.source == listen_lyrics::providers::Source::Genius {
            hit.id.trim_start_matches("https://").trim_start_matches("genius.com").to_string()
        } else {
            hit.id.clone()
        };
        let pct = ((1.0 - hit.score) * 100.0).clamp(0.0, 100.0);
        let selected = s.selected == Some(i);
        let content = row![
            column![
                label_w(ui::ellipsize(&hit.name, 56), metrics::font_md(), t.foreground, Weight::Semibold),
                label(ui::ellipsize(&hit.artist, 56), metrics::font_sm(), t.muted),
                label(
                    ui::ellipsize(&format!("{} — {} — {}", hit.source.name(), id, sync), 70),
                    metrics::font_xs(),
                    t.muted
                ),
            ]
            .width(Length::Fill),
            label(format!("{pct:.0}%"), metrics::font_md(), t.foreground),
        ]
        .align_y(Alignment::Center);
        list = list.push(
            button(content)
                .width(Length::Fill)
                .padding(Padding::from([6.0, 10.0]))
                .style(style::row_button(t, selected))
                .on_press(msg(LyricsMsg::SearchPick(i))),
        );
    }
    let results = scrollable(list).style(style::scrollbar(t)).height(Length::Fixed(200.0));
    let preview: Element<'_, Message> = match (&s.preview, s.preview_loading) {
        (_, true) => label("Loading the lyrics…", metrics::font_sm(), t.muted).into(),
        (Some(l), _) => {
            let text: String =
                l.lines.iter().take(60).map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
            scrollable(ui::text_in(text, ui::font(Weight::Normal)).size(metrics::font_sm()).color(t.muted))
                .style(style::scrollbar(t))
                .height(Length::Fixed(130.0))
                .width(Length::Fill)
                .into()
        }
        _ => space().height(Length::Fixed(0.0)).into(),
    };
    let body = column![inputs, results, preview].spacing(metrics::space_md()).into();
    let close = action(t, "Close", Some(msg(LyricsMsg::SearchClose)), false);
    let search = action(t, "Search", Some(msg(LyricsMsg::SearchRun)), false);
    let export = action(t, "Export", s.preview.is_some().then(|| msg(LyricsMsg::SearchExport)), false);
    let apply = action(t, "Apply", s.selected.map(|_| msg(LyricsMsg::SearchApply)), true);
    Some((body, vec![close, search, export, apply]))
}

fn export_dialog(state: &Listen) -> Option<(Element<'_, Message>, Vec<Element<'_, Message>>)> {
    let t = state.tokens;
    let e = state.lyrics_ui.export.as_ref()?;
    let mut options = row![].spacing(metrics::space_lg()).align_y(Alignment::Center);
    if e.lyrics.synced {
        options = options.push(
            row![
                toggler(e.synced).on_toggle(|v| msg(LyricsMsg::ExportSynced(v))).size(20),
                label("Synchronized (.lrc)", metrics::font_md(), t.foreground),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
        if e.synced {
            options = options.push(
                row![
                    label("Offset (ms)", metrics::font_md(), t.foreground),
                    text_input(crate::i18n::tr("0"), &e.offset_text)
                        .on_input(|v| msg(LyricsMsg::ExportOffset(v)))
                        .style(style::input(t))
                        .width(90),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center),
            );
        }
    }
    let preview = scrollable(
        container(ui::text_in(e.text(), Font::MONOSPACE).size(metrics::font_sm()).color(t.muted)).padding(8),
    )
    .style(style::scrollbar(t))
    .height(Length::Fixed(300.0))
    .width(Length::Fill);
    let body = column![
        options,
        label(format!("Saved to the Downloads folder as {}", e.file_name()), metrics::font_sm(), t.muted),
        preview
    ]
    .spacing(metrics::space_md())
    .into();
    let close =
        action(t, "Close", Some(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal)), false);
    let save = action(t, "Export", Some(msg(LyricsMsg::ExportSave)), true);
    Some((body, vec![close, save]))
}
