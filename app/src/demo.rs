//! Built-in demo library for `--demo`: UI work and headless screenshots without a server.

use listen_core::model::{ArtistRef, GainInfo, ServerId, Song, SourceKind};
use std::time::Duration;

pub struct DemoAlbum {
    pub name: &'static str,
    pub artist: &'static str,
    pub year: i32,
    /// Placeholder artwork colour.
    pub color: (u8, u8, u8),
    pub tracks: &'static [(&'static str, u64)],
}

pub const ALBUMS: &[DemoAlbum] = &[
    DemoAlbum {
        name: "Night Drive",
        artist: "Neon Coast",
        year: 2024,
        color: (88, 60, 170),
        tracks: &[
            ("Midnight Signal", 214),
            ("Coastline", 187),
            ("Afterglow Avenue", 243),
            ("Static Hearts", 199),
            ("Rearview", 231),
            ("Last Exit", 276),
        ],
    },
    DemoAlbum {
        name: "Paper Moons",
        artist: "Ivy Lane",
        year: 2021,
        color: (200, 120, 60),
        tracks: &[("Paper Moons", 201), ("Lanterns", 176), ("Soft Rain", 222)],
    },
    DemoAlbum {
        name: "Low Tide",
        artist: "The Harbours",
        year: 2019,
        color: (40, 130, 150),
        tracks: &[("Low Tide", 245), ("Gulls", 162), ("Undertow", 298)],
    },
    DemoAlbum {
        name: "Glass Garden",
        artist: "Mira Sol",
        year: 2023,
        color: (60, 150, 90),
        tracks: &[("Glass Garden", 188), ("Fern", 205)],
    },
    DemoAlbum {
        name: "Overdrive",
        artist: "Volt Theory",
        year: 2022,
        color: (190, 50, 70),
        tracks: &[("Overdrive", 233), ("Circuit", 219), ("Ignition", 187)],
    },
    DemoAlbum {
        name: "Quiet Hours",
        artist: "Ana Brandt",
        year: 2020,
        color: (120, 120, 140),
        tracks: &[("Quiet Hours", 264), ("Dust", 190)],
    },
    DemoAlbum {
        name: "Solar Drift",
        artist: "Kepler Kids",
        year: 2025,
        color: (220, 170, 40),
        tracks: &[("Solar Drift", 240), ("Orbit", 201)],
    },
    DemoAlbum {
        name: "Velvet Room",
        artist: "Jules & The Tape",
        year: 2018,
        color: (130, 40, 110),
        tracks: &[("Velvet Room", 222), ("Slow Dance", 248)],
    },
];

pub fn album_songs(index: usize) -> Vec<Song> {
    let album = &ALBUMS[index % ALBUMS.len()];
    album
        .tracks
        .iter()
        .enumerate()
        .map(|(i, (title, _))| Song {
            mbz: Default::default(),
            extra: listen_core::model::SongExtra {
                composers: vec![["Lena Voss", "Marc Ostrow"][(i + index) % 2].into()],
                date: Some(format!("{}-0{}-1{}", 2015 + index % 9, 1 + i % 9, i % 9)),
                release_date: Some(format!("{}-0{}-0{}", 2015 + index % 9, 1 + i % 9, 1 + i % 9)),
                last_played: (i % 2 == 0).then(|| format!("2026-10-{:02}T20:00:00Z", 1 + (i + index) % 14)),
            },
            id: format!("demo-{index}-{i}"),
            server_id: ServerId("demo".into()),
            source: SourceKind::Subsonic,
            title: (*title).into(),
            artists: vec![ArtistRef { id: None, name: album.artist.into() }],
            album_artists: vec![ArtistRef { id: None, name: album.artist.into() }],
            album: Some(album.name.into()),
            album_id: Some(format!("demo-{index}")),
            image: None,
            duration: Duration::from_secs(TRACK_SECS as u64),
            track_number: Some(i as u32 + 1),
            disc_number: Some(1),
            year: Some(album.year),
            genres: vec!["Synthwave".into()],
            codec: Some("FLAC".into()),
            bit_rate: Some(1011),
            sample_rate: Some(44100),
            bit_depth: Some(16),
            bpm: Some(bpm_for(&format!("demo-{index}-{i}")) as u32),
            favorite: i == 0,
            rating: None,
            play_count: Some(3),
            gain: GainInfo::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        })
        .collect()
}

/// Placeholder colour for a song's album (demo artwork).
pub fn color_for_album(album_id: Option<&str>) -> (u8, u8, u8) {
    album_id
        .and_then(|id| id.strip_prefix("demo-"))
        .and_then(|n| n.parse::<usize>().ok())
        .map_or((60, 60, 60), |i| ALBUMS[i % ALBUMS.len()].color)
}

/// The demo albums as library `Album`s (songs filled in).
pub fn albums() -> Vec<listen_core::model::Album> {
    ALBUMS
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let songs = album_songs(i);
            listen_core::model::Album {
                release_types: Vec::new(),
                id: format!("demo-{i}"),
                server_id: ServerId("demo".into()),
                source: SourceKind::Subsonic,
                name: a.name.into(),
                album_artists: vec![ArtistRef { id: None, name: a.artist.into() }],
                image: None,
                year: Some(a.year),
                release_type: Some("Album".into()),
                genres: vec!["Synthwave".into()],
                song_count: Some(songs.len() as u32),
                duration: songs.iter().map(|s| s.duration).sum(),
                favorite: false,
                rating: None,
                play_count: None,
                songs,
                info: Default::default(),
            }
        })
        .collect()
}

/// A song of a detail fixture album.
fn fixture_song(
    album: &listen_core::model::Album,
    source: SourceKind,
    disc: u32,
    track: u32,
    title: &str,
    explicit: bool,
) -> Song {
    let mut s = album_songs(0).remove(0);
    s.id = format!("{}-{disc}-{track}", album.id);
    s.source = source;
    s.title = title.into();
    s.artists = album.album_artists.clone();
    s.album_artists = album.album_artists.clone();
    s.album = Some(album.name.clone());
    s.album_id = Some(album.id.clone());
    s.disc_number = Some(disc);
    s.track_number = Some(track);
    s.duration = Duration::from_secs(150 + 17 * u64::from(track) + 40 * u64::from(disc));
    s.genres = album.genres.clone();
    s.favorite = disc == 1 && track == 2;
    s.explicit = explicit;
    s.size = Some(31_000_000 + 1_000_000 * u64::from(track));
    s.bpm = None;
    s
}

/// Navidrome-style album fixture: two discs, OpenSubsonic metadata, a comment, favourite and rating.
pub fn navidrome_album() -> listen_core::model::Album {
    use listen_core::model::{Album, AlbumInfo};
    let mut album = Album {
        release_types: vec!["album".into(), "live".into()],
        id: "nd-album-neon".into(),
        server_id: ServerId("nd".into()),
        source: SourceKind::Subsonic,
        name: "Neon Horizons".into(),
        album_artists: vec![
            ArtistRef { id: Some("nd-artist-1".into()), name: "Night Drive".into() },
            ArtistRef { id: Some("nd-artist-2".into()), name: "Violet Skies".into() },
        ],
        image: None,
        year: Some(2021),
        release_type: Some("Album".into()),
        genres: vec!["Synthwave".into(), "Electronic".into()],
        song_count: Some(8),
        duration: Duration::ZERO,
        favorite: true,
        rating: Some(4),
        play_count: Some(37),
        songs: vec![],
        info: AlbumInfo {
            version: Some("Deluxe Edition".into()),
            original_date: Some("2019-11-08".into()),
            release_date: Some("2021-03-12".into()),
            last_played: Some("2026-10-14T21:30:00Z".into()),
            explicit: true,
            size: Some(262_000_000),
            comment: Some(
                "Recorded over two summers on the coast. The deluxe edition adds a second disc of \
                 reworks and live takes from the 2020 tour. Mastered for vinyl and streaming. "
                    .repeat(3),
            ),
            record_labels: vec!["Midnight Tapes".into()],
            moods: vec!["Dreamy".into(), "Nocturnal".into()],
            groupings: vec!["Night Drive Series".into()],
            tags: vec![("releasecountry".into(), vec!["DE".into()])],
            musicbrainz_id: Some("9f3d5b2a-1c4e-4b7a-8d21-0a5e6c7f8b90".into()),
            disc_titles: vec![(1, "Originals".into()), (2, "Reworks".into())],
        },
    };
    let titles = [
        ["Coastal Lights", "Afterglow Drive", "Static Hearts", "Midnight Mile"],
        ["Afterglow (Rework)", "Static Hearts (Live)", "Coastal Lights (Dub)", "Midnight Mile (Live)"],
    ];
    for (d, disc) in titles.iter().enumerate() {
        for (t, title) in disc.iter().enumerate() {
            let explicit = d == 0 && t == 2;
            album.songs.push(fixture_song(
                &album.clone(),
                SourceKind::Subsonic,
                d as u32 + 1,
                t as u32 + 1,
                title,
                explicit,
            ));
        }
    }
    album.duration = album.songs.iter().map(|s| s.duration).sum();
    album
}

/// YouTube Music album fixture: a `MPREb_` id, one disc, no ratings or OpenSubsonic metadata.
pub fn ytm_album() -> listen_core::model::Album {
    use listen_core::model::{Album, AlbumInfo};
    let mut album = Album {
        release_types: Vec::new(),
        id: "MPREb_demoAlbum01".into(),
        server_id: ServerId("yt".into()),
        source: SourceKind::YouTubeMusic,
        name: "Glass Cities".into(),
        album_artists: vec![ArtistRef { id: Some("UCdemoArtist01".into()), name: "Paper Satellites".into() }],
        image: None,
        year: Some(2023),
        release_type: Some("Single".into()),
        genres: vec![],
        song_count: Some(5),
        duration: Duration::ZERO,
        favorite: false,
        rating: None,
        play_count: None,
        songs: vec![],
        info: AlbumInfo { release_date: Some("2023".into()), ..AlbumInfo::default() },
    };
    for (t, title) in
        ["Glass Cities", "Low Orbit", "Signal Fires", "Harbour Lights", "Glass Cities (Instrumental)"]
            .iter()
            .enumerate()
    {
        let mut s = fixture_song(&album.clone(), SourceKind::YouTubeMusic, 1, t as u32 + 1, title, false);
        s.id = format!("ytDemoVid{t:02}");
        s.size = None;
        s.favorite = t == 1;
        album.songs.push(s);
    }
    album.duration = album.songs.iter().map(|s| s.duration).sum();
    album
}

/// A playlist fixture made of songs of the demo albums.
#[allow(clippy::too_many_arguments)]
fn playlist_fixture(
    id: &str,
    server: &str,
    source: SourceKind,
    name: &str,
    owner: &str,
    description: Option<&str>,
    public: bool,
    editable: bool,
    picks: &[(usize, usize)],
) -> crate::app::PlaylistDetail {
    let songs: Vec<Song> = picks
        .iter()
        .enumerate()
        .map(|(n, &(album, track))| {
            let mut songs = album_songs(album % ALBUMS.len());
            let mut s = songs.remove(track % songs.len());
            s.id = format!("{id}-{n}");
            s.server_id = ServerId(server.into());
            s.source = source;
            s.favorite = n % 4 == 1;
            s.rating = (source != SourceKind::YouTubeMusic && n % 3 == 2).then_some(4);
            s.year = Some(2018 + (n % 7) as i32);
            s
        })
        .collect();
    let playlist = listen_core::model::Playlist {
        id: id.into(),
        server_id: ServerId(server.into()),
        source,
        name: name.into(),
        description: description.map(String::from),
        owner: Some(owner.into()),
        image: None,
        song_count: Some(songs.len() as u32),
        duration: songs.iter().map(|s| s.duration).sum(),
        public,
        editable,
    };
    crate::app::PlaylistDetail { playlist, songs }
}

/// A Navidrome playlist page fixture: a description, public, owned by the user.
pub fn navidrome_playlist() -> crate::app::PlaylistDetail {
    playlist_fixture(
        "nd-pl-road",
        "nd",
        SourceKind::Subsonic,
        "Road trip",
        "alex",
        Some("Songs for the long drive north. Windows down, volume up. Shuffle it or play it in order."),
        true,
        true,
        &[(0, 0), (1, 1), (2, 2), (0, 3), (3, 0), (1, 4), (2, 1), (4, 2), (0, 5), (3, 3)],
    )
}

/// A YouTube Music playlist page fixture: private, no ratings.
pub fn ytm_playlist() -> crate::app::PlaylistDetail {
    playlist_fixture(
        "PLdemoYtm01",
        "yt",
        SourceKind::YouTubeMusic,
        "Late night mix",
        "You",
        None,
        false,
        true,
        &[(1, 0), (2, 1), (3, 2), (0, 2), (4, 0), (1, 3)],
    )
}

/// The playlists of the Playlists page fixture (Navidrome).
pub fn playlists() -> Vec<listen_core::model::Playlist> {
    let mut out = vec![navidrome_playlist().playlist];
    let more: [(&str, &str, &str, u32, u64, bool, bool); 5] = [
        ("nd-pl-gym", "Gym", "alex", 42, 9_400, false, true),
        ("nd-pl-chill", "Chill evenings", "alex", 18, 4_100, true, true),
        ("nd-pl-focus", "Deep focus", "amy", 64, 15_300, true, false),
        ("nd-pl-party", "Party starters", "amy", 31, 7_200, true, false),
        ("nd-pl-old", "Old favourites", "alex", 120, 29_800, false, true),
    ];
    for (id, name, owner, count, secs, public, editable) in more {
        out.push(listen_core::model::Playlist {
            id: id.into(),
            server_id: ServerId("nd".into()),
            source: SourceKind::Subsonic,
            name: name.into(),
            description: None,
            owner: Some(owner.into()),
            image: None,
            song_count: Some(count),
            duration: Duration::from_secs(secs),
            public,
            editable,
        });
    }
    out
}

/// Index of a demo album id (`demo-3` → 3).
pub fn album_index(id: &str) -> Option<usize> {
    id.strip_prefix("demo-")?.parse().ok()
}

const RATE: u32 = 22_050;
const TRACK_SECS: usize = 100;

/// Tempo of a demo song (distinct per album so blends have something to match).
pub fn bpm_for(song_id: &str) -> f32 {
    let album = parse_id(song_id).map_or(0, |(a, _)| a);
    [100.0, 110.0, 120.0, 124.0, 128.0, 132.0, 136.0, 140.0][album % 8]
}

fn parse_id(id: &str) -> Option<(usize, usize)> {
    let (a, t) = id.strip_prefix("demo-")?.split_once('-')?;
    Some((a.parse().ok()?, t.parse().ok()?))
}

/// Path of a generated 100 s clip for a demo song, written once: quiet intro, a groove at the
/// album's tempo, and a silent gap near the end.
pub fn audio_path(song_id: &str) -> Result<std::path::PathBuf, String> {
    let (album, track) = parse_id(song_id).ok_or("not a demo song")?;
    let dir = std::env::temp_dir().join(format!("listen-demo-audio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("demo-{album}-{track}.wav"));
    if path.exists() {
        return Ok(path);
    }
    let bpm = bpm_for(song_id);
    let beat = 60.0 / bpm;
    let root = 55.0 * 2f32.powf(((album * 3 + track * 5) % 12) as f32 / 12.0);
    let n = RATE as usize * TRACK_SECS;
    let mut pcm = Vec::with_capacity(n * 2);
    let mut noise = 0x1234_5678u32 ^ (album as u32 * 7919 + track as u32 * 104_729);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let phase = (t % beat) / beat;
        let in_beat = t % beat;
        let kick = (-in_beat * 28.0).exp()
            * (2.0 * std::f32::consts::PI * (50.0 + 90.0 * (-in_beat * 40.0).exp()) * in_beat).sin();
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let hat_t = (t + beat / 2.0) % beat;
        let hat = (noise as f32 / u32::MAX as f32 - 0.5) * (-hat_t * 60.0).exp() * 0.5;
        let bass = (2.0 * std::f32::consts::PI * root * t).sin() * (1.0 - phase) * 0.35;
        let lead = (2.0 * std::f32::consts::PI * root * 4.0 * t).sin() * 0.12;
        let gain = if t < 5.0 {
            0.12
        } else if ((TRACK_SECS as f32 - 9.0)..(TRACK_SECS as f32 - 6.0)).contains(&t) {
            0.0
        } else {
            0.8
        };
        let s = ((kick * 0.7 + hat + bass + lead) * gain).clamp(-1.0, 1.0);
        pcm.extend_from_slice(&((s * 30_000.0) as i16).to_le_bytes());
    }
    let mut wav = Vec::with_capacity(pcm.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, wav).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}
