use super::*;
use crate::analysis::{BarCurves, BeatGrid, Mood};
use crate::model::{ArtistRef, GainInfo, ServerId};
use std::time::Duration;

fn make(source: SourceKind, title: &str, artist: &str, secs: u64) -> Song {
    Song {
        mbz: Default::default(),
        extra: Default::default(),
        id: format!("{title}/{artist}/{secs}"),
        server_id: ServerId("s".into()),
        source,
        title: title.into(),
        artists: vec![ArtistRef { id: None, name: artist.into() }],
        album_artists: vec![],
        album: None,
        album_id: None,
        image: None,
        duration: Duration::from_secs(secs),
        track_number: None,
        disc_number: None,
        year: None,
        genres: vec![],
        codec: None,
        bit_rate: None,
        sample_rate: None,
        bit_depth: None,
        bpm: None,
        favorite: false,
        rating: None,
        play_count: None,
        gain: GainInfo::default(),
        explicit: false,
        path: None,
        size: None,
        suffix: None,
        channels: None,
        created: None,
        comment: None,
    }
}

fn ytm(title: &str, artist: &str, secs: u64) -> Song {
    make(SourceKind::YouTubeMusic, title, artist, secs)
}

fn own(title: &str, artist: &str, secs: u64) -> Song {
    make(SourceKind::Subsonic, title, artist, secs)
}

fn same(a: &Song, b: &Song) -> bool {
    score(a, b).is_some()
}

#[test]
fn identical_song_matches() {
    assert!(same(&ytm("Blinding Lights", "The Weeknd", 200), &own("Blinding Lights", "The Weeknd", 200)));
}

#[test]
fn case_and_punctuation_do_not_matter() {
    assert!(same(&ytm("Don't Stop Me Now", "Queen", 209), &own("dont stop me now!", "QUEEN", 209)));
    assert!(same(&ytm("Don’t Stop Me Now", "Queen", 209), &own("Don't Stop Me Now", "Queen", 209)));
    assert!(same(&ytm("Rock & Roll", "Led Zeppelin", 220), &own("Rock and Roll", "Led Zeppelin", 220)));
}

#[test]
fn accents_are_folded() {
    assert!(same(&ytm("Déjà Vu", "Beyoncé", 260), &own("Deja Vu", "Beyonce", 260)));
    assert!(same(&ytm("Mädchen", "Rammstein", 240), &own("Madchen", "Rammstein", 240)));
}

#[test]
fn noise_in_brackets_is_dropped() {
    let own = own("Shape of You", "Ed Sheeran", 234);
    for title in [
        "Shape of You (Official Video)",
        "Shape of You (Official Music Video)",
        "Shape of You [Official Audio]",
        "Shape of You (Lyric Video)",
        "Shape of You (Remastered 2011)",
        "Shape of You - Remastered 2011",
        "Shape of You - 2011 Remaster",
        "Shape of You (Explicit)",
        "Shape of You (Deluxe Edition)",
        "Shape of You (20th Anniversary Edition)",
        "Shape of You [HD]",
        "Shape of You 【MV】",
        "Shape of You (Radio Edit)",
        "Shape of You (Album Version)",
        "Shape of You - Single Version",
        "Shape of You (Original Mix)",
        "Shape of You (From \"A Film\")",
        "Shape of You - From the Film",
    ] {
        assert!(same(&ytm(title, "Ed Sheeran", 234), &own), "{title}");
    }
}

#[test]
fn topic_and_vevo_artists() {
    let own = own("Blinding Lights", "The Weeknd", 200);
    assert!(same(&ytm("Blinding Lights", "The Weeknd - Topic", 200), &own));
    assert!(same(&ytm("Blinding Lights", "TheWeekndVEVO", 200), &own));
    assert!(same(&ytm("Blinding Lights", "Weeknd", 200), &own));
}

#[test]
fn featured_artists_in_title_or_artist_list() {
    let own = own("Stay", "The Kid LAROI", 141);
    assert!(same(&ytm("Stay (feat. Justin Bieber)", "The Kid LAROI", 141), &own));
    assert!(same(&ytm("Stay ft. Justin Bieber", "The Kid LAROI", 141), &own));
    assert!(same(&ytm("Stay (with Justin Bieber)", "The Kid LAROI", 141), &own));
    // The server lists both, YouTube Music one.
    let both = self::own("Stay", "The Kid LAROI & Justin Bieber", 141);
    assert!(same(&ytm("Stay", "The Kid LAROI", 141), &both));
    assert!(same(&ytm("Stay", "Justin Bieber", 141), &both));
    // The server puts the featured artist into the title and the credit.
    let named = self::own("Stay (feat. Justin Bieber)", "The Kid LAROI feat. Justin Bieber", 141);
    assert!(same(&ytm("Stay", "The Kid LAROI", 141), &named));
}

#[test]
fn artist_prefix_in_the_title() {
    assert!(same(&ytm("Queen - Bohemian Rhapsody", "Queen", 355), &own("Bohemian Rhapsody", "Queen", 355)));
}

#[test]
fn leading_the_is_ignored() {
    assert!(same(&ytm("Yellow", "Coldplay", 267), &own("Yellow", "Coldplay", 267)));
    assert!(same(&ytm("Mr. Brightside", "The Killers", 222), &own("Mr. Brightside", "Killers", 222)));
}

#[test]
fn different_artist_or_title_is_rejected() {
    assert!(!same(&ytm("Hurt", "Johnny Cash", 218), &own("Hurt", "Nine Inch Nails", 218)));
    assert!(!same(&ytm("Hurt", "Johnny Cash", 218), &own("Hurts", "Johnny Cash", 218)));
    assert!(!same(&ytm("Home", "Edward Sharpe", 305), &own("Home Again", "Edward Sharpe", 305)));
}

#[test]
fn version_tags_must_agree() {
    let studio = own("Hotel California", "Eagles", 391);
    assert!(!same(&ytm("Hotel California (Live)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California - Live at the Forum", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Acoustic)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Instrumental)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Karaoke Version)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Sped Up)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Slowed + Reverb)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (DJ X Remix)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Piano Version)", "Eagles", 391), &studio));
    assert!(!same(&ytm("Hotel California (Cover)", "Eagles", 391), &studio));
    // And the other way round: the server's copy is the live one.
    let live = own("Hotel California (Live)", "Eagles", 391);
    assert!(!same(&ytm("Hotel California", "Eagles", 391), &live));
    // The same tag on both sides is the same recording.
    assert!(same(&ytm("Hotel California (Live)", "Eagles", 391), &live));
    assert!(same(&ytm("Hotel California - Live", "Eagles", 391), &live));
    let remix = own("Hotel California - Some Remix", "Eagles", 391);
    assert!(same(&ytm("Hotel California (Some Remix)", "Eagles", 391), &remix));
}

#[test]
fn remix_by_mix_word() {
    let studio = own("Blue Monday", "New Order", 450);
    assert!(!same(&ytm("Blue Monday (Club Mix)", "New Order", 450), &studio));
    assert!(!same(&ytm("Blue Monday (Hot Mix)", "New Order", 450), &studio));
    assert!(same(&ytm("Blue Monday (Original Mix)", "New Order", 450), &studio));
    assert!(same(&ytm("Blue Monday (Radio Mix)", "New Order", 450), &studio));
}

#[test]
fn words_that_merely_contain_a_tag_are_not_tags() {
    // "Alive" and "Deliver" contain "live" but are names.
    assert!(same(&ytm("Alive", "Pearl Jam", 340), &own("Alive", "Pearl Jam", 340)));
    assert!(same(&ytm("Live and Let Die", "Wings", 191), &own("Live and Let Die", "Wings", 191)));
}

#[test]
fn unknown_extras_must_agree() {
    let original = own("Love Story", "Taylor Swift", 235);
    assert!(!same(&ytm("Love Story (Taylor's Version)", "Taylor Swift", 235), &original));
    let rerecord = own("Love Story (Taylor’s Version)", "Taylor Swift", 235);
    assert!(same(&ytm("Love Story (Taylor's Version)", "Taylor Swift", 235), &rerecord));
    assert!(!same(&ytm("Love Story", "Taylor Swift", 235), &rerecord));
    let ja = own("Dynamite (Japanese Ver.)", "BTS", 199);
    assert!(!same(&ytm("Dynamite", "BTS", 199), &ja));
    assert!(same(
        &ytm("Dynamite (Japanese Version)", "BTS", 199),
        &own("Dynamite (Japanese Ver)", "BTS", 199)
    ));
}

#[test]
fn duration_within_three_seconds() {
    let own = own("Africa", "Toto", 295);
    assert!(same(&ytm("Africa", "Toto", 295), &own));
    assert!(same(&ytm("Africa", "Toto", 297), &own));
    assert!(same(&ytm("Africa", "Toto", 292), &own));
    assert!(!same(&ytm("Africa", "Toto", 330), &own));
    // A short song gets no share.
    let short = self::own("Jingle", "Toto", 60);
    assert!(same(&ytm("Jingle", "Toto", 63), &short));
    assert!(!same(&ytm("Jingle", "Toto", 65), &short));
}

#[test]
fn long_songs_get_two_percent() {
    let long = own("Echoes", "Pink Floyd", 1400);
    assert!(same(&ytm("Echoes", "Pink Floyd", 1420), &long)); // 20 s < 28 s
    assert!(!same(&ytm("Echoes", "Pink Floyd", 1450), &long));
    let mid = own("Money", "Pink Floyd", 382);
    assert!(same(&ytm("Money", "Pink Floyd", 388), &mid)); // 6 s < 7.6 s
    assert!(!same(&ytm("Money", "Pink Floyd", 392), &mid));
}

#[test]
fn unknown_duration_is_allowed_but_scores_lower() {
    let known = score(&ytm("Africa", "Toto", 295), &own("Africa", "Toto", 295)).unwrap();
    let unknown = score(&ytm("Africa", "Toto", 0), &own("Africa", "Toto", 295)).unwrap();
    assert!(unknown < known);
}

#[test]
fn album_match_is_preferred() {
    let mut single = own("Hello", "Adele", 295);
    single.album = Some("Hello - Single".into());
    single.id = "single".into();
    let mut album = own("Hello", "Adele", 295);
    album.album = Some("25".into());
    album.id = "album".into();
    let mut wanted = ytm("Hello", "Adele", 295);
    wanted.album = Some("25".into());
    let found = [single.clone(), album.clone()];
    assert_eq!(best_match(&wanted, &found).unwrap().id, "album");
    // Without an album on our side the first of equals wins.
    wanted.album = None;
    assert_eq!(best_match(&wanted, &found).unwrap().id, "single");
}

#[test]
fn album_deluxe_suffix_is_ignored() {
    let mut a = ytm("Hello", "Adele", 295);
    a.album = Some("25 (Deluxe)".into());
    let mut b = own("Hello", "Adele", 295);
    b.album = Some("25".into());
    let mut c = own("Hello", "Adele", 295);
    c.album = Some("Other".into());
    assert!(score(&a, &b).unwrap() > score(&a, &c).unwrap());
}

#[test]
fn closer_duration_and_lossless_win() {
    let wanted = ytm("Africa", "Toto", 295);
    let mut near = own("Africa", "Toto", 295);
    near.id = "near".into();
    let mut far = own("Africa", "Toto", 297);
    far.id = "far".into();
    assert_eq!(best_match(&wanted, &[far.clone(), near.clone()]).unwrap().id, "near");
    let mut flac = own("Africa", "Toto", 295);
    flac.id = "flac".into();
    flac.suffix = Some("flac".into());
    let mut mp3 = own("Africa", "Toto", 295);
    mp3.id = "mp3".into();
    mp3.bit_rate = Some(192);
    assert_eq!(best_match(&wanted, &[mp3, flac]).unwrap().id, "flac");
}

#[test]
fn youtube_candidates_never_match() {
    assert!(!same(&ytm("Africa", "Toto", 295), &ytm("Africa", "Toto", 295)));
}

#[test]
fn nothing_found_is_none() {
    assert!(best_match(&ytm("Africa", "Toto", 295), &[]).is_none());
    assert!(best_match(&ytm("Africa", "Toto", 295), &[own("Rosanna", "Toto", 295)]).is_none());
}

#[test]
fn korean_titles_and_artists() {
    assert!(same(&ytm("봄날", "BTS", 274), &own("봄날", "BTS", 274)));
    assert!(same(&ytm("봄날 (Spring Day)", "BTS", 274), &own("봄날", "BTS", 274)));
    assert!(!same(&ytm("봄날", "BTS", 274), &own("Spring Day", "BTS", 274)));
    assert!(same(&ytm("봄날 (Spring Day)", "BTS", 274), &own("Spring Day", "BTS", 274)));
    assert!(same(&ytm("봄날", "방탄소년단", 274), &own("봄날", "BTS (방탄소년단)", 274)));
    assert!(same(&ytm("봄날", "BTS", 274), &own("봄날", "BTS (방탄소년단)", 274)));
    assert!(same(&ytm("Dynamite", "방탄소년단", 199), &own("Dynamite", "BTS (방탄소년단)", 199)));
    assert!(!same(&ytm("봄날 (Instrumental)", "BTS", 274), &own("봄날", "BTS", 274)));
    assert!(!same(&ytm("봄날", "BTS", 274), &own("봄날 (Inst.)", "BTS", 274)));
    assert!(same(&ytm("봄날 (Official MV)", "BTS", 274), &own("봄날", "BTS", 274)));
    assert!(!same(&ytm("봄날 (Japanese Ver.)", "BTS", 274), &own("봄날", "BTS", 274)));
    assert!(!same(&ytm("봄날", "BTS", 274), &own("봄날", "Someone Else", 274)));
    assert!(!same(&ytm("봄", "BTS", 274), &own("봄날", "BTS", 274)));
}

#[test]
fn korean_mr_is_an_instrumental() {
    assert!(!same(&ytm("Love Dive (MR)", "IVE", 177), &own("Love Dive", "IVE", 177)));
    assert!(same(&ytm("Love Dive (MR)", "IVE", 177), &own("Love Dive (Instrumental)", "IVE", 177)));
}

#[test]
fn japanese_titles_and_artists() {
    assert!(same(&ytm("残酷な天使のテーゼ", "高橋洋子", 247), &own("残酷な天使のテーゼ", "高橋洋子", 247)));
    assert!(same(
        &ytm("残酷な天使のテーゼ (TV Size)", "高橋洋子", 90),
        &own("残酷な天使のテーゼ (TV Size)", "高橋洋子", 90)
    ));
    assert!(!same(
        &ytm("残酷な天使のテーゼ (TV Size)", "高橋洋子", 90),
        &own("残酷な天使のテーゼ", "高橋洋子", 247)
    ));
    // Full-width letters, digits, punctuation and spaces.
    assert!(same(&ytm("ＬＯＶＥ　２００", "Ｆｏｏ", 200), &own("LOVE 200", "Foo", 200)));
    assert!(same(&ytm("シャルル", "バルーン", 261), &own("シャルル", "バルーン", 261)));
    assert!(same(&ytm("Lemon", "米津玄師", 255), &own("Lemon", "米津玄師 (Kenshi Yonezu)", 255)));
    assert!(same(&ytm("Lemon", "Kenshi Yonezu", 255), &own("Lemon", "米津玄師 (Kenshi Yonezu)", 255)));
    assert!(same(&ytm("【MV】夜に駆ける", "YOASOBI", 261), &own("夜に駆ける", "YOASOBI", 261)));
    assert!(!same(&ytm("夜に駆ける (Live)", "YOASOBI", 261), &own("夜に駆ける", "YOASOBI", 261)));
    assert!(!same(&ytm("夜に駆ける", "YOASOBI", 261), &own("夜に駆けない", "YOASOBI", 261)));
}

#[test]
fn quoted_titles_in_brackets_of_east_asian_style() {
    assert!(same(&ytm("「Lemon」", "米津玄師", 255), &own("Lemon", "米津玄師", 255)));
}

#[test]
fn dashes_in_names_are_not_extras_when_both_have_them() {
    assert!(same(&ytm("Hello - Part 2", "Foo", 200), &own("Hello - Part 2", "Foo", 200)));
    assert!(!same(&ytm("Hello - Part 2", "Foo", 200), &own("Hello", "Foo", 200)));
    assert!(same(&ytm("Spider-Man Theme", "Foo", 200), &own("Spider-Man Theme", "Foo", 200)));
}

#[test]
fn comma_artists_and_ampersands() {
    let own = own("Under Pressure", "Queen & David Bowie", 248);
    assert!(same(&ytm("Under Pressure", "Queen", 248), &own));
    assert!(same(&ytm("Under Pressure", "David Bowie", 248), &own));
    let both = self::own("Under Pressure", "Queen, David Bowie", 248);
    assert!(same(&ytm("Under Pressure", "Queen & David Bowie", 248), &both));
    assert!(same(&ytm("Under Pressure - Remastered 2011", "Queen", 248), &both));
}

#[test]
fn album_artist_counts_on_the_server() {
    let mut s = own("Intro", "Various", 60);
    s.album_artists = vec![ArtistRef { id: None, name: "Daft Punk".into() }];
    assert!(same(&ytm("Intro", "Daft Punk", 60), &s));
}

#[test]
fn title_with_only_brackets_still_matches_itself() {
    assert!(same(&ytm("(Intro)", "Foo", 30), &own("(Intro)", "Foo", 30)));
}

#[test]
fn queries_name_the_song_cleanly() {
    let q = queries(&ytm("Shape of You (Official Video) feat. X", "Ed Sheeran - Topic", 234));
    assert_eq!(q, vec!["shape of you".to_string(), "shape of you ed sheeran".to_string()]);
    assert!(queries(&ytm("", "X", 1)).is_empty());
    assert_eq!(queries(&ytm("봄날 (Spring Day)", "BTS", 274))[0], "봄날");
}

fn features(start: f32, bpm: f32, first_beat: f32, confidence: f32) -> Features {
    let period = 60.0 / bpm;
    Features {
        audio_start: start,
        audio_end: 200.0,
        grid: Some(BeatGrid {
            beats: (0..400).map(|i| first_beat + i as f32 * period).collect(),
            downbeat_phase: 0,
            beats_per_bar: 4,
            bpm,
            confidence,
            stability: 1.0,
        }),
        key: None,
        lufs: -9.0,
        lra: 5.0,
        bars: BarCurves::default(),
        phrase_bars: 8,
        phrase_offset: 0,
        sections: vec![],
        intro_instrumental_bars: 0,
        outro_instrumental_bars: 0,
        timbre: vec![],
        mood: Mood::default(),
    }
}

#[test]
fn shift_from_the_beat_grids() {
    // The other file has 1.31 s more lead-in: its beats sit 1.31 s later.
    let ytm = features(1.35, 120.0, 1.81, 0.9);
    let own = features(0.04, 120.0, 0.5, 0.9);
    let s = time_shift(&ytm, &own).unwrap();
    assert!((s - 1.31).abs() < 0.01, "{s}");
    // Whole beats are chosen by where the sound starts, not by the first detected beat.
    let ytm = features(2.0, 120.0, 0.0, 0.9);
    let own = features(0.1, 120.0, 0.0, 0.9);
    let s = time_shift(&ytm, &own).unwrap();
    assert!((s - 2.0).abs() < 0.01, "{s}");
}

#[test]
fn shift_needs_matching_tempo_and_confidence() {
    // Tempo differs: only the sound start is used.
    let s = time_shift(&features(1.5, 120.0, 0.0, 0.9), &features(0.0, 100.0, 0.0, 0.9)).unwrap();
    assert!((s - 1.5).abs() < 1e-6);
    let s = time_shift(&features(1.5, 120.0, 0.0, 0.2), &features(0.0, 120.0, 0.0, 0.9)).unwrap();
    assert!((s - 1.5).abs() < 1e-6);
}

#[test]
fn shift_without_grid_uses_the_start() {
    let mut a = features(1.0, 120.0, 0.0, 0.9);
    let mut b = features(0.2, 120.0, 0.0, 0.9);
    a.grid = None;
    b.grid = None;
    assert!((time_shift(&a, &b).unwrap() - 0.8).abs() < 1e-6);
}

#[test]
fn shift_ignores_tiny_gaps_without_grid_and_far_ones() {
    let mut a = features(0.52, 120.0, 0.0, 0.9);
    let mut b = features(0.5, 120.0, 0.0, 0.9);
    a.grid = None;
    b.grid = None;
    assert_eq!(time_shift(&a, &b), None);
    assert_eq!(time_shift(&features(30.0, 120.0, 0.0, 0.9), &features(0.0, 120.0, 0.0, 0.9)), None);
}
