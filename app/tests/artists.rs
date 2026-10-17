//! The artist pages: the artist and album-artist lists, the artist page of a
//! Navidrome artist (release-type groups, appears on, top songs community /
//! personal, favourites, biography links, similar artists, section order) and
//! of a YouTube Music artist (albums, singles, videos, subscribe).

use iced_test::simulator::Simulator;
use listen::app::artist_page::{ArtistMsg, ArtistSetting, ArtistView, FavMode, TopMode};
use listen::app::list::ListId;
use listen::app::{ArtistDetail, INTER, Listen, Message, Route};
use listen::{Args, FONTS, demo};
use listen_core::model::{Album, Artist, ArtistRef, ImageRef, Page, ServerId, Song, SourceKind};
use listen_core::settings::PlayBehavior;
use listen_source::{ArtistInfo, ExternalLinks};
use std::time::Duration;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

const SIZE: iced::Size = iced::Size::new(1600.0, 1400.0);

fn boot() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(SIZE));
    s
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn click(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn len(s: &Listen, id: &ListId) -> usize {
    s.list_data(id).map_or(0, |d| d.len())
}

fn artist(id: &str, name: &str, source: SourceKind) -> Artist {
    Artist {
        id: id.into(),
        server_id: ServerId("demo".into()),
        source,
        name: name.into(),
        image: None,
        album_count: None,
        song_count: None,
        favorite: false,
        biography: None,
        similar: vec![],
        rating: None,
    }
}

fn album(id: &str, name: &str, year: i32, kind: &str, by: &str) -> Album {
    let mut a = demo::albums().remove(0);
    a.id = id.into();
    a.name = name.into();
    a.year = Some(year);
    a.release_type = Some(kind.into());
    a.album_artists = vec![ArtistRef { id: Some(by.into()), name: by.into() }];
    a.genres = vec!["Synthwave".into(), "Electronic".into()];
    a.duration = Duration::from_secs(40 * 60);
    a.song_count = Some(10);
    a.songs.clear();
    a
}

fn song(id: &str, title: &str, artist: &Artist) -> Song {
    let mut s = demo::album_songs(0).remove(0);
    s.id = id.into();
    s.title = title.into();
    s.artists = vec![ArtistRef { id: Some(artist.id.clone()), name: artist.name.clone() }];
    s.album_artists = s.artists.clone();
    s
}

const BIO: &str = "Neon Coast make glowing synth records. <a href=\"https://www.last.fm/music/Neon+Coast\">Read more on Last.fm</a>. \
    The band formed in 2010 and has released five albums, two EPs and a handful of singles since then. Their live shows are \
    known for the lights, the long intros and the way the songs of the first record are stretched out into extended versions. \
    Critics have compared them to the early 80s film scores, to french house, and to nothing at all.";

/// A Navidrome artist with everything the page can show.
fn navidrome(s: &mut Listen) -> Artist {
    let a = artist("nc", "Neon Coast", SourceKind::Subsonic);
    let albums = vec![
        album("a1", "Night Drive", 2022, "album", "nc"),
        album("a2", "Horizon", 2019, "album", "nc"),
        album("e1", "Afterglow EP", 2020, "ep", "nc"),
        album("s1", "Single Light", 2021, "single", "nc"),
        album("l1", "Live At The Pier", 2023, "live", "nc"),
    ];
    let top =
        vec![song("t1", "Night Drive", &a), song("t2", "Horizon Line", &a), song("t3", "Afterglow", &a)];
    let _ = s.update(Message::Navigate(Route::Artist("nc".into())));
    let mut a = a;
    a.album_count = Some(5);
    a.song_count = Some(52);
    a.rating = Some(4);
    a.biography = Some(BIO.into());
    let _ = s.update(Message::ArtistLoaded(Ok(ArtistDetail {
        artist: a.clone(),
        albums,
        top_songs: top.clone(),
    })));
    let rev = s.artist_page.rev;
    let guest = album("g1", "Various Summer Hits", 2018, "compilation", "va");
    let _ = s.update(Message::Artist(ArtistMsg::AppearsOn(rev, Ok(vec![guest]))));
    let info = ArtistInfo {
        biography: Some(BIO.into()),
        links: ExternalLinks {
            musicbrainz_url: Some("https://musicbrainz.org/artist/abc".into()),
            lastfm_url: Some("https://www.last.fm/music/Neon+Coast".into()),
        },
        musicbrainz_id: Some("abc".into()),
        similar: vec![],
        similar_artists: vec![
            artist("ic", "Ivy Lane", SourceKind::Subsonic),
            artist("hb", "The Harbours", SourceKind::Subsonic),
            artist("ms", "Mira Sol", SourceKind::Subsonic),
        ],
        genres: vec![],
    };
    let _ = s.update(Message::Artist(ArtistMsg::Info(rev, Ok(info))));
    let mut fav = song("t2", "Horizon Line", &a);
    fav.favorite = true;
    let _ = s.update(Message::Artist(ArtistMsg::Favorites(rev, Ok(vec![fav]))));
    a
}

fn ytm(s: &mut Listen) -> Artist {
    let a = artist("UC123", "Kai Rosenkranz", SourceKind::YouTubeMusic);
    let mut albums = vec![
        album("yb1", "Gothic Dreams", 2021, "album", "UC123"),
        album("ys1", "Midnight Single", 2022, "single", "UC123"),
    ];
    for al in &mut albums {
        al.source = SourceKind::YouTubeMusic;
        al.genres.clear();
    }
    let top = vec![song("v1", "Gothic Opening", &a), song("v2", "Dark Hall", &a)];
    let _ = s.update(Message::Navigate(Route::Artist("UC123".into())));
    let mut a = a;
    a.biography = Some("Composer of games and film.".into());
    a.similar = vec![ArtistRef { id: Some("UC9".into()), name: "Ana Brandt".into() }];
    let _ = s.update(Message::ArtistLoaded(Ok(ArtistDetail { artist: a.clone(), albums, top_songs: top })));
    let rev = s.artist_page.rev;
    let video = song("vid1", "Gothic Dreams (Official Video)", &a);
    let _ = s.update(Message::Artist(ArtistMsg::Videos(rev, Ok(vec![video]))));
    a
}

fn snapshot(name: &str, s: &Listen) {
    let theme = s.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    let snap = ui.snapshot(&theme).expect("snapshot renders");
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    assert!(snap.matches_image(dir.join(name)).unwrap());
}

#[test]
fn navidrome_artist_page_shows_the_sections_in_feishins_order() {
    let mut s = boot();
    navidrome(&mut s);
    for text in [
        "ARTIST",
        "Neon Coast",
        "5 albums · 52 tracks · 3 hr 20 min",
        "VIEW DISCOGRAPHY",
        "VIEW ALL TRACKS",
        "ARTIST RADIO",
        "GENRES",
        "EXTERNAL LINKS",
        "About Neon Coast",
        "Appears on",
        "Album",
        "EP",
        "Single",
        "Live",
        "Related artists",
        "Top songs",
        "Favorite songs",
        "Ivy Lane",
    ] {
        assert!(shows(&s, text), "{text:?} missing");
    }
    snapshot("artist-navidrome", &s);
}

#[test]
fn release_groups_follow_the_priority_and_hide() {
    let mut s = boot();
    navidrome(&mut s);
    let keys: Vec<String> = s.artist_groups().into_iter().map(|g| g.key).collect();
    assert_eq!(keys, ["appears-on", "album", "ep", "live", "single"]);
    // Hide Live (index of releaseTypeLive in the list) and move Single first.
    let idx = |s: &Listen, id: &str| {
        s.config.settings.artist.release_types.iter().position(|i| i.id == id).unwrap()
    };
    let live = idx(&s, "releaseTypeLive");
    let _ =
        s.update(Message::Setting(listen::app::Setting::Artist(ArtistSetting::ReleaseEnabled(live, false))));
    let single = idx(&s, "releaseTypeSingle");
    for i in (1..=single).rev() {
        let _ = s.update(Message::Setting(listen::app::Setting::Artist(ArtistSetting::MoveRelease(i, -1))));
    }
    let keys: Vec<String> = s.artist_groups().into_iter().map(|g| g.key).collect();
    assert_eq!(keys, ["single", "appears-on", "album", "ep"]);
    assert!(!shows(&s, "Live At The Pier"));
}

#[test]
fn section_order_and_visibility_follow_the_setting() {
    let mut s = boot();
    navidrome(&mut s);
    assert!(shows(&s, "About Neon Coast"));
    let _ = s.update(Message::Setting(listen::app::Setting::Artist(ArtistSetting::SectionEnabled(0, false))));
    assert!(!shows(&s, "About Neon Coast"), "the biography is switched off");
    let _ = s.update(Message::Setting(listen::app::Setting::Artist(ArtistSetting::SectionEnabled(4, false))));
    assert!(!shows(&s, "Top songs"));
    assert!(shows(&s, "Favorite songs"));
    assert_eq!(
        s.config.settings.artist.sections().iter().map(|x| x.id()).collect::<Vec<_>>(),
        ["favoriteSongs", "recentAlbums", "similarArtists"]
    );
}

#[test]
fn biography_collapses_and_links_are_rich_text() {
    let mut s = boot();
    navidrome(&mut s);
    assert!(shows(&s, "Show more"));
    let _ = click(&mut s, "Show more");
    assert!(s.artist_page.bio_open);
    assert!(shows(&s, "Show less"));
}

#[test]
fn header_buttons_send_their_messages() {
    let mut s = boot();
    navidrome(&mut s);
    let m = click(&mut s, "Shuffle");
    assert!(m.iter().any(|m| matches!(m, Message::Artist(ArtistMsg::Shuffle))), "{m:?}");
    let m = click(&mut s, "Add next");
    assert!(m.iter().any(|m| matches!(m, Message::Artist(ArtistMsg::Play(PlayBehavior::Next)))));
    let m = click(&mut s, "ARTIST RADIO");
    assert!(m.iter().any(|m| matches!(m, Message::Artist(ArtistMsg::Radio))));
    let m = click(&mut s, "VIEW DISCOGRAPHY");
    assert!(m.iter().any(
        |m| matches!(m, Message::Navigate(Route::ArtistList(id, ArtistView::Discography)) if id == "nc")
    ));
}

#[test]
fn favourite_and_rating_update_the_artist() {
    let mut s = boot();
    navidrome(&mut s);
    let _ = s.update(Message::Artist(ArtistMsg::ToggleFavorite));
    assert!(s.library.artist.as_ref().unwrap().artist.favorite);
    let _ = s.update(Message::Artist(ArtistMsg::Rate(2)));
    assert_eq!(s.library.artist.as_ref().unwrap().artist.rating, Some(2));
    // The same star again clears it.
    let _ = s.update(Message::Artist(ArtistMsg::Rate(2)));
    assert_eq!(s.library.artist.as_ref().unwrap().artist.rating, None);
}

#[test]
fn top_songs_toggle_between_community_and_personal() {
    let mut s = boot();
    navidrome(&mut s);
    assert_eq!(s.artist_page.top_all.len(), 3);
    // Personal needs plays in the local history (the demo's own are cleared).
    s.history.clear().unwrap();
    let a = s.library.artist.as_ref().unwrap().artist.clone();
    let mut played = song("t3", "Afterglow", &a);
    played.server_id = a.server_id.clone();
    for i in 0..3 {
        s.history.add(listen_core::history::Entry::new(&played, 1_700_000_000 + i)).unwrap();
    }
    let rev = s.artist_page.rev;
    let _ = s.update(Message::ArtistLoaded(Ok(s.library.artist.clone().unwrap())));
    assert!(s.artist_page.rev > rev);
    assert_eq!(s.artist_page.personal.len(), 1);
    assert!(shows(&s, "Personal"));
    let _ = s.update(Message::Artist(ArtistMsg::TopMode(TopMode::Personal)));
    assert_eq!(s.artist_page.top_mode, TopMode::Personal);
    assert_eq!(s.artist_page.top_all.len(), 1);
    assert_eq!(s.artist_page.personal_plays, [3]);
    let _ = s.update(Message::Artist(ArtistMsg::TopMode(TopMode::Community)));
    assert_eq!(s.artist_page.top_all.len(), 3);
    // The search narrows the rows, not the count of the section.
    let _ = s.update(Message::Artist(ArtistMsg::TopSearch("horizon".into())));
    assert_eq!(s.artist_page.top_view.len(), 1);
    assert_eq!(s.artist_page.top_all.len(), 3);
}

#[test]
fn favorite_songs_follow_the_mode() {
    let mut s = boot();
    navidrome(&mut s);
    assert_eq!(s.artist_page.fav_all.len(), 1);
    let rev = s.artist_page.rev;
    let mut rated = song("t1", "Night Drive", &s.library.artist.as_ref().unwrap().artist.clone());
    rated.rating = Some(5);
    let _ = s.update(Message::Artist(ArtistMsg::AllSongs(rev, Ok(vec![rated]))));
    let _ = s.update(Message::Artist(ArtistMsg::FavMode(FavMode::Rating)));
    assert_eq!(s.artist_page.fav_all[0].title, "Night Drive");
}

#[test]
fn release_group_play_buttons_queue_the_albums() {
    let mut s = boot();
    navidrome(&mut s);
    // The demo albums carry their songs, so no server is needed to queue them.
    if let Some(d) = s.library.artist.as_mut() {
        d.albums[0].songs = demo::album_songs(0);
        d.albums[1].songs = demo::album_songs(1);
    }
    let before = s.queue.len();
    let _ = s.update(Message::Artist(ArtistMsg::PlayGroup("album".into(), PlayBehavior::Last)));
    let added = demo::album_songs(0).len() + demo::album_songs(1).len();
    assert_eq!(s.queue.len(), before + added);
}

#[test]
fn sub_pages_list_the_data() {
    let mut s = boot();
    navidrome(&mut s);
    let id = "nc".to_string();
    let _ = s.update(Message::Navigate(Route::ArtistList(id.clone(), ArtistView::Discography)));
    assert_eq!(len(&s, &ListId::ArtistAlbums(id.clone())), 6, "5 albums and 1 appears-on");
    assert!(shows(&s, "Discography"));
    assert!(shows(&s, "Night Drive"));
    let _ = s.update(Message::Navigate(Route::ArtistList(id.clone(), ArtistView::TopSongs)));
    assert_eq!(len(&s, &ListId::Artist(id.clone())), 3);
    let _ = s.update(Message::Navigate(Route::ArtistList(id.clone(), ArtistView::FavoriteSongs)));
    assert_eq!(len(&s, &ListId::Artist(id.clone())), 1);
    let rev = s.artist_page.rev;
    let a = s.library.artist.as_ref().unwrap().artist.clone();
    let all = vec![song("t1", "Night Drive", &a), song("t2", "Horizon Line", &a)];
    let _ = s.update(Message::Navigate(Route::ArtistList(id.clone(), ArtistView::Tracks)));
    let _ = s.update(Message::Artist(ArtistMsg::AllSongs(s.artist_page.rev.max(rev), Ok(all))));
    assert_eq!(len(&s, &ListId::Artist(id)), 2);
    assert!(shows(&s, "All tracks"));
}

#[test]
fn youtube_music_artist_has_albums_singles_videos_and_no_appears_on() {
    let mut s = boot();
    ytm(&mut s);
    for text in [
        "Kai Rosenkranz",
        "Album",
        "Single",
        "Videos",
        "Top songs",
        "Gothic Dreams",
        "Related artists",
        "Ana Brandt",
    ] {
        assert!(shows(&s, text), "{text:?} missing");
    }
    assert!(!shows(&s, "Appears on"));
    let keys: Vec<String> = s.artist_groups().into_iter().map(|g| g.key).collect();
    assert_eq!(keys, ["album", "single"]);
    // Subscribing is the favourite button.
    let _ = s.update(Message::Artist(ArtistMsg::ToggleFavorite));
    assert!(s.library.artist.as_ref().unwrap().artist.favorite);
    snapshot("artist-ytm", &s);
}

#[test]
fn artist_lists_use_the_list_component() {
    let mut s = boot();
    let names = ["Neon Coast", "Ivy Lane", "The Harbours"];
    let artists: Vec<Artist> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut a = artist(&format!("a{i}"), n, SourceKind::Subsonic);
            a.album_count = Some(i as u32 + 2);
            a.image = Some(ImageRef::Id(format!("img{i}")));
            a
        })
        .collect();
    let _ = s.update(Message::Navigate(Route::AlbumArtists));
    let _ = s.update(Message::ArtistsLoaded(Ok(Page { items: artists, start_index: 0, total: Some(3) })));
    assert!(shows(&s, "Album Artists"));
    assert!(shows(&s, "3 artists"));
    assert!(shows(&s, "Ivy Lane"));
    snapshot("artists-list", &s);
    let _ = s.update(Message::Navigate(Route::Artists));
    assert!(shows(&s, "Artists"));
    assert!(shows(&s, "The Harbours"));
    assert_eq!(len(&s, &ListId::Artists), 3);
}
