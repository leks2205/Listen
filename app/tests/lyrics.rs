//! The lyrics view: offset buttons, search / override / clear, translation and
//! karaoke display, export, follow, and the settings.

use iced_test::simulator::Simulator;
use listen::app::lyrics_state::{LyricsMsg, LyricsSetting, SEARCH_MODAL};
use listen::app::{INTER, Listen, Message, Setting};
use listen::{Args, FONTS};
use listen_core::model::Lyrics;
use listen_lyrics::providers::{SearchHit, Source};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

const LRC: &str = "[00:00.50]Neon lights across the bay\n[00:05.00]We drive until the break of day\n[00:10.00]Midnight signal on the air\n[00:15.00]Afterglow is everywhere";

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn boot_with(lyrics: Lyrics) -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::OpenFullscreenTab(listen::app::FullscreenTab::Lyrics));
    let id = s.queue.current().unwrap().song.id.clone();
    let _ = s.update(Message::LyricsLoaded(id, Some(lyrics)));
    s
}

fn boot() -> Listen {
    boot_with(listen_lyrics::parse_lrc(LRC))
}

fn size() -> iced::Size {
    iced::Size::new(1200.0, 800.0)
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), size(), s.view());
    ui.find(text).is_ok()
}

/// Clicks `text` and feeds the resulting messages back.
fn click(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), size(), s.view());
        ui.click(text).unwrap_or_else(|_| panic!("no {text} on screen"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn offset(s: &Listen) -> i32 {
    s.lyrics_ui.offset_ms(&s.config.settings.lyrics)
}

fn hit(source: Source, id: &str, name: &str, artist: &str, synced: bool, score: f32) -> SearchHit {
    SearchHit { source, id: id.into(), artist: artist.into(), name: name.into(), synced: Some(synced), score }
}

#[test]
fn offset_buttons_step_by_50_ms_per_song() {
    let mut s = boot();
    assert_eq!(offset(&s), 0);
    click(&mut s, "+");
    click(&mut s, "+");
    assert_eq!(offset(&s), 100);
    click(&mut s, "−");
    assert_eq!(offset(&s), 50);
    assert!(has_text(&s, "50"));
    // The number input takes typed values, including negative ones.
    let _ = s.update(Message::Lyrics(LyricsMsg::OffsetInput("-".into())));
    assert_eq!(offset(&s), 50, "a lone minus changes nothing yet");
    let _ = s.update(Message::Lyrics(LyricsMsg::OffsetInput("-120".into())));
    assert_eq!(offset(&s), -120);
    // Another song starts from the setting again.
    let next = listen::demo::album_songs(1)[0].clone();
    s.queue.set(vec![next], 0);
    let _ = s.update(Message::Lyrics(LyricsMsg::Refresh));
    assert_eq!(offset(&s), 0);
}

#[test]
fn the_offset_shifts_the_active_line() {
    let mut s = boot();
    let at = |s: &mut Listen, secs: u64| {
        let _ = s.update(Message::Deck(
            0,
            listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(
                std::time::Duration::from_secs(secs),
            )),
        ));
    };
    s.config.settings.lyrics.line_lead_time_ms = 0;
    at(&mut s, 4);
    assert_eq!(s.lyric_active, Some(0));
    for _ in 0..20 {
        let _ = s.update(Message::Lyrics(LyricsMsg::OffsetStep(50)));
    }
    // 4 s + 1 s offset reaches the line at 5 s.
    at(&mut s, 4);
    assert_eq!(s.lyric_active, Some(1));
}

#[test]
fn search_pick_apply_remembers_the_override_and_clear_forgets_it() {
    let mut s = boot();
    click(&mut s, "SEARCH");
    assert!(s.lyrics_ui.search.is_some());
    assert!(s.overlay.modal.is_some());
    let token = s.lyrics_ui.search.as_ref().unwrap().token;
    // Demo mode never calls a provider: feed the results in.
    let hits = vec![
        hit(Source::NetEase, "42", "Neon Horizons", "Demo Artist", true, 0.05),
        hit(Source::Genius, "https://genius.com/x-lyrics", "Neon Horizons", "Demo Artist", false, 0.2),
    ];
    let _ = s.update(Message::Lyrics(LyricsMsg::SearchResults(token, hits)));
    assert!(has_text(&s, "Neon Horizons"));
    assert!(has_text(&s, "95%"));
    // A stale answer is ignored.
    let _ = s.update(Message::Lyrics(LyricsMsg::SearchResults(token + 99, vec![])));
    assert_eq!(s.lyrics_ui.search.as_ref().unwrap().hits.len(), 2);

    let _ = s.update(Message::Lyrics(LyricsMsg::SearchPick(0)));
    assert_eq!(s.lyrics_ui.search.as_ref().unwrap().selected, Some(0));
    let token = s.lyrics_ui.search.as_ref().unwrap().token;
    let picked = listen_lyrics::fetch::lyrics_from_text(
        "[00:01.00]Picked line",
        &hit(Source::NetEase, "42", "Neon Horizons", "Demo Artist", true, 0.05),
    );
    let _ = s.update(Message::Lyrics(LyricsMsg::Preview(token, Some(picked))));
    click(&mut s, "Apply");

    let key = Listen::song_key(&s.queue.current().unwrap().song);
    let o = s.lyrics_ui.override_for(&key).expect("override remembered");
    assert_eq!((o.source.as_str(), o.id.as_str()), ("NetEase", "42"));
    assert_eq!(s.lyrics.as_ref().unwrap().lines[0].text, "Picked line");
    assert_eq!(s.lyrics.as_ref().unwrap().provider, "NetEase");
    assert!(s.overlay.modal.is_none());

    click(&mut s, "CLEAR");
    assert!(s.lyrics_ui.override_for(&key).is_none());
    assert!(s.lyrics.is_none(), "demo mode: nothing else to fall back to");
    // With nothing shown the button reads Refresh.
    assert!(has_text(&s, "REFRESH"));
    assert!(!has_text(&s, "CLEAR"));
}

#[test]
fn search_dialog_prefills_the_song_and_closes() {
    let mut s = boot();
    let _ = s.update(Message::Lyrics(LyricsMsg::SearchOpen));
    let song = s.queue.current().unwrap().song.clone();
    let st = s.lyrics_ui.search.as_ref().unwrap();
    assert_eq!(st.name, song.title);
    assert_eq!(st.artist, song.artists[0].name);
    assert!(matches!(s.overlay.modal, Some(listen::app::overlay::Modal::Custom(SEARCH_MODAL))));
    let _ = s.update(Message::Lyrics(LyricsMsg::SearchName("Other".into())));
    assert_eq!(s.lyrics_ui.search.as_ref().unwrap().name, "Other");
    click(&mut s, "Close");
    assert!(s.lyrics_ui.search.is_none());
    assert!(s.overlay.modal.is_none());
}

#[test]
fn no_provider_means_no_search_or_clear_buttons() {
    let mut s = boot();
    assert!(has_text(&s, "SEARCH") && has_text(&s, "CLEAR"));
    s.config.settings.lyrics.sources.clear();
    assert!(!has_text(&s, "SEARCH") && !has_text(&s, "CLEAR"));
    assert!(has_text(&s, "EXPORT"));
}

#[test]
fn translation_shows_under_the_line_and_toggles() {
    let mut s = boot_with(listen_lyrics::parse_lrc(
        "[00:01.00]Hello_BREAK_Hallo Welt\n[00:05.00]Goodbye_BREAK_Tschüss",
    ));
    assert!(has_text(&s, "Hallo Welt"));
    click(&mut s, "TRANSLATION");
    assert!(!has_text(&s, "Hallo Welt"));
    click(&mut s, "TRANSLATION");
    assert!(has_text(&s, "Tschüss"));
}

#[test]
fn online_translation_needs_a_provider_and_key() {
    let mut s = boot();
    assert!(!has_text(&s, "TRANSLATION"), "no translation, nothing configured");
    s.config.settings.lyrics.translation_api_provider = "Google Cloud".into();
    s.config.settings.lyrics.translation_api_key = "k".into();
    assert!(has_text(&s, "TRANSLATION"));
    // The answer lays over the lines.
    let id = s.queue.current().unwrap().song.id.clone();
    let lines = vec![Some("Eins".to_string()), None, Some("Drei".to_string()), None];
    let _ = s.update(Message::Lyrics(LyricsMsg::Translated(id.clone(), Some(lines))));
    assert!(has_text(&s, "Eins") && has_text(&s, "Drei"));
    // A wrong-sized answer is refused.
    let _ = s.update(Message::Lyrics(LyricsMsg::Translated(id, Some(vec![Some("x".into())]))));
    assert!(has_text(&s, "Eins"));
}

#[test]
fn romaji_appears_under_kana_lines() {
    let mut s = boot_with(listen_lyrics::parse_lrc("[00:01.00]ありがとう\n[00:05.00]Hello"));
    assert!(!has_text(&s, "arigatou"));
    let _ = s.update(Message::Setting(Setting::Lyrics(LyricsSetting::Romaji(true))));
    assert!(has_text(&s, "arigatou"));
}

#[test]
fn version_picker_switches_between_found_lyrics() {
    let mut s = boot();
    let id = s.queue.current().unwrap().song.id.clone();
    let a = listen_lyrics::parse_lrc("[00:01.00]From the server");
    let mut b = listen_lyrics::parse_lrc("[00:01.00]From lrclib");
    b.provider = "lrclib.net".into();
    let _ = s.update(Message::Lyrics(LyricsMsg::Loaded(id, vec![a, b])));
    assert!(has_text(&s, "From the server"));
    let _ = s.update(Message::Lyrics(LyricsMsg::Version(1)));
    assert!(has_text(&s, "From lrclib"));
    assert_eq!(s.lyrics_ui.selected, 1);
}

#[test]
fn provider_and_match_lines_follow_the_settings() {
    let mut l = listen_lyrics::parse_lrc(LRC);
    l.provider = "NetEase".into();
    l.matched = Some("Neon Horizons — Demo Artist".into());
    let mut s = boot_with(l);
    assert!(has_text(&s, "NetEase"));
    assert!(!has_text(&s, "Neon Horizons — Demo Artist"));
    let _ = s.update(Message::Setting(Setting::Lyrics(LyricsSetting::ShowMatch(true))));
    let _ = s.update(Message::Setting(Setting::Lyrics(LyricsSetting::ShowProvider(false))));
    assert!(has_text(&s, "Neon Horizons — Demo Artist"));
    assert!(!has_text(&s, "NetEase"));
}

#[test]
fn export_dialog_previews_lrc_and_txt() {
    let mut s = boot();
    click(&mut s, "EXPORT");
    assert!(s.lyrics_ui.export.is_some());
    assert!(has_text(&s, "Synchronized (.lrc)"));
    let e = s.lyrics_ui.export.as_ref().unwrap();
    assert!(e.text().starts_with("[ar:") && e.text().contains("[00:00.50]Neon lights across the bay"));
    let _ = s.update(Message::Lyrics(LyricsMsg::ExportSynced(false)));
    let e = s.lyrics_ui.export.as_ref().unwrap();
    assert!(e.text().starts_with("Neon lights across the bay\n"));
    assert!(e.file_name().ends_with(".txt"));
    let _ = s.update(Message::Lyrics(LyricsMsg::ExportSynced(true)));
    let _ = s.update(Message::Lyrics(LyricsMsg::ExportOffset("250".into())));
    let e = s.lyrics_ui.export.as_ref().unwrap();
    assert!(e.text().contains("[offset:250]"));
    assert!(e.file_name().ends_with(".lrc"));
}

#[test]
fn following_scrolls_further_for_later_lines_and_honours_the_alignment() {
    use listen::ui::lyrics::follow_offset;
    let mut s = boot();
    let a = follow_offset(&s, 1).unwrap();
    let b = follow_offset(&s, 3).unwrap();
    assert!(b > a, "{a} {b}");
    s.config.settings.lyrics.follow_scroll_alignment = -50;
    let up = follow_offset(&s, 3).unwrap();
    assert!(up > b, "a higher anchor scrolls further: {up} > {b}");
    s.config.settings.lyrics.follow_scroll_alignment = 50;
    assert!(follow_offset(&s, 3).unwrap() < b);
    assert_eq!(follow_offset(&s, 99), None);
}

#[test]
fn the_mpris_text_is_lrc_for_synced_and_plain_otherwise() {
    let s = boot();
    let text = s.lyrics_mpris_text().unwrap();
    assert!(text.contains("[00:00.50]Neon lights across the bay"), "{text}");
    let s = boot_with(listen_lyrics::parse_lrc("one\ntwo"));
    assert_eq!(s.lyrics_mpris_text().as_deref(), Some("one\ntwo\n"));
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    s.lyrics = None;
    assert_eq!(s.lyrics_mpris_text(), None);
}

#[test]
fn lyrics_changes_bump_the_revision_mpris_watches() {
    let mut s = boot();
    let before = s.lyrics_ui.rev;
    let id = s.queue.current().unwrap().song.id.clone();
    let _ = s.update(Message::Lyrics(LyricsMsg::Loaded(id, vec![listen_lyrics::parse_lrc("[00:01.00]x")])));
    assert!(s.lyrics_ui.rev > before);
}

#[test]
fn settings_page_lists_the_lyrics_options() {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        route: Some("settings".into()),
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    s.config.settings.settings_tab = listen_core::settings::SettingsTab::Playback;
    for text in ["Prefer local lyrics", "Lyric providers", "Follow the current lyric", "Translation provider"]
    {
        assert!(has_text_sized(&s, text, iced::Size::new(1400.0, 4000.0)), "{text}");
    }
}

fn has_text_sized(s: &Listen, text: &str, size: iced::Size) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), size, s.view());
    ui.find(text).is_ok()
}

#[test]
fn karaoke_words_light_up_with_the_playback_position() {
    let mut s = boot_with(listen_lyrics::parse_lrc(
        "[00:10.00]<00:10.00>Sing <00:11.00>along <00:12.00>now\n[00:14.00]<00:14.00>Next",
    ));
    s.config.settings.lyrics.line_lead_time_ms = 0;
    let at = |s: &mut Listen, ms: u64| {
        let _ = s.update(Message::Deck(
            0,
            listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(
                std::time::Duration::from_millis(ms),
            )),
        ));
    };
    at(&mut s, 11_200);
    assert_eq!(s.lyric_active, Some(0));
    assert_eq!(s.lyrics_ui.pos_ms, 11_200);
    at(&mut s, 12_500);
    assert_eq!(s.lyrics_ui.pos_ms, 12_500);
}
