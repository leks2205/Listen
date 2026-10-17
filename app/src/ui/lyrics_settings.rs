//! Lyrics settings controls (Feishin's General -> Lyrics and display form), shared by the settings page
//! and the lyrics view's display dialog, each with its own row layout (`mk`).

use crate::app::lyrics_state::LyricsSetting;
use crate::app::{Listen, Message, Setting};
use crate::style::{self, Tokens, metrics};
use crate::ui::label;
use iced::widget::{Row, button, pick_list, row, slider, text_input, toggler};
use iced::{Alignment, Element, Padding};
use listen_lyrics::providers::Source;
use listen_lyrics::translate::{LANGUAGES, PROVIDERS};

/// Builds one row: (name, hint, control).
pub type Mk<'a, 'b> = &'b dyn Fn(&'static str, &'static str, Element<'a, Message>) -> Element<'a, Message>;

fn set(m: LyricsSetting) -> Message {
    Message::Setting(Setting::Lyrics(m))
}

fn switch<'a>(value: bool, f: fn(bool) -> LyricsSetting) -> Element<'a, Message> {
    toggler(value).on_toggle(move |v| set(f(v))).size(20).into()
}

fn slide<'a>(
    t: Tokens,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    step: f32,
    shown: String,
    f: impl Fn(f32) -> LyricsSetting + 'a,
) -> Element<'a, Message> {
    row![
        slider(range, value, move |v| set(f(v))).step(step).width(160),
        label(shown, metrics::font_md(), t.muted).width(56),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .into()
}

fn alignment<'a>(t: Tokens, current: &str) -> Element<'a, Message> {
    let mut r = Row::new().spacing(metrics::space_xs());
    for (value, text) in [("left", "Left"), ("center", "Center"), ("right", "Right")] {
        let on = current == value;
        r = r.push(
            button(label(text, metrics::font_sm(), if on { t.primary } else { t.foreground }))
                .padding(Padding::from([4.0, 12.0]))
                .style(style::default_button(t))
                .on_press(set(LyricsSetting::Alignment(value.into()))),
        );
    }
    r.into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Lang(&'static str, &'static str);

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

/// The display settings: sizes, gaps, opacity, alignment, follow, match / provider.
pub fn display_rows<'a>(state: &'a Listen, mk: Mk<'a, '_>) -> Vec<Element<'a, Message>> {
    let t = state.tokens;
    let s = &state.config.settings.lyrics;
    let px = |v: f32| format!("{v:.0}px");
    let pct = |v: f32| format!("{:.0}%", v * 100.0);
    vec![
        mk("Alignment", "Where the lyrics sit.", alignment(t, &s.alignment)),
        mk(
            "Lyric size (synchronized)",
            "Font size of lyrics that follow the song.",
            slide(t, 8.0..=72.0, s.font_size, 1.0, px(s.font_size), LyricsSetting::FontSize),
        ),
        mk(
            "Lyric size (unsynchronized)",
            "Font size of lyrics without timing.",
            slide(
                t,
                8.0..=72.0,
                s.font_size_unsync,
                1.0,
                px(s.font_size_unsync),
                LyricsSetting::FontSizeUnsync,
            ),
        ),
        mk(
            "Lyric gap (synchronized)",
            "Space between the lines.",
            slide(t, 0.0..=50.0, s.gap, 1.0, px(s.gap), LyricsSetting::Gap),
        ),
        mk(
            "Lyric gap (unsynchronized)",
            "Space between the lines of untimed lyrics.",
            slide(t, 0.0..=50.0, s.gap_unsync, 1.0, px(s.gap_unsync), LyricsSetting::GapUnsync),
        ),
        mk(
            "Left padding",
            "Empty space left of the lyrics, in percent of the width.",
            slide(
                t,
                0.0..=50.0,
                s.padding_left,
                1.0,
                format!("{:.0}%", s.padding_left),
                LyricsSetting::PaddingLeft,
            ),
        ),
        mk(
            "Right padding",
            "Empty space right of the lyrics, in percent of the width.",
            slide(
                t,
                0.0..=50.0,
                s.padding_right,
                1.0,
                format!("{:.0}%", s.padding_right),
                LyricsSetting::PaddingRight,
            ),
        ),
        mk(
            "Follow the current lyric",
            "Scroll along with the line being sung.",
            switch(s.follow, LyricsSetting::Follow),
        ),
        mk(
            "Follow position",
            "Where the current line sits while following: -50 near the top, 0 centred, 50 near the bottom.",
            slide(
                t,
                -50.0..=50.0,
                s.follow_scroll_alignment as f32,
                1.0,
                s.follow_scroll_alignment.to_string(),
                |v| LyricsSetting::FollowAlignment(v.round() as i32),
            ),
        ),
        mk(
            "Line lead time",
            "A line lights up this many milliseconds before its time.",
            slide(
                t,
                0.0..=3000.0,
                s.line_lead_time_ms as f32,
                50.0,
                format!("{} ms", s.line_lead_time_ms),
                |v| LyricsSetting::LeadMs(v.round() as u32),
            ),
        ),
        mk(
            "Opacity of other lines",
            "How visible the lines that are not being sung are.",
            slide(
                t,
                0.0..=1.0,
                s.opacity_non_active,
                0.01,
                pct(s.opacity_non_active),
                LyricsSetting::Opacity,
            ),
        ),
        mk(
            "Size of other lines",
            "Size of the lines that are not being sung, compared to the current one.",
            slide(t, 0.5..=1.0, s.scale_non_active, 0.01, pct(s.scale_non_active), LyricsSetting::Scale),
        ),
        mk(
            "Show the match",
            "Show which song the provider matched.",
            switch(s.show_match, LyricsSetting::ShowMatch),
        ),
        mk(
            "Show the provider",
            "Show where the lyrics came from.",
            switch(s.show_provider, LyricsSetting::ShowProvider),
        ),
    ]
}

/// The provider list: switch, name and order buttons for each.
fn providers<'a>(t: Tokens, s: &listen_core::settings::LyricsSettings) -> Element<'a, Message> {
    let enabled = Source::parse_list(&s.sources);
    let off = Source::ALL.into_iter().filter(|p| !enabled.contains(p));
    let mut col = iced::widget::Column::new().spacing(metrics::space_xs());
    let count = enabled.len();
    for (i, source) in enabled.iter().copied().chain(off).enumerate() {
        let on = i < count;
        let name = source.name();
        let arrow = |text: &'static str, by: i32, ok: bool| {
            button(label(text, metrics::font_sm(), if ok { t.foreground } else { t.muted }))
                .padding(Padding::from([2.0, 8.0]))
                .style(style::default_button(t))
                .on_press_maybe(ok.then(|| set(LyricsSetting::MoveSource(name.into(), by))))
        };
        col = col.push(
            row![
                toggler(on).on_toggle(move |v| set(LyricsSetting::Source(name.into(), v))).size(18),
                label(name, metrics::font_md(), if on { t.foreground } else { t.muted }).width(100),
                arrow("▲", -1, on && i > 0),
                arrow("▼", 1, on && i + 1 < count),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

/// Every lyrics setting of the Playback tab, in Feishin's order.
pub fn rows<'a>(state: &'a Listen, mk: Mk<'a, '_>, fetch: Element<'a, Message>) -> Vec<Element<'a, Message>> {
    let t = state.tokens;
    let s = &state.config.settings.lyrics;
    let mut v = vec![
        mk("Fetch lyrics", "From the server, YouTube Music or the providers below.", fetch),
        mk(
            "Prefer local lyrics",
            "Use the server's or YouTube Music's own lyrics before the providers'. Plain ones still give way to synchronized lyrics from a provider.",
            switch(s.prefer_local, LyricsSetting::PreferLocal),
        ),
        mk(
            "Lyric providers",
            "Which providers are asked, and in which order (the first with synchronized lyrics wins).",
            providers(t, s),
        ),
        mk(
            "Lyric offset",
            "Default shift of the lyrics against the music in milliseconds (the lyrics view can change it per song).",
            slide(t, -2000.0..=2000.0, s.delay_ms as f32, 10.0, format!("{} ms", s.delay_ms), |v| {
                LyricsSetting::DelayMs(v.round() as i32)
            }),
        ),
        mk(
            "Romaji",
            if listen_lyrics::furigana::AVAILABLE {
                "Show Japanese lyrics in Latin letters under the line (kanji are read with the built-in dictionary)."
            } else {
                "Show Japanese kana lyrics in Latin letters under the line (kanji stay as they are: this build has no dictionary)."
            },
            switch(s.enable_romaji, LyricsSetting::Romaji),
        ),
        mk(
            "Furigana",
            if listen_lyrics::furigana::AVAILABLE {
                "Show the reading of the kanji above them in Japanese lyrics (built-in dictionary)."
            } else {
                "Not in this build: it was made without the Japanese dictionary (cargo feature `furigana`)."
            },
            if listen_lyrics::furigana::AVAILABLE {
                switch(s.enable_furigana, LyricsSetting::Furigana)
            } else {
                switch(false, |_| LyricsSetting::Furigana(false))
            },
        ),
        mk(
            "NetEase translations",
            "Fetch NetEase's own translation and show it under each line.",
            switch(s.enable_netease_translation, LyricsSetting::NeteaseTranslation),
        ),
    ];
    let lang = Lang(
        LANGUAGES.iter().find(|(c, _)| *c == s.translation_target_language).map_or("en", |l| l.0),
        LANGUAGES.iter().find(|(c, _)| *c == s.translation_target_language).map_or("English", |l| l.1),
    );
    v.push(mk(
        "Translation language",
        "The language online translation translates into.",
        pick_list(LANGUAGES.iter().map(|(c, n)| Lang(c, n)).collect::<Vec<_>>(), Some(lang), |l| {
            set(LyricsSetting::TranslationLanguage(l.0.into()))
        })
        .into(),
    ));
    let mut providers_list: Vec<String> = vec!["None".into()];
    providers_list.extend(PROVIDERS.iter().map(|p| p.to_string()));
    let current = if s.translation_api_provider.is_empty() {
        "None".to_string()
    } else {
        s.translation_api_provider.clone()
    };
    v.push(mk(
        "Translation provider",
        "The online service for the Translation button (needs your own API key).",
        pick_list(providers_list, Some(current), |p| {
            set(LyricsSetting::TranslationProvider(if p == "None" { String::new() } else { p }))
        })
        .into(),
    ));
    v.push(mk(
        "Translation API key",
        "The key of the provider above. It is stored in Listen's config file.",
        text_input(crate::i18n::tr("API key"), &s.translation_api_key)
            .secure(true)
            .on_input(|k| set(LyricsSetting::TranslationKey(k)))
            .style(style::input(t))
            .width(260)
            .into(),
    ));
    v.push(mk(
        "Automatic translation",
        "Translate every song's lyrics when they load (needs the provider and key).",
        switch(s.enable_auto_translation, LyricsSetting::AutoTranslation),
    ));
    v.extend(display_rows(state, mk));
    v
}
