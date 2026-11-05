//! `listen --check-youtube-music`: signs in the way the app does (the
//! browser's YouTube session) and prints what Listen sees of the library.
//! Only counts and page titles are printed — never cookies.

use listen_core::model::ServerId;
use listen_source::{AlbumQuery, AlbumSort, ListQuery, MusicSource, SongQuery};

pub fn youtube_music() -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(run()).map_or_else(
        |e| {
            eprintln!("✗ {e}");
            1
        },
        |()| 0,
    )
}

async fn run() -> Result<(), String> {
    let (mut profile, header) =
        tokio::task::spawn_blocking(listen_ytmusic::browser::detect).await.map_err(|e| e.to_string())??;
    println!("browser: {} ({})", profile.label(), profile.cookies.display());
    let channels = listen_ytmusic::channels::list(&header).await?;
    println!("channels:");
    for c in &channels {
        println!("  {}", c.label());
    }
    // The app asks; here the fullest library is used.
    let chosen = channels.first().ok_or("no channels")?;
    println!("using: {}", chosen.name);
    profile.page_id = chosen.page_id.clone();
    profile.channel = Some(chosen.name.clone());
    let yt = listen_ytmusic::YouTubeMusic::connect(ServerId("check".into()), &profile.to_secret())
        .await
        .map_err(|e| format!("sign-in: {e}"))?;
    yt.ping().await.map_err(|e| format!("signed-in request: {e}"))?;
    println!("✓ signed in");

    // How fast a big playlist shows up: first page, all pages, reopening.
    let t = std::time::Instant::now();
    if let Ok((first, mut next)) = yt.playlist_page(listen_ytmusic::LIKED_PLAYLIST, None).await {
        let first_page = t.elapsed();
        let mut total = first.len();
        while let Some(cursor) = next {
            match yt.playlist_page(listen_ytmusic::LIKED_PLAYLIST, Some(&cursor)).await {
                Ok((page, n)) => {
                    total += page.len();
                    next = n;
                }
                Err(e) => {
                    println!("✗ liked music page: {e}");
                    break;
                }
            }
        }
        let all = t.elapsed();
        let t2 = std::time::Instant::now();
        let _ = yt.playlist_page(listen_ytmusic::LIKED_PLAYLIST, None).await;
        println!(
            "liked music: first page ({}) after {:.1}s, all {total} after {:.1}s, reopening {:.2}s",
            first.len(),
            first_page.as_secs_f32(),
            all.as_secs_f32(),
            t2.elapsed().as_secs_f32()
        );
    }
    let songs = yt.songs(&SongQuery { limit: 100_000, ..Default::default() }).await;
    let liked = yt.songs(&SongQuery { limit: 100_000, favorites_only: true, ..Default::default() }).await;
    let albums = yt.albums(&AlbumQuery { sort: AlbumSort::Name, limit: 100_000, ..Default::default() }).await;
    let artists = yt.artists(&ListQuery { limit: 100_000, ..Default::default() }).await;
    let playlists = yt.playlists().await;
    let home = yt.home().await;
    let count =
        |r: &Result<usize, String>| r.as_ref().map_or_else(|e| format!("error: {e}"), |n| n.to_string());
    // Playback: resolve the audio stream of the first track (yt-dlp).
    if let Some(song) = songs.as_ref().ok().and_then(|p| p.items.first()) {
        match yt.stream(song).await {
            Ok(s) if s.url.starts_with("https://") => {
                println!("✓ stream resolves (yt-dlp) for a library track")
            }
            // Fetched before (DJ analysis/mixing cache): played from the local copy.
            Ok(s) if std::path::Path::new(&s.url).is_file() => {
                println!("✓ stream: plays from the local audio cache")
            }
            Ok(_) => println!("✗ stream: unexpected URL"),
            Err(e) => println!("✗ stream: {e}"),
        }
    }
    // LISTEN_CHECK_STREAMS=N: open N random library tracks in a muted mpv; failures are retried like the
    // player does (backup server, then a fresh URL).
    if let (Ok(n), Ok(page)) =
        (std::env::var("LISTEN_CHECK_STREAMS").map(|v| v.parse::<usize>().unwrap_or(20)), songs.as_ref())
    {
        use rand::seq::SliceRandom;
        let mut sample: Vec<_> = page.items.iter().collect();
        sample.shuffle(&mut rand::rng());
        let (mut first_try, mut via_backup, mut via_fresh, mut failed) = (0, 0, 0, Vec::new());
        for song in sample.into_iter().take(n) {
            let url = match yt.stream(song).await {
                Ok(s) => s.url,
                Err(e) => {
                    failed.push(format!("“{}” [{}]: resolve: {e}", song.title, song.id));
                    continue;
                }
            };
            let Err(e1) = mpv_opens(&url).await else {
                first_try += 1;
                continue;
            };
            if let Some(backup) = listen_ytmusic::ytdlp::fallback_url(&url)
                && mpv_opens(&backup).await.is_ok()
            {
                via_backup += 1;
                println!("  ↻ “{}”: backup server worked ({e1})", song.title);
                continue;
            }
            yt.forget_stream(song);
            match yt.stream(song).await {
                Ok(fresh) if mpv_opens(&fresh.url).await.is_ok() => {
                    via_fresh += 1;
                    println!("  ↻ “{}”: fresh URL worked ({e1})", song.title);
                }
                Ok(_) => failed.push(format!("“{}” [{}]: {e1}", song.title, song.id)),
                Err(e) => failed.push(format!("“{}” [{}]: re-resolve: {e}", song.title, song.id)),
            }
        }
        println!(
            "streams: {first_try} played at once, {via_backup} via backup server, {via_fresh} via fresh URL, {} failed",
            failed.len()
        );
        for f in &failed {
            println!("  ✗ {f}");
        }
    }
    let songs_n = songs.as_ref().map(|p| p.items.len()).map_err(|e| e.to_string());
    println!("tracks (library songs): {}", count(&songs_n));
    println!("liked songs: {}", count(&liked.map(|p| p.items.len()).map_err(|e| e.to_string())));
    println!("albums: {}", count(&albums.as_ref().map(|p| p.items.len()).map_err(|e| e.to_string())));
    match &artists {
        Ok(p) => {
            let names: Vec<&str> = p.items.iter().take(5).map(|a| a.name.as_str()).collect();
            println!("artists (subscribed artist channels): {} — e.g. {}", p.items.len(), names.join(", "));
        }
        Err(e) => println!("artists: error: {e}"),
    }
    // What "Add to playlist" offers for the first library track (read-only).
    if let Some(song) = songs.as_ref().ok().and_then(|p| p.items.first()) {
        match yt.playlists_for_adding(std::slice::from_ref(&song.id)).await {
            Ok(list) => println!(
                "add to playlist: {} playlists offered, {} already hold the first track",
                list.len(),
                list.iter().filter(|p| !p.editable).count()
            ),
            Err(e) => println!("add to playlist: error: {e}"),
        }
    }
    match &playlists {
        Ok(p) => println!("playlists: {}", p.len()),
        Err(e) => println!("playlists: error: {e}"),
    }
    // The "Recommended" strip of a playlist (read-only): its radio seeds, minus its own songs.
    if let Ok(list) = &playlists
        && let Some(p) = list.iter().find(|p| p.song_count.unwrap_or(0) > 2)
    {
        match yt.playlist_songs(&p.id).await {
            Ok(songs) => match yt.playlist_recommendations(&songs, 5).await {
                Ok(rec) => {
                    let dupes = rec.iter().filter(|r| songs.iter().any(|s| s.id == r.id)).count();
                    let names: Vec<&str> = rec.iter().take(5).map(|r| r.title.as_str()).collect();
                    println!(
                        "recommended: {} songs for \"{}\" ({dupes} already in it) — {}",
                        rec.len(),
                        p.name,
                        names.join(", ")
                    );
                }
                Err(e) => println!("recommended: error: {e}"),
            },
            Err(e) => println!("recommended: could not read \"{}\": {e}", p.name),
        }
    }
    // Opens a random handful of playlists and albums with human pauses: this is a real account and must not
    // look like a bot. LISTEN_CHECK_ALL=1 opens all of them (still with short pauses).
    let all = std::env::var("LISTEN_CHECK_ALL").is_ok();
    let pause = || async move {
        let ms = if all { rand::random_range(300..=900) } else { rand::random_range(2000..=5000) };
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    };
    let sample = |n: usize| -> Vec<usize> {
        use rand::seq::SliceRandom;
        let mut idx: Vec<usize> = (0..n).collect();
        if !all {
            idx.shuffle(&mut rand::rng());
            idx.truncate(5);
        }
        idx
    };
    if let Ok(list) = &playlists {
        let (mut songs, mut failed) = (0, Vec::new());
        let picked = sample(list.len());
        for p in picked.iter().map(|&i| &list[i]) {
            pause().await;
            match yt.playlist_songs(&p.id).await {
                Ok(s) => songs += s.len(),
                Err(e) => failed.push(format!("{}: {e}", p.name)),
            }
        }
        println!(
            "  opened {} playlists ({songs} songs), {} failed",
            picked.len() - failed.len(),
            failed.len()
        );
        for f in failed.iter().take(5) {
            println!("    ✗ {f}");
        }
    }
    if let Ok(page) = &albums {
        let mut failed = Vec::new();
        let picked = sample(page.items.len());
        for a in picked.iter().map(|&i| &page.items[i]) {
            pause().await;
            if let Err(e) = yt.album(&a.id).await {
                failed.push(format!("{} [{}]: {e}", a.name, a.id));
            }
        }
        println!("  opened {} albums, {} failed", picked.len() - failed.len(), failed.len());
        if let Some(id) = failed
            .first()
            .and_then(|f| f.rsplit_once(" ["))
            .and_then(|(_, r)| r.split_once(']'))
            .map(|(id, _)| id.to_string())
            && std::env::var("LISTEN_CHECK_SHAPE").is_ok()
            && let Ok(v) = yt.raw_browse(&id).await
        {
            let mut lines = Vec::new();
            shape(&v, 0, &mut lines);
            println!("  page structure of {id}:");
            for l in lines.iter().take(40) {
                println!("    {l}");
            }
        }
        for f in failed.iter().take(5) {
            println!("    ✗ {f}");
        }
    }
    match home {
        Ok(shelves) => {
            println!("home shelves: {}", shelves.len());
            for s in shelves {
                println!(
                    "  {}: {} albums, {} playlists, {} songs",
                    s.title,
                    s.albums.len(),
                    s.playlists.len(),
                    s.songs.len()
                );
            }
        }
        Err(e) => println!("home: error: {e}"),
    }
    if songs_n.as_ref().is_ok_and(|n| *n > 0) { Ok(()) } else { Err("no tracks found".into()) }
}

/// Keys of a JSON response down a few levels (structure only, no values).
fn shape(v: &serde_json::Value, depth: usize, out: &mut Vec<String>) {
    if depth > 7 {
        return;
    }
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                if matches!(x, serde_json::Value::Object(_) | serde_json::Value::Array(_)) {
                    out.push(format!("{}{k}", "  ".repeat(depth)));
                    shape(x, depth + 1, out);
                }
            }
        }
        serde_json::Value::Array(a) => {
            if let Some(x) = a.first() {
                shape(x, depth, out);
            }
        }
        _ => {}
    }
}

/// Lets mpv (muted) play two seconds of `url`; the error is mpv's last message.
async fn mpv_opens(url: &str) -> Result<(), String> {
    let mpv = std::env::var("LISTEN_MPV")
        .unwrap_or_else(|_| listen_core::platform::tool("mpv").to_string_lossy().into_owned());
    let mut cmd = tokio::process::Command::new(&mpv);
    listen_core::platform::no_window(cmd.as_std_mut());
    let out = cmd
        .args([
            "--no-config",
            "--ao=null",
            "--no-video",
            "--msg-level=all=warn",
            "--end=2",
            "--network-timeout=10",
        ])
        .args(["--ytdl=no", url])
        .output()
        .await
        .map_err(|e| format!("can't run mpv: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr);
    let reason = text
        .lines()
        .find_map(|l| {
            ["Connection refused", "403", "404", "timed out", "Failed to open"]
                .iter()
                .find(|k| l.contains(**k))
                .map(|k| k.to_string())
        })
        .unwrap_or_else(|| format!("exit {:?}", out.status.code()));
    Err(reason)
}

/// What `listen --check-ai-dj` does.
#[derive(Debug, Clone)]
pub struct AiDjCheck {
    /// Speak the first two breaks (`--no-voice` = never).
    pub voice: bool,
    /// Planning rounds (`--rounds N`, default 2).
    pub rounds: usize,
    /// Grade every break with a second model call (`--no-judge` = off).
    pub judge: bool,
    /// Print each round's prompt (`--dump-prompt`).
    pub dump: bool,
    /// "Tell the DJ" from round 2 on (`--request calmer`, repeatable; chip ids or free text).
    pub requests: listen_dj::Requests,
}

impl AiDjCheck {
    pub fn from_args(args: &[String]) -> Self {
        let rounds = args
            .iter()
            .position(|a| a == "--rounds")
            .and_then(|i| args.get(i + 1))
            .and_then(|n| n.parse().ok())
            .unwrap_or(2usize)
            .clamp(1, 50);
        Self {
            voice: !args.iter().any(|a| a == "--no-voice"),
            rounds,
            judge: !args.iter().any(|a| a == "--no-judge"),
            dump: args.iter().any(|a| a == "--dump-prompt"),
            requests: {
                let mut r = listen_dj::Requests::default();
                for w in args.windows(2).filter(|w| w[0] == "--request") {
                    if let Some(s) = listen_dj::Chip::from_id(&w[1])
                        .map(listen_dj::Steer::Chip)
                        .or_else(|| listen_dj::Steer::text(&w[1]))
                    {
                        r.add(s);
                    }
                }
                r
            },
        }
    }
}

/// `listen --check-ai-dj [--no-voice] [--rounds N] [--no-judge] [--dump-prompt] [--request R]…`: AI DJ rounds against
/// Gemini on a fixed song pool with listening-history tags and measured tempo/key/energy; prints picks,
/// lines, timings and tokens, grades every break with a judge call, saves the first two spoken breaks as
/// WAVs in the temp dir. Key: `$LISTEN_GEMINI_API_KEY` or the app's keyring entry. Costs one text request
/// per round, one more per round with the judge, and up to 2 speech requests. Fails when a break fails
/// the judge. `--request` (a chip id such as `calmer`, `more_energy`, `no_ballads`, or free text) is told to
/// the DJ before round 2: the pool is filtered as in the app, and for Calmer / More energy the next 5 songs
/// must have a lower / higher mean energy than round 1's set (the songs the request replaced).
pub fn ai_dj(check: AiDjCheck) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(run_ai_dj(check)).map_or_else(
        |e| {
            eprintln!("✗ {e}");
            1
        },
        |()| 0,
    )
}

fn check_song(n: usize, title: &str, artist: &str, year: i32, genre: &str) -> listen_core::model::Song {
    use listen_core::model::{ArtistRef, GainInfo, Song, SourceKind};
    Song {
        mbz: Default::default(),
        extra: Default::default(),
        id: format!("check-{n}"),
        server_id: ServerId("check".into()),
        source: SourceKind::Subsonic,
        title: title.into(),
        artists: vec![ArtistRef { id: None, name: artist.into() }],
        album_artists: vec![],
        album: None,
        album_id: None,
        image: None,
        duration: std::time::Duration::from_secs(240),
        track_number: None,
        disc_number: None,
        year: Some(year),
        genres: vec![genre.into()],
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

/// Sample analysis for the check pool (bpm, Camelot number, minor, energy 0..1).
fn check_flow(bpm: f32, key: u8, minor: bool, energy: f32) -> listen_core::analysis::FlowFeatures {
    listen_core::analysis::FlowFeatures {
        bpm: Some(bpm),
        bpm_confidence: 0.9,
        key: Some((listen_core::analysis::Camelot { number: key, minor }, 0.8)),
        lufs: -10.0,
        mood: listen_core::analysis::Mood { energy, danceability: 0.5, brightness: 0.5 },
        timbre: vec![],
        mixable_in: true,
        mixable_out: true,
        vocal: None,
    }
}

/// A check pool song: title, artist, year, genre, (bpm, Camelot number, minor, energy), history tag.
type CheckSong = (&'static str, &'static str, i32, &'static str, (f32, u8, bool, f32), &'static str);

async fn run_ai_dj(check: AiDjCheck) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};
    let key = std::env::var("LISTEN_GEMINI_API_KEY").ok().filter(|k| !k.trim().is_empty()).or_else(|| {
        let paths = listen_core::config::Paths::discover().ok()?;
        crate::secrets::Secrets::new(&paths.config_dir).get(&ServerId("gemini-api-key".into()))
    });
    let key = key.ok_or("no key: set LISTEN_GEMINI_API_KEY or save one in Settings → AI DJ")?;
    let settings = listen_core::config::Paths::discover()
        .ok()
        .and_then(|p| listen_core::config::Config::load(&p.config_file()).ok())
        .map(|c| c.settings.ai_dj)
        .unwrap_or_default();
    let persona =
        listen_dj::Persona::by_id(&std::env::var("LISTEN_AI_DJ_PERSONA").unwrap_or(settings.persona.clone()));
    let http = reqwest::Client::new();
    let gemini = listen_dj::Gemini::new(http, key).map_err(|e| e.to_string())?;
    // Title, artist, year, genre, (bpm, Camelot, minor, energy), what the listener knows.
    #[rustfmt::skip]
    let songs: [CheckSong; 20] = [
        ("Teardrop", "Massive Attack", 1998, "Trip-Hop", (77.0, 4, true, 0.35), "favourite, plays 23, last played 2 days ago"),
        ("Roads", "Portishead", 1994, "Trip-Hop", (69.0, 6, true, 0.25), "plays 6, last played 12 days ago"),
        ("Angel", "Massive Attack", 1998, "Trip-Hop", (78.0, 4, true, 0.45), "new to you"),
        ("Hyperballad", "Björk", 1995, "Electronic", (120.0, 7, false, 0.55), "new to you, new artist"),
        ("Cherry Blossom Girl", "Air", 2004, "Downtempo", (92.0, 9, true, 0.3), "plays 3, not heard for 4 months"),
        ("Intro", "The xx", 2009, "Indie", (100.0, 6, true, 0.45), "favourite, plays 31, last played yesterday"),
        ("Breathe", "Télépopmusik", 2001, "Downtempo", (78.0, 3, true, 0.35), "new to you, new artist"),
        ("Midnight City", "M83", 2011, "Synthpop", (105.0, 4, false, 0.75), "plays 9, last played 20 days ago"),
        ("Mr. Brightside", "The Killers", 2004, "Rock", (148.0, 1, false, 0.9), "plays 2, last played 40 days ago"),
        ("Strobe", "deadmau5", 2009, "Progressive House", (128.0, 9, false, 0.6), "new to you, new artist"),
        ("Holocene", "Bon Iver", 2011, "Indie Folk", (74.0, 10, false, 0.25), "favourite, plays 15, not heard for 3 months"),
        ("Nightcall", "Kavinsky", 2010, "Synthwave", (91.0, 8, true, 0.5), "new to you"),
        ("Kids", "MGMT", 2007, "Indie", (123.0, 12, false, 0.7), "plays 4, last played 8 days ago"),
        ("Sour Times", "Portishead", 1994, "Trip-Hop", (100.0, 9, true, 0.4), "new to you"),
        ("Sweet Disposition", "The Temper Trap", 2008, "Indie Rock", (126.0, 2, false, 0.75), "plays 1, last played 3 days ago"),
        ("Bloodstream", "Stateless", 2007, "Trip-Hop", (75.0, 5, true, 0.35), "new to you, new artist"),
        ("Porcelain", "Moby", 1999, "Downtempo", (96.0, 8, true, 0.3), "plays 5, not heard for 6 months"),
        ("Smells Like Teen Spirit", "Nirvana", 1991, "Grunge", (117.0, 4, true, 0.95), "plays 2, last played 25 days ago"),
        ("Unfinished Sympathy", "Massive Attack", 1991, "Trip-Hop", (100.0, 7, true, 0.5), "favourite, plays 11, last played 6 days ago"),
        ("Glory Box", "Portishead", 1994, "Trip-Hop", (60.0, 4, true, 0.3), "new to you"),
    ];
    let all: Vec<_> = songs.iter().enumerate().map(|(n, s)| check_song(n, s.0, s.1, s.2, s.3)).collect();
    let flow: HashMap<String, listen_core::analysis::FlowFeatures> = songs
        .iter()
        .zip(&all)
        .map(|(s, song)| (song.id.clone(), check_flow(s.4.0, s.4.1, s.4.2, s.4.3)))
        .collect();
    let familiar: HashMap<String, String> =
        songs.iter().zip(&all).map(|(s, song)| (song.id.clone(), s.5.to_string())).collect();
    let seed = all[0].clone();
    // The judge may be a stronger model than the DJ's (`LISTEN_AI_DJ_JUDGE_MODEL`).
    let judge_model = std::env::var("LISTEN_AI_DJ_JUDGE_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| settings.text_model.clone());
    let language = crate::i18n::english_name(crate::i18n::dj_language(&settings.language));
    println!(
        "persona: {} (voice {}), text {}, speech {}, {} rounds, judge {} ({judge_model})",
        persona.name,
        persona.voice,
        settings.text_model,
        settings.tts_model,
        check.rounds,
        if check.judge { "on" } else { "off" }
    );
    println!("seed: {} – {}", seed.title, seed.artist_line());

    let mut played: Vec<listen_dj::Played> = Vec::new();
    let mut current = seed.clone();
    let (mut mood, mut last_line) = (None::<String>, None::<String>);
    let mut seed_mood = None::<String>;
    let mut used: HashSet<String> = [seed.id.clone()].into();
    let (mut plan_tokens, mut judge_tokens) = (listen_dj::Usage::default(), listen_dj::Usage::default());
    let (mut failed, mut new_sets, mut introduced, mut named_new) = (0usize, 0usize, 0usize, 0usize);
    let (mut noted, mut picked) = (0usize, 0usize);
    // Tell the DJ: no requests in round 1, then `check.requests`; set energies to compare.
    let none = listen_dj::Requests::default();
    let mut first_energy = None::<f32>;
    // Energies of the songs picked after the request, in play order (the first 5 are compared).
    let mut after: Vec<f32> = Vec::new();
    let energy_of = |s: &listen_core::model::Song| listen_dj::steer::Facts {
        energy: flow.get(&s.id).map(|f| f.mood.energy),
        bpm: flow.get(&s.id).and_then(|f| f.bpm),
        new_to_you: familiar.get(&s.id).is_some_and(|t| t.starts_with("new to you")),
        plays: familiar
            .get(&s.id)
            .and_then(|t| t.split("plays ").nth(1))
            .and_then(|t| t.split(',').next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0),
    };
    for round in 1..=check.rounds {
        let asked = round > 1 && !check.requests.is_empty();
        let requests = if asked { &check.requests } else { &none };
        let mut pool: Vec<_> = all.iter().filter(|s| !used.contains(&s.id)).cloned().collect();
        // Long runs: the pool starts over (except the song playing now).
        if pool.len() < settings.set_max as usize + 3 {
            used = [current.id.clone()].into();
            pool = all.iter().filter(|s| !used.contains(&s.id)).cloned().collect();
        }
        if asked {
            let before = pool.len();
            pool = listen_dj::steer::filter_pool(pool, energy_of, requests, first_energy);
            println!("requests: {} candidates of {before} fit", pool.len());
        }
        let now = flow.get(&current.id);
        let mut notes: HashMap<String, String> =
            all.iter().map(|s| (s.id.clone(), flow[&s.id].describe())).collect();
        for s in &pool {
            notes.insert(s.id.clone(), listen_dj::facts::candidate_note(&flow[&s.id], now));
        }
        let req = listen_dj::PlanRequest {
            persona,
            seed: &seed,
            seed_context: &[],
            listener_picks: &[],
            mood: mood.as_deref(),
            seed_mood: seed_mood.as_deref(),
            played: &played,
            current: Some(&current),
            last_line: last_line.as_deref(),
            candidates: &pool,
            notes: &notes,
            familiar: &familiar,
            set_min: settings.set_min as usize,
            set_max: settings.set_max as usize,
            first: round == 1,
            mode: listen_dj::BreakMode::ALL[round % 4],
            // Two-host shows: the co-host joins every other round.
            duo: round % 2 == 0,
            show: listen_dj::Show { language, location: &settings.location, notes: &settings.notes },
            requests,
            requests_fresh: round == 2,
        };
        if check.dump {
            println!("\n— prompt {round}:\n{}", listen_dj::prompt::user_prompt(&req));
        }
        let t = std::time::Instant::now();
        let (plan, usage) =
            gemini.plan(&settings.text_model, &req).await.map_err(|e| format!("plan: {e}"))?;
        plan_tokens = add_usage(plan_tokens, usage);
        println!("\n— round {round}: plan in {:.1}s ({usage})", t.elapsed().as_secs_f32());
        println!("break: {}", req.mode.name());
        println!("mood: {}", plan.mood);
        for (k, &i) in plan.picks.iter().enumerate() {
            let tag = familiar.get(&pool[i].id).map(|t| format!(" [{t}]")).unwrap_or_default();
            println!("  ♪ {} – {}{tag}", pool[i].title, pool[i].artist_line());
            // "Why this song?": the DJ's note for the queue (≤ 80 characters).
            let note = plan.notes.get(k).map(String::as_str).unwrap_or_default();
            noted += usize::from(!note.is_empty());
            picked += 1;
            println!("      why: {}", if note.is_empty() { "(no note)" } else { note });
        }
        for t in &plan.turns {
            println!("{}: {}  [{}]", if t.speaker.is_empty() { "line" } else { &t.speaker }, t.text, t.style);
        }
        if check.voice && round <= 2 {
            let t = std::time::Instant::now();
            let (wav, usage) = gemini
                .speak(&settings.tts_model, persona, &plan.turns, settings.place_pronunciation().as_slice())
                .await
                .map_err(|e| format!("speech: {e}"))?;
            let pcm = listen_dj::wav::decode(&wav)?;
            let secs = pcm.duration_secs();
            let pcm = listen_dj::wav::prepare(pcm, -17.0, settings.voice_gain_db, 0.35);
            let out = std::env::temp_dir().join(format!("listen-ai-dj-check-{round}.wav"));
            std::fs::write(&out, listen_dj::wav::encode(&pcm)).map_err(|e| e.to_string())?;
            println!(
                "speech in {:.1}s ({usage}; {secs:.1}s audio, {} Hz) → {}",
                t.elapsed().as_secs_f32(),
                pcm.rate,
                out.display()
            );
        }
        if check.judge && !plan.turns.is_empty() {
            let line_of = |s: &listen_core::model::Song| {
                let tag = familiar.get(&s.id).map(|t| format!(" [{t}]")).unwrap_or_default();
                format!(
                    "{} – {} ({}, {}){tag}",
                    s.title,
                    s.artist_line(),
                    s.year.unwrap_or(0),
                    s.genres.join("/")
                )
            };
            let line = plan.line();
            let case = listen_dj::judge::Case {
                persona: persona.brief,
                language,
                hosts: match &persona.cohost {
                    Some(co) if req.duo => format!("Both {} and {} talk this break.", persona.host, co.name),
                    Some(_) => format!("Only {} talks this break.", persona.host),
                    None => String::new(),
                },
                played: played.iter().map(|p| &p.song).chain([&current]).map(line_of).collect(),
                set: plan.picks.iter().map(|&i| line_of(&pool[i])).collect(),
                line: &line,
                previous_line: last_line.as_deref(),
            };
            let (verdict, usage) =
                gemini.judge(&judge_model, &case).await.map_err(|e| format!("judge: {e}"))?;
            judge_tokens = add_usage(judge_tokens, usage);
            let pass = verdict.passed(&case);
            if case.has_new() {
                new_sets += 1;
                introduced += usize::from(verdict.introduces_new);
            }
            failed += usize::from(!pass);
            println!("judge: {} {} ({usage})", if pass { "PASS" } else { "FAIL" }, verdict.summary(&case));
            if case.has_new() {
                // Did the model name a new-to-you song of its set as the one it introduces?
                let said = plan.introduces.to_lowercase();
                let named = plan.picks.iter().map(|&i| &pool[i]).find(|s| {
                    let title = s.title.to_lowercase();
                    !said.is_empty() && (said.contains(&title) || title.contains(&said))
                });
                let new =
                    named.is_some_and(|s| familiar.get(&s.id).is_some_and(|t| t.starts_with("new to you")));
                named_new += usize::from(new);
                println!(
                    "introduces: “{}”{}",
                    plan.introduces,
                    if new { "" } else { " (not a new-to-you song of the set)" }
                );
            }
        }
        // The set "plays"; the last song of it is playing when the next plan is made.
        let picks: Vec<_> = plan.picks.iter().map(|&i| pool[i].clone()).collect();
        if let Some(e) = listen_dj::steer::mean_energy(&picks, energy_of) {
            println!("set energy: {e:.2}");
            if first_energy.is_none() {
                first_energy = Some(e);
            } else {
                after.extend(picks.iter().filter_map(|s| energy_of(s).energy));
            }
        }
        for s in &picks {
            used.insert(s.id.clone());
        }
        played = picks[..picks.len() - 1]
            .iter()
            .map(|s| listen_dj::Played { song: s.clone(), skipped: false, by_user: false, mixed_in: None })
            .collect();
        current = picks.last().cloned().unwrap_or(current);
        seed_mood.get_or_insert_with(|| plan.mood.clone());
        last_line = Some(plan.line());
        mood = Some(plan.mood);
    }
    let n = check.rounds as u64;
    if !check.requests.is_empty() {
        let asked: Vec<&str> = check.requests.items.iter().map(listen_dj::Steer::label).collect();
        let next: Vec<f32> = after.iter().copied().take(5).collect();
        let mean = (!next.is_empty()).then(|| next.iter().sum::<f32>() / next.len() as f32);
        let before = first_energy.unwrap_or(0.5);
        println!(
            "requests ({}): the next {} songs average energy {} (the replaced set: {before:.2})",
            asked.join(", "),
            next.len(),
            mean.map_or("?".into(), |m| format!("{m:.2}"))
        );
        let calmer = check.requests.has(listen_dj::Chip::Calmer);
        let livelier = check.requests.has(listen_dj::Chip::MoreEnergy);
        if let Some(m) = mean
            && ((calmer && m >= before) || (livelier && m <= before))
        {
            return Err(format!("the request was not followed: next songs {m:.2} vs {before:.2} before"));
        }
    }
    println!("songs with a note (\"Why this song?\"): {noted}/{picked}");
    if noted * 5 < picked * 4 {
        return Err(format!("only {noted} of {picked} picked songs got a note"));
    }
    println!(
        "\ntokens per plan: {} in / {} out (total {plan_tokens}){}",
        plan_tokens.input / n,
        plan_tokens.output / n,
        if check.judge { format!("; judge total {judge_tokens}") } else { String::new() }
    );
    if check.judge {
        println!(
            "judge: {}/{} breaks passed; {introduced}/{new_sets} sets with a new-to-you song introduced one \
             ({named_new} named it in `introduces`)",
            check.rounds - failed,
            check.rounds
        );
        // Single breaks vary from run to run: the check wants 80 % of breaks and of introductions.
        let short = |ok: usize, of: usize| ok * 5 < of * 4;
        if short(check.rounds - failed, check.rounds) || short(introduced, new_sets) {
            return Err(format!(
                "below 80 %: {failed} of {} breaks failed the judge, {introduced}/{new_sets} introductions",
                check.rounds
            ));
        }
    }
    println!("\n✓ AI DJ works");
    Ok(())
}

fn add_usage(a: listen_dj::Usage, b: listen_dj::Usage) -> listen_dj::Usage {
    listen_dj::Usage { input: a.input + b.input, output: a.output + b.output, thought: a.thought + b.thought }
}

/// `listen --check-cd`: what CD ripping would do, without ripping — tools,
/// drives, the disc and its TOC, MusicBrainz's matches, the destination
/// folder and the Navidrome server. Never prints passwords.
pub fn cd() -> i32 {
    match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(run_cd()),
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

async fn run_cd() -> i32 {
    use listen_rip::{cover, drive, musicbrainz, publish, whipper};
    let mut problems = 0;
    let paths = listen_core::config::Paths::discover().ok();
    let config = paths
        .as_ref()
        .and_then(|p| listen_core::config::Config::load(&p.config_file()).ok())
        .unwrap_or_default();
    let settings = &config.settings.ripping;

    println!("Tools:");
    let tools: [(&str, &[&str], bool); 6] = [
        ("whipper", &["--version"], true),
        ("flac", &["--version"], true),
        ("metaflac", &["--version"], true),
        ("cd-paranoia", &["--version"], true),
        ("cdrdao", &[], true),
        ("pw-play", &["--version"], false),
    ];
    for (tool, args, needed) in tools {
        match tokio::process::Command::new(tool).args(args).output().await {
            Ok(o) => {
                let text =
                    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
                let line = text
                    .lines()
                    .find(|l| l.to_lowercase().contains("version") || l.chars().any(|c| c.is_ascii_digit()))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                println!("  ✓ {tool}: {line}");
            }
            Err(_) if needed => {
                problems += 1;
                println!("  ✗ {tool}: not installed (sudo dnf install whipper flac cdrdao)");
            }
            Err(_) => println!("  - {tool}: not installed (no completion sound)"),
        }
    }

    println!("\nDrives:");
    let drives = drive::list();
    if drives.is_empty() {
        problems += 1;
        println!("  ✗ no optical drive (/dev/sr*) — is the USB drive plugged in?");
    }
    let http = musicbrainz::http_client();
    for d in &drives {
        let offset = match whipper::resolve_offset(settings.read_offset, d) {
            Some((o, src)) => format!("read offset {o:+} ({})", src.label()),
            None => {
                problems += 1;
                "read offset UNKNOWN — set it in Settings → CD ripping".into()
            }
        };
        println!("  {} at {} — {offset}", d.label(), d.device.display());
        let state = drive::disc_state(&d.device);
        println!("  disc: {state:?}");
        if state != drive::DiscState::Audio {
            continue;
        }
        let toc = match drive::read_toc(&d.device) {
            Ok(t) => t,
            Err(e) => {
                problems += 1;
                println!("  ✗ TOC: {e}");
                continue;
            }
        };
        println!(
            "  TOC: {} tracks ({} audio), {:.1} min",
            toc.tracks.len(),
            toc.audio_count(),
            toc.duration_ms() as f64 / 60_000.0
        );
        for (t, len) in toc.audio_tracks().zip(toc.track_lengths_ms()) {
            println!("    {:2}  start {:6}  {}:{:02}", t.number, t.lba, len / 60_000, len / 1000 % 60);
        }
        for t in toc.tracks.iter().filter(|t| !t.audio) {
            println!("    {:2}  start {:6}  data track (not ripped)", t.number, t.lba);
        }
        println!("  MusicBrainz disc ID: {}", toc.musicbrainz_id());
        println!("  TOC string: {}", toc.toc_string());
        match musicbrainz::lookup(&http, &toc).await {
            Ok(rs) if rs.is_empty() => {
                println!(
                    "  MusicBrainz: unknown disc — it would be tagged from CD-Text or as “Unknown Album”."
                );
                println!("  Add it: {}", toc.submit_url());
            }
            Ok(rs) => {
                let order = musicbrainz::rank(&rs, &toc);
                println!("  MusicBrainz: {} matching release(s), best first:", rs.len());
                for (n, i) in order.iter().enumerate() {
                    let r = &rs[*i];
                    let medium = r.medium_for(&toc.musicbrainz_id(), toc.audio_count());
                    println!(
                        "   {} {} – {} [{}]{}{}",
                        if n == 0 { "→" } else { " " },
                        r.artist(),
                        r.title,
                        r.details(),
                        medium
                            .map(|m| format!(" disc {}/{}", r.media[m].position, r.media.len()))
                            .unwrap_or_default(),
                        if r.has_front_cover { " (cover)" } else { "" }
                    );
                }
                let best = &rs[order[0]];
                let meta = listen_rip::tags::AlbumMeta::from_release(
                    best,
                    best.medium_for(&toc.musicbrainz_id(), toc.audio_count()).unwrap_or(0),
                    &toc,
                );
                println!("  would file into: {}/", listen_rip::layout::album_dir(&meta).display());
                if let Some(t) = meta.tracks.first() {
                    println!("    e.g. {}", listen_rip::layout::track_file(&meta, t));
                }
                match cover::fetch(&http, best).await {
                    Some(c) => println!("  cover: {} ({} KB)", c.source, c.bytes.len() / 1024),
                    None => println!("  cover: none found"),
                }
            }
            Err(e) => {
                problems += 1;
                println!("  ✗ MusicBrainz: {e}");
            }
        }
    }

    println!("\nDestination:");
    let dest = std::path::PathBuf::from(settings.destination.trim());
    let d2 = dest.clone();
    match tokio::task::spawn_blocking(move || publish::check(&d2, 800 << 20)).await {
        Ok(Ok(())) => println!("  ✓ {} is mounted and writable (room for a CD)", dest.display()),
        Ok(Err(e)) => {
            problems += 1;
            println!("  ✗ {e}");
        }
        Err(e) => println!("  ✗ {e}"),
    }
    println!(
        "  auto-rip {}, eject {}, ReplayGain {}, Navidrome scan {}",
        on(settings.auto_rip),
        on(settings.eject_when_done),
        on(settings.replay_gain),
        on(settings.scan_navidrome)
    );

    println!("\nNavidrome:");
    let secrets = paths.as_ref().map(|p| crate::secrets::Secrets::new(&p.config_dir));
    let nd = |s: &&listen_core::config::ServerConfig| s.kind == listen_core::model::SourceKind::Subsonic;
    let server = config.active().filter(nd).or_else(|| config.servers.iter().find(nd));
    match (server, secrets) {
        (Some(s), Some(secrets)) => match secrets.get(&s.id) {
            Some(pw) => match listen_subsonic::Subsonic::new(s.id.clone(), &s.url, &s.username, &pw) {
                Ok(client) => match client.scan_status().await {
                    Ok(st) => println!(
                        "  ✓ {} as {}: reachable ({}, {} files). Starting a scan after a rip needs an admin account.",
                        s.url,
                        s.username,
                        if st.scanning { "scanning" } else { "idle" },
                        st.count
                    ),
                    Err(e) => println!("  ✗ {}: {e}", s.url),
                },
                Err(e) => println!("  ✗ {}: {e}", s.url),
            },
            None => println!("  - {}: no stored password (sign in once in Listen)", s.url),
        },
        _ => println!("  - no Navidrome server configured; its file watcher picks up new albums anyway"),
    }
    println!();
    if problems == 0 {
        println!("✓ ready to rip");
        0
    } else {
        println!("✗ {problems} problem(s) above");
        1
    }
}

fn on(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

/// `listen --check-analysis [N | <file or URL> | ytm:<videoId> | <YouTube Music link> | ytm [N]]`:
/// analyses N random Navidrome songs (default 20) into the shared store and compares tempo with BPM tags,
/// or prints one file's / URL's full analysis. The `ytm` forms fetch through the YouTube Music source
/// (a videoId prints its FileId and store key; `ytm N`: N unanalysed library songs, default 10).
pub fn analysis(args: Vec<String>) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(run_analysis(args)).map_or_else(
        |e| {
            eprintln!("✗ {e}");
            1
        },
        |()| 0,
    )
}

/// What `--check-analysis` should analyse on YouTube Music.
#[derive(Debug, PartialEq)]
enum YtmTarget {
    Song(String),
    Library(usize),
}

/// `ytm:<id>`, a youtube.com/youtu.be link (`v=` or the path), or `ytm [N]`.
fn ytm_target(args: &[String]) -> Option<YtmTarget> {
    let first = args.first()?.trim();
    if first == "ytm" {
        return Some(YtmTarget::Library(args.get(1).and_then(|n| n.parse().ok()).unwrap_or(10)));
    }
    if let Some(id) = first.strip_prefix(listen_analysis::YTM_PREFIX) {
        return Some(YtmTarget::Song(id.to_string()));
    }
    if !(first.contains("youtube.com/") || first.contains("youtu.be/")) {
        return None;
    }
    let id = match first.split_once("v=") {
        Some((_, rest)) => rest.split(['&', '#']).next()?,
        None => first.split('?').next()?.trim_end_matches('/').rsplit('/').next()?,
    };
    (!id.is_empty()).then(|| YtmTarget::Song(id.to_string()))
}

/// The shared analysis folder as the app picks it.
fn shared_dir(
    config: &listen_core::config::Config,
    paths: &listen_core::config::Paths,
) -> std::path::PathBuf {
    let s = &config.settings.analysis;
    if s.shared_dir.trim().is_empty() {
        let home = listen_core::platform::home_dir().unwrap_or_default();
        listen_analysis::store::detect_shared_dir(&home, &paths.data_dir.join("analysis"))
    } else {
        std::path::PathBuf::from(s.shared_dir.trim())
    }
}

async fn run_ytm_analysis(
    paths: &listen_core::config::Paths,
    beats: listen_analysis::BeatSource,
    target: YtmTarget,
) -> Result<(), String> {
    use listen_analysis::{Cancel, Opts};
    use listen_core::analysis::{ANALYZER_VERSION, FileId, Outcome};
    use listen_core::model::SourceKind;
    use listen_source::Urgency;
    let config = listen_core::config::Config::load(&paths.config_file()).map_err(|e| e.to_string())?;
    let is_yt = |s: &&listen_core::config::ServerConfig| s.kind == SourceKind::YouTubeMusic;
    let server = config
        .active()
        .filter(is_yt)
        .or_else(|| config.servers.iter().find(is_yt))
        .ok_or("no YouTube Music server configured")?;
    let secret = {
        let (dir, id) = (paths.config_dir.clone(), server.id.clone());
        tokio::task::spawn_blocking(move || crate::secrets::Secrets::new(&dir).get(&id))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no stored sign-in for the YouTube Music server (sign in once in Listen)")?
    };
    let yt = listen_ytmusic::YouTubeMusic::connect(server.id.clone(), &secret)
        .await
        .map_err(|e| format!("sign-in: {e}"))?;
    if !yt.analysable() {
        return Err("this build's YouTube Music source can't hand out audio for analysis yet".into());
    }
    let dir = shared_dir(&config, paths);
    let machine = listen_analysis::store::machine_id();
    let mut store = listen_analysis::store::ShardStore::open(&dir, &machine).map_err(|e| e.to_string())?;
    let songs = match target {
        YtmTarget::Song(id) => {
            // Only the id is known: no listing duration (matched leniently).
            let mut song = check_song(0, &id, "", 0, "");
            song.id = id;
            song.server_id = server.id.clone();
            song.source = SourceKind::YouTubeMusic;
            song.duration = std::time::Duration::ZERO;
            song.year = None;
            song.genres.clear();
            song.artists.clear();
            vec![song]
        }
        YtmTarget::Library(n) => {
            let mut songs = yt.random_songs(n * 3 + 10, None).await.map_err(|e| e.to_string())?;
            if songs.is_empty() {
                return Err(if yt.is_guest() {
                    "the configured YouTube Music server is signed in as a guest (no library); sign in with your browser in Listen".into()
                } else {
                    "the YouTube Music library listing came back empty".into()
                });
            }
            println!("library sample: {} songs", songs.len());
            songs.retain(|song| {
                let key = listen_analysis::record_key(song);
                !store.lookup(&key, &FileId::of(song)).is_some_and(|r| match &r.outcome {
                    Outcome::Ok(_) => r.version >= ANALYZER_VERSION,
                    Outcome::Failed { transient, .. } => !transient && r.version >= ANALYZER_VERSION,
                })
            });
            songs.truncate(n);
            songs
        }
    };
    println!("YouTube Music: {} songs to analyse → {} ({machine})\n", songs.len(), dir.display());
    for song in &songs {
        let t = std::time::Instant::now();
        let source = match yt.analysis_source(song, Urgency::Now).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                println!("  {}: not analysable", song.title);
                continue;
            }
            Err(e) => {
                println!("  {}: ✗ can't get the audio: {e}", song.title);
                continue;
            }
        };
        let fetched = t.elapsed().as_secs_f32();
        let opts = Opts { beats: beats.clone(), bpm_hint: None };
        let result =
            listen_analysis::analyze_url(&source.input.as_mpv_arg(), &opts, &Cancel::default()).await;
        println!(
            "{:>5.1}s ({fetched:.1}s fetch)  {} – {}  [{}]\n        input {}\n        file {:?}",
            t.elapsed().as_secs_f32(),
            song.artist_line(),
            song.title,
            listen_analysis::record_key(song),
            source.input.as_mpv_arg(),
            source.file
        );
        match &result {
            Ok(f) => print_features(f),
            Err(e) => println!("        ✗ {e}"),
        }
        if let Some(record) =
            listen_analysis::record(song, source.file, "music.youtube.com", &machine, result)
        {
            store.append(record).map_err(|e| e.to_string())?;
        }
    }
    store.flush().map_err(|e| e.to_string())?;
    println!("\nstore: {} songs analysed across {:?}", store.len(), store.machines());
    Ok(())
}

async fn run_analysis(args: Vec<String>) -> Result<(), String> {
    use listen_analysis::{BeatModel, BeatSource, Cancel, Opts};
    use listen_core::analysis::FlowFeatures;
    let paths = listen_core::config::Paths::discover().map_err(|e| e.to_string())?;
    let beats = match BeatModel::ensure(&paths.cache_dir.join("models")).await {
        Ok(m) => BeatSource::BeatThis(m),
        Err(e) => {
            println!("beat model unavailable ({e}); using the built-in tracker");
            BeatSource::Own
        }
    };
    if let Some(target) = ytm_target(&args) {
        return run_ytm_analysis(&paths, beats, target).await;
    }
    let arg = args.into_iter().next();
    // One file or URL: everything about it.
    if let Some(input) = arg.as_ref().filter(|a| a.parse::<usize>().is_err()) {
        let t = std::time::Instant::now();
        let f = listen_analysis::analyze_url(input, &Opts { beats, bpm_hint: None }, &Cancel::default())
            .await
            .map_err(|e| e.to_string())?;
        println!("analysed in {:.1}s", t.elapsed().as_secs_f32());
        print_features(&f);
        return Ok(());
    }
    let n: usize = arg.and_then(|a| a.parse().ok()).unwrap_or(20);
    let config = listen_core::config::Config::load(&paths.config_file()).map_err(|e| e.to_string())?;
    let nd = |s: &&listen_core::config::ServerConfig| s.kind == listen_core::model::SourceKind::Subsonic;
    let server = config
        .active()
        .filter(nd)
        .or_else(|| config.servers.iter().find(nd))
        .ok_or("no Navidrome server configured")?;
    // The keyring client blocks (its own runtime): off the async threads.
    let password = {
        let (dir, id) = (paths.config_dir.clone(), server.id.clone());
        tokio::task::spawn_blocking(move || crate::secrets::Secrets::new(&dir).get(&id))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no stored password for the Navidrome server (sign in once in Listen)")?
    };
    let client = listen_subsonic::Subsonic::new(server.id.clone(), &server.url, &server.username, &password)
        .map_err(|e| e.to_string())?;
    let songs = client.random_songs(n, None).await.map_err(|e| e.to_string())?;
    let dir = shared_dir(&config, &paths);
    let machine = listen_analysis::store::machine_id();
    let mut store = listen_analysis::store::ShardStore::open(&dir, &machine).map_err(|e| e.to_string())?;
    println!(
        "{} as {}: {} random songs; results go to {} ({machine})\n",
        server.url,
        server.username,
        songs.len(),
        dir.display()
    );
    let (mut tagged, mut exact, mut octave, mut total_secs) = (0, 0, 0, 0.0);
    for song in &songs {
        let Some(url) = client.analysis_url(song) else { continue };
        let t = std::time::Instant::now();
        let opts = Opts { beats: beats.clone(), bpm_hint: None };
        let result = listen_analysis::analyze_url(&url, &opts, &Cancel::default()).await;
        let secs = t.elapsed().as_secs_f32();
        total_secs += secs;
        let line = match &result {
            Ok(f) => {
                let measured = f.grid.as_ref().map(|g| g.bpm);
                if let (Some(tag), Some(m)) = (song.bpm.filter(|b| *b > 0), measured) {
                    tagged += 1;
                    let ratio = m / tag as f32;
                    if (ratio - 1.0).abs() <= 0.02 {
                        exact += 1;
                    }
                    if [1.0, 2.0, 0.5, 1.5, 2.0 / 3.0].iter().any(|k| (ratio / k - 1.0).abs() <= 0.02) {
                        octave += 1;
                    }
                }
                format!(
                    "{} | tag {} | intro/outro instrumental {}/{} bars | {}",
                    FlowFeatures::of(f).describe(),
                    song.bpm.map_or("-".into(), |b| b.to_string()),
                    f.intro_instrumental_bars,
                    f.outro_instrumental_bars,
                    f.sections
                        .iter()
                        .map(|s| format!("{:?} {}", s.kind, s.bars))
                        .collect::<Vec<_>>()
                        .join(" · ")
                )
            }
            Err(e) => format!("✗ {e}"),
        };
        println!("{:>5.1}s  {} – {}\n        {line}", secs, song.artist_line(), song.title);
        let host = server.url.split('/').nth(2).unwrap_or("").to_string();
        if let Some(record) =
            listen_analysis::record(song, listen_core::analysis::FileId::of(song), &host, &machine, result)
        {
            store.append(record).map_err(|e| e.to_string())?;
        }
    }
    store.flush().map_err(|e| e.to_string())?;
    println!("\naverage {:.1}s per song (download + analysis)", total_secs / songs.len().max(1) as f32);
    if tagged > 0 {
        println!(
            "tempo vs BPM tags: {exact}/{tagged} exact (±2 %), {octave}/{tagged} allowing half/double time"
        );
    } else {
        println!("tempo vs BPM tags: no song had a BPM tag");
    }
    println!("store: {} songs analysed across {:?}", store.len(), store.machines());
    Ok(())
}

fn print_features(f: &listen_core::analysis::Features) {
    use listen_core::analysis::FlowFeatures;
    println!("{}", FlowFeatures::of(f).describe());
    println!("audio {:.2}s – {:.2}s, {:.1} LUFS, LRA {:.1}", f.audio_start, f.audio_end, f.lufs, f.lra);
    match &f.grid {
        Some(g) => println!(
            "grid: {:.2} bpm, confidence {:.2}, stability {:.2}, {} beats, {} bars, first downbeat {:.3}s",
            g.bpm,
            g.confidence,
            g.stability,
            g.beats.len(),
            g.bar_count(),
            g.bar_time(0)
        ),
        None => println!("grid: none (no steady beat → plain fades)"),
    }
    if let Some((k, c)) = f.key {
        println!("key: {} (confidence {c:.2})", k.label());
    }
    println!(
        "phrases of {} bars from bar {}; instrumental intro {} bars, outro {} bars",
        f.phrase_bars, f.phrase_offset, f.intro_instrumental_bars, f.outro_instrumental_bars
    );
    for s in &f.sections {
        let at = f.grid.as_ref().map_or(f32::NAN, |g| g.bar_time(s.start_bar as usize));
        println!("  bar {:>3} ({:>6.1}s): {:?}, {} bars", s.start_bar, at, s.kind, s.bars);
    }
    println!("mood: {:?}", f.mood);
}

/// `listen --analyze-library [--jobs N]`: analyses every not-yet-analysed Navidrome song, `N` at a time
/// (default 3), into the shared analysis folder. Writes under its own shard name so a running Listen
/// can't collide with it.
pub fn analyze_library(jobs: usize) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(run_analyze_library(jobs.clamp(1, 8))).map_or_else(
        |e| {
            eprintln!("✗ {e}");
            1
        },
        |()| 0,
    )
}

async fn run_analyze_library(jobs: usize) -> Result<(), String> {
    use listen_analysis::{BeatModel, BeatSource, Cancel, Opts};
    use listen_core::analysis::{ANALYZER_VERSION, FileId, FlowFeatures, Outcome};
    let paths = listen_core::config::Paths::discover().map_err(|e| e.to_string())?;
    let config = listen_core::config::Config::load(&paths.config_file()).map_err(|e| e.to_string())?;
    let nd = |s: &&listen_core::config::ServerConfig| s.kind == listen_core::model::SourceKind::Subsonic;
    let server = config
        .active()
        .filter(nd)
        .or_else(|| config.servers.iter().find(nd))
        .ok_or("no Navidrome server configured")?;
    let password = {
        let (dir, id) = (paths.config_dir.clone(), server.id.clone());
        tokio::task::spawn_blocking(move || crate::secrets::Secrets::new(&dir).get(&id))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no stored password for the Navidrome server (sign in once in Listen)")?
    };
    let client = std::sync::Arc::new(
        listen_subsonic::Subsonic::new(server.id.clone(), &server.url, &server.username, &password)
            .map_err(|e| e.to_string())?,
    );
    let beats = match BeatModel::ensure(&paths.cache_dir.join("models")).await {
        Ok(m) => BeatSource::BeatThis(m),
        Err(e) => {
            println!("beat model unavailable ({e}); using the built-in tracker");
            BeatSource::Own
        }
    };
    let dir = shared_dir(&config, &paths);
    let machine = format!("{}-library", listen_analysis::store::machine_id());
    let mut store = listen_analysis::store::ShardStore::open(&dir, &machine).map_err(|e| e.to_string())?;

    // Everything on the server, minus what is analysed already.
    let mut songs = Vec::new();
    let mut start = 0;
    loop {
        let q = listen_source::SongQuery { start, limit: 500, ..Default::default() };
        let page = client.songs(&q).await.map_err(|e| e.to_string())?.items;
        let n = page.len();
        songs.extend(page);
        start += n;
        if n < 500 {
            break;
        }
    }
    let total = songs.len();
    songs.retain(|song| {
        !store.lookup(&song.id, &FileId::of(song)).is_some_and(|r| match &r.outcome {
            Outcome::Ok(_) => r.version >= ANALYZER_VERSION,
            Outcome::Failed { transient, .. } => !transient && r.version >= ANALYZER_VERSION,
        })
    });
    println!(
        "{} as {}: {total} songs, {} to analyse ({jobs} at a time) → {} ({machine})",
        server.url,
        server.username,
        songs.len(),
        dir.display()
    );
    let host = server.url.split('/').nth(2).unwrap_or("").to_string();
    let started = std::time::Instant::now();
    let todo = songs.len();
    let mut queue = songs.into_iter();
    let mut running = tokio::task::JoinSet::new();
    let (mut done, mut failed) = (0usize, 0usize);
    loop {
        while running.len() < jobs
            && let Some(song) = queue.next()
        {
            let Some(url) = client.analysis_url(&song) else { continue };
            let opts = Opts { beats: beats.clone(), bpm_hint: song.bpm.map(|b| b as f32) };
            running.spawn(async move {
                let result = listen_analysis::analyze_url(&url, &opts, &Cancel::default()).await;
                (song, result)
            });
        }
        let Some(joined) = running.join_next().await else { break };
        let Ok((song, result)) = joined else { continue };
        done += 1;
        let line = match &result {
            Ok(f) => FlowFeatures::of(f).describe(),
            Err(e) => {
                failed += 1;
                format!("✗ {e}")
            }
        };
        let per = started.elapsed().as_secs_f64() / done as f64;
        let left = (todo - done) as f64 * per;
        println!(
            "[{done}/{todo}] {} – {}: {line}  (ETA {}h{:02}m)",
            song.artist_line(),
            song.title,
            (left / 3600.0) as u64,
            ((left % 3600.0) / 60.0) as u64
        );
        if let Some(record) = listen_analysis::record(&song, FileId::of(&song), &host, &machine, result) {
            store.append(record).map_err(|e| e.to_string())?;
        }
    }
    store.flush().map_err(|e| e.to_string())?;
    println!(
        "done: {done} analysed ({failed} failed) in {:.0} min; the store now knows {} songs",
        started.elapsed().as_secs_f64() / 60.0,
        store.len()
    );
    Ok(())
}

/// `listen --song-card <TITLE> <ARTIST> [--album A] [--year Y] [--lang de|en]`: builds the About
/// card (Gemini with Google Search + MusicBrainz credits), prints it as JSON with a summary line,
/// and fails when a fact has no source. Costs up to three Gemini requests; nothing is cached.
pub fn song_card(args: &[String]) -> i32 {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(title), Some(artist)) = (args.first().filter(|a| !a.starts_with("--")), args.get(1)) else {
        eprintln!("usage: listen --song-card <TITLE> <ARTIST> [--album A] [--year Y] [--lang de|en]");
        return 2;
    };
    let key = std::env::var("LISTEN_GEMINI_API_KEY").ok().filter(|k| !k.trim().is_empty()).or_else(|| {
        let paths = listen_core::config::Paths::discover().ok()?;
        crate::secrets::Secrets::new(&paths.config_dir).get(&ServerId("gemini-api-key".into()))
    });
    let Some(key) = key else {
        eprintln!("no key: set LISTEN_GEMINI_API_KEY or save one in Settings → AI DJ");
        return 1;
    };
    let model = listen_core::config::Paths::discover()
        .ok()
        .and_then(|p| listen_core::config::Config::load(&p.config_file()).ok())
        .map(|c| c.settings.ai_dj)
        .unwrap_or_default()
        .text_model;
    let language = if opt("--lang").as_deref() == Some("de") { "German" } else { "English" };
    let info = listen_lyrics::card::SongInfo {
        title: title.clone(),
        artists: vec![artist.clone()],
        album: opt("--album"),
        year: opt("--year").and_then(|y| y.parse().ok()),
        recording_mbid: None,
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let http = reqwest::Client::new();
    let now = listen_core::history::now_secs();
    match rt.block_on(listen_lyrics::card::card(&http, &key, &model, language, &info, now)) {
        Ok(card) => {
            println!("{}", serde_json::to_string_pretty(&card).unwrap_or_default());
            let unsourced = card.facts.iter().filter(|f| f.sources.is_empty()).count();
            println!(
                "facts {} (unsourced {unsourced}), credits {}, shown: {}",
                card.facts.len(),
                card.credits.credits.len(),
                !card.is_empty()
            );
            i32::from(unsourced > 0)
        }
        Err(e) => {
            eprintln!("song card failed: {e}");
            1
        }
    }
}

/// `listen --weekly-stats [--history PATH] [--now UNIX]`: prints the Statistics page's
/// "Last 4 weeks" numbers as JSON (for recounting against `history.jsonl`).
pub fn weekly_stats(args: &[String]) -> i32 {
    use listen_core::history;
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let path = match opt("--history") {
        Some(p) => std::path::PathBuf::from(p),
        None => match listen_core::config::Paths::discover() {
            Ok(p) => p.data_dir.join(history::FILE_NAME),
            Err(e) => {
                eprintln!("no data folder: {e}");
                return 1;
            }
        },
    };
    let now = opt("--now").and_then(|n| n.parse().ok()).unwrap_or_else(history::now_secs);
    let h = history::History::load(&path);
    let w = listen_core::weekly::weekly(&h.entries, now, history::local_offset(), 10);
    println!("{}", serde_json::to_string_pretty(&weekly_json(&w, now)).unwrap_or_default());
    0
}

/// The rolling stats as JSON (shared by the CLI check and its test).
pub fn weekly_json(w: &listen_core::weekly::Weekly, now: i64) -> serde_json::Value {
    use serde_json::json;
    let totals = |t: &listen_core::weekly::Totals| {
        json!({"plays": t.plays, "listening_secs": t.listening_secs, "skips": t.skips,
               "songs": t.songs, "artists": t.artists})
    };
    let ranked = |v: &[listen_core::history::Ranked]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|r| json!({"name": r.name, "detail": r.detail, "plays": r.plays, "secs": r.secs, "skips": r.skips}))
            .collect()
    };
    let f = &w.four_weeks;
    json!({
        "now": now,
        "this_week": totals(&w.this_week),
        "last_week": totals(&w.last_week),
        "four_weeks": {"plays": f.plays, "listening_secs": f.listening_secs, "skips": f.skips,
                       "songs": ranked(&f.songs), "artists": ranked(&f.artists), "albums": ranked(&f.albums)},
        "new_artists": ranked(&w.new_artists),
        "new_artists_total": w.new_artists_total,
        "most_skipped": ranked(&w.most_skipped),
    })
}

/// `--memory [--history FILE] [--now SECS] [--limit N]`: the memory model on a history file as JSON —
/// the top forgotten favourites (Auto-DJ slot, AI DJ pool, Rediscover) and the worn-out songs rested
/// from automatic picks now.
pub fn memory(args: &[String]) -> i32 {
    use listen_core::{history, memory};
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let path = match opt("--history") {
        Some(p) => std::path::PathBuf::from(p),
        None => match listen_core::config::Paths::discover() {
            Ok(p) => p.data_dir.join(history::FILE_NAME),
            Err(e) => {
                eprintln!("no data folder: {e}");
                return 1;
            }
        },
    };
    let now = opt("--now").and_then(|n| n.parse().ok()).unwrap_or_else(history::now_secs);
    let limit = opt("--limit").and_then(|n| n.parse().ok()).unwrap_or(20);
    let h = history::History::load(&path);
    let m = memory::Memory::new(&h.entries, now);
    println!("{}", serde_json::to_string_pretty(&memory_json(&m, limit)).unwrap_or_default());
    0
}

/// `listen --focus [--history PATH] [--store DIR] [--fills N] [--per-fill N] [--pool N] [--seed N]`: focus mode
/// on the user's own data. The history's analysed songs are shuffled (fixed seed) into Auto-DJ-sized
/// candidate pools; each fill takes `per-fill` songs as an ordinary fill would (in pool order) and as
/// focus mode does ([`listen_mix::pick::focus_pool`]). Prints the mean sung-bar share of both and the
/// ratio as JSON (the acceptance: focus ≤ 50 % of normal).
/// `listen --flow-weights [--history FILE] [--store DIR] [--simulate N [--seed S]]`: the flow weights item 16 learns from this
/// history (labelled handovers, skips, cross-validated AUC against the default weights' AUC, the weights).
pub fn flow_weights(args: &[String]) -> i32 {
    use listen_core::analysis::FlowFeatures;
    use listen_core::history;
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let paths = match listen_core::config::Paths::discover() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no data folder: {e}");
            return 1;
        }
    };
    let config = listen_core::config::Config::load(&paths.config_file()).unwrap_or_default();
    let hist = opt("--history")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| paths.data_dir.join(history::FILE_NAME));
    let dir = opt("--store").map(std::path::PathBuf::from).unwrap_or_else(|| shared_dir(&config, &paths));
    let store = match listen_analysis::store::ShardStore::open(&dir, &listen_analysis::store::machine_id()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("analysis store {}: {e}", dir.display());
            return 1;
        }
    };
    let mut h = history::History::load(&hist);
    let flow = |song: &listen_core::model::Song| {
        store
            .lookup(&listen_analysis::record_key(song), &listen_core::analysis::FileId::of(song))
            .and_then(|r| r.features().map(FlowFeatures::of))
    };
    if let Some(n) = opt("--simulate").and_then(|n| n.parse::<usize>().ok()) {
        h.entries = simulated_handovers(
            &h.entries,
            &flow,
            n,
            opt("--seed").and_then(|s| s.parse().ok()).unwrap_or(16),
        );
        println!(
            "simulated: {n} Auto-DJ handovers over the history's analysed songs; early skips follow key clashes and tempo gaps"
        );
    }
    let data = listen_mix::learn::transitions(&h.entries, flow);
    let l = listen_mix::learn::learn(&data, history::now_secs());
    let auc = |a: Option<f32>| a.map_or("-".to_string(), |a| format!("{a:.3}"));
    println!("history entries      {}", h.entries.len());
    println!("labelled handovers   {} ({} skipped within 30 s)", l.labels, l.skips);
    println!("trust                {:.2} (full at {})", l.trust, listen_mix::learn::FULL_TRUST);
    println!("AUC 5-fold (learnt)  {}", auc(l.auc_cv));
    println!("AUC default weights  {}", auc(l.auc_default));
    let d = listen_mix::learn::TermWeights::of(&listen_mix::Weights::default());
    let row = |name: &str, def: f32, fit: Option<f32>, used: f32| {
        println!(
            "  {name:<14} default {def:>5.2}  fitted {:>6}  used {used:>5.2}",
            fit.map_or("-".into(), |f| format!("{f:.2}"))
        );
    };
    let f = l.fitted;
    let w = l.weights;
    row("tempo", d.tempo, f.map(|f| f.tempo), w.tempo);
    row("key", d.key, f.map(|f| f.key), w.key);
    row("energy", d.energy, f.map(|f| f.energy), w.energy);
    row("timbre", d.timbre, f.map(|f| f.timbre), w.timbre);
    row("mixability", d.mixability, f.map(|f| f.mixability), w.mixability);
    row("artist_repeat", d.artist_repeat, f.map(|f| f.artist_repeat), w.artist_repeat);
    row("album_repeat", d.album_repeat, f.map(|f| f.album_repeat), w.album_repeat);
    0
}

/// A made-up Auto-DJ history over the analysed songs of `entries`: `n` handovers on one device, the next
/// song skipped 10 s in with a probability that rises with the key clash and the tempo gap (as a listener
/// who minds those), else played. Shows that the fit finds such a listener.
fn simulated_handovers(
    entries: &[listen_core::history::Entry],
    flow: &dyn Fn(&listen_core::model::Song) -> Option<listen_core::analysis::FlowFeatures>,
    n: usize,
    seed: u64,
) -> Vec<listen_core::history::Entry> {
    use listen_core::history::{Entry, Picker, PlayCtx, PlayMode};
    use rand::{Rng, SeedableRng};
    let mut seen = std::collections::HashSet::new();
    let songs: Vec<(listen_core::model::Song, listen_core::analysis::FlowFeatures)> = entries
        .iter()
        .map(Entry::to_song)
        .filter(|s| seen.insert((s.server_id.0.clone(), s.id.clone())))
        .filter_map(|s| flow(&s).map(|f| (s, f)))
        .collect();
    if songs.len() < 2 {
        return Vec::new();
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let ctx = |picker| PlayCtx { mode: PlayMode::AutoDj, picker, device: "sim".into() };
    let mut ts = 1_700_000_000i64;
    let mut prev = rng.random_range(0..songs.len());
    let mut out =
        vec![Entry { ctx: Some(ctx(Picker::User)), at_s: Some(100.0), ..Entry::new(&songs[prev].0, ts) }];
    for _ in 0..n {
        let next = rng.random_range(0..songs.len());
        let t = listen_mix::score::flow_terms(&songs[prev].1, &songs[next].1, &[], None);
        let logit = -3.5 + 1.6 * t[1] + 0.9 * t[0];
        let skip = rng.random::<f32>() < 1.0 / (1.0 + (-logit).exp());
        let left = songs[prev].0.duration.as_secs() as i64 - 100;
        if skip {
            ts += left.max(0) + 10;
            out.push(Entry::skipped(&songs[next].0, ts, 10.0, ctx(Picker::AutoDj)));
            // The song after a skip starts fresh (not a handover).
            prev = rng.random_range(0..songs.len());
            ts += 3600;
            out.push(Entry {
                ctx: Some(ctx(Picker::User)),
                at_s: Some(100.0),
                ..Entry::new(&songs[prev].0, ts)
            });
        } else {
            ts += left.max(0) + 100;
            out.push(Entry {
                ctx: Some(ctx(Picker::AutoDj)),
                at_s: Some(100.0),
                ..Entry::new(&songs[next].0, ts)
            });
            prev = next;
        }
    }
    out
}

pub fn focus(args: &[String]) -> i32 {
    use listen_core::analysis::FlowFeatures;
    use listen_core::history;
    use listen_core::model::Song;
    use rand::SeedableRng;
    use rand::seq::SliceRandom;
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let num = |name: &str, d: usize| opt(name).and_then(|n| n.parse().ok()).unwrap_or(d);
    let (fills, per_fill, pool_n) = (num("--fills", 8), num("--per-fill", 5), num("--pool", 30));
    let paths = match listen_core::config::Paths::discover() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no data folder: {e}");
            return 1;
        }
    };
    let config = listen_core::config::Config::load(&paths.config_file()).unwrap_or_default();
    let hist = opt("--history")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| paths.data_dir.join(history::FILE_NAME));
    let dir = opt("--store").map(std::path::PathBuf::from).unwrap_or_else(|| shared_dir(&config, &paths));
    let store = match listen_analysis::store::ShardStore::open(&dir, &listen_analysis::store::machine_id()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("analysis store {}: {e}", dir.display());
            return 1;
        }
    };
    let h = history::History::load(&hist);
    let mut seen = std::collections::HashSet::new();
    let mut pool: Vec<(Song, Option<FlowFeatures>)> = Vec::new();
    for e in h.entries.iter().rev() {
        let song = e.to_song();
        if !seen.insert(listen_core::versions::song_key(&song)) {
            continue;
        }
        let file = listen_core::analysis::FileId::of(&song);
        let f = store
            .lookup(&listen_analysis::record_key(&song), &file)
            .and_then(|r| r.features().map(FlowFeatures::of))
            .filter(|f| f.vocal.is_some());
        if let Some(f) = f {
            pool.push((song, Some(f)));
        }
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(num("--seed", 13) as u64);
    pool.shuffle(&mut rng);
    let vocal = |f: &Option<FlowFeatures>| f.as_ref().and_then(|f| f.vocal).unwrap_or(0.0);
    // As the app: each fill's candidates are a fetched pool plus the 30 least sung songs from the history;
    // nothing comes twice.
    let (mut normal, mut focused) = (Vec::new(), Vec::new());
    let mut used = std::collections::HashSet::new();
    for chunk in pool.chunks(pool_n).take(fills) {
        normal.extend(chunk.iter().take(per_fill).map(|c| (c.0.clone(), vocal(&c.1))));
        let mut cands: Vec<(Song, Option<FlowFeatures>)> =
            chunk.iter().filter(|c| !used.contains(&c.0.id)).cloned().collect();
        let rest: Vec<(Song, Option<FlowFeatures>)> = pool
            .iter()
            .filter(|c| !used.contains(&c.0.id) && !cands.iter().any(|x| x.0.id == c.0.id))
            .cloned()
            .collect();
        let extra = listen_mix::pick::focus_extras(&rest, 30);
        cands.extend(extra.into_iter().map(|i| rest[i].clone()));
        let order = listen_mix::pick::focus_pool(&cands, per_fill);
        for i in order.into_iter().take(per_fill) {
            used.insert(cands[i].0.id.clone());
            focused.push((cands[i].0.clone(), vocal(&cands[i].1)));
        }
    }
    let mean = |v: &[(Song, f32)]| {
        if v.is_empty() { 0.0 } else { v.iter().map(|x| x.1).sum::<f32>() / v.len() as f32 }
    };
    let (mn, mf) = (mean(&normal), mean(&focused));
    let rows = |v: &[(Song, f32)]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|(s, x)| serde_json::json!({"title": s.title, "artist": s.artist_line(), "vocal": (f64::from(*x) * 100.0).round() / 100.0}))
            .collect()
    };
    let out = serde_json::json!({
        "analysed_history_songs": pool.len(),
        "songs": {"normal": normal.len(), "focus": focused.len()},
        "mean_vocal": {"normal": (f64::from(mn) * 1000.0).round() / 1000.0, "focus": (f64::from(mf) * 1000.0).round() / 1000.0},
        "ratio": if mn > 0.0 { (f64::from(mf / mn) * 1000.0).round() / 1000.0 } else { 0.0 },
        "focus_songs": rows(&focused),
        "normal_songs": rows(&normal),
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    0
}

/// [`memory`]'s JSON.
pub fn memory_json(m: &listen_core::memory::Memory, limit: usize) -> serde_json::Value {
    use serde_json::json;
    let row = |s: &listen_core::memory::SongMemory| {
        json!({"title": s.song.title, "artist": s.song.artist_line(), "plays": s.plays,
               "last_played": s.last_played, "days_since": (m.now - s.last_played) / 86_400,
               "activation": (s.activation * 100.0).round() / 100.0, "peak": (s.peak * 100.0).round() / 100.0,
               "recent_skips": s.recent_skips, "score": (s.forgotten_score() * 100.0).round() / 100.0})
    };
    let mut worn: Vec<&listen_core::memory::SongMemory> =
        m.songs.values().filter(|s| s.is_worn_out()).collect();
    worn.sort_by(|a, b| b.recent_skips.cmp(&a.recent_skips).then(a.key.cmp(&b.key)));
    json!({
        "now": m.now,
        "songs": m.songs.len(),
        "forgotten": m.forgotten(limit).into_iter().map(row).collect::<Vec<_>>(),
        "worn_out": worn.into_iter().map(row).collect::<Vec<_>>(),
    })
}

/// `listen --known-words <TITLE> <ARTIST> [--lrc FILE] [--hub URL]`: the song's Korean words with
/// their Lute status (vocab-hub) and the share of known words, as JSON — what the lyrics' words
/// mode colours and the Learnable songs mix counts.
pub fn known_words(args: &[String]) -> i32 {
    use listen_lyrics::known;
    use listen_lyrics::vocab::Knowledge;
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let lrc = opt("--lrc");
    let (title, artist) =
        (args.first().filter(|a| !a.starts_with("--")), args.get(1).filter(|a| !a.starts_with("--")));
    if lrc.is_none() && (title.is_none() || artist.is_none()) {
        eprintln!("usage: listen --known-words <TITLE> <ARTIST> [--lrc FILE] [--hub URL]");
        return 2;
    }
    let hub = opt("--hub").or_else(|| {
        let p = listen_core::config::Paths::discover().ok()?;
        let c = listen_core::config::Config::load(&p.config_file()).ok()?;
        Some(c.settings.lyrics.vocab_hub_url).filter(|u| !u.trim().is_empty())
    });
    let Some(hub) = hub else {
        eprintln!("no vocab-hub: pass --hub URL or set it in Settings → Lyrics");
        return 1;
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let http = reqwest::Client::new();
    let lines: Vec<String> = match &lrc {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(t) => listen_lyrics::parse_lrc(&t).lines.into_iter().map(|l| l.text).collect(),
            Err(e) => {
                eprintln!("{path}: {e}");
                return 1;
            }
        },
        None => {
            let q = listen_lyrics::providers::Query {
                name: title.cloned().unwrap_or_default(),
                artist: artist.cloned().unwrap_or_default(),
                album: String::new(),
                duration_secs: 0.0,
            };
            let opts = listen_lyrics::fetch::Options::default();
            let sources = [listen_lyrics::providers::Source::Lrclib];
            let found = rt.block_on(async {
                for hit in listen_lyrics::fetch::search_all(&http, &q, &sources).await {
                    if let Some(l) = listen_lyrics::fetch::get(&http, &hit, &opts).await {
                        return Some(l);
                    }
                }
                None
            });
            match found {
                Some(l) => l.lines.into_iter().map(|l| l.text).collect(),
                None => {
                    eprintln!("no lyrics found");
                    return 1;
                }
            }
        }
    };
    let words = match rt.block_on(listen_lyrics::vocab::words(&http, &hub)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("vocab-hub: {e}");
            return 1;
        }
    };
    let lemmas = rt.block_on(known::hub_lemmas(&http, &hub, &lines)).ok().flatten();
    let resolved = lemmas.is_some();
    let tokens = known::tokens(&lines, &lemmas.unwrap_or_default());
    let local = known::coverage(&words, &tokens, 5);
    let mut store = known::Store::default();
    store.record("song", &lines, &std::collections::HashMap::new(), 0);
    let text = store.songs.get("song").map(|s| s.text()).unwrap_or_default();
    let hub_cov = rt.block_on(known::hub_coverage(&http, &hub, &text, 5));
    let name = |k: Knowledge| match k {
        Knowledge::Known => "known",
        Knowledge::Learning(_) => "learning",
        Knowledge::New => "new",
        Knowledge::Ignored => "ignored",
    };
    let list: Vec<serde_json::Value> = tokens
        .iter()
        .map(|t| {
            let status = known::status_of(&words, &t.form, t.lemma.as_deref());
            // As the lyrics show it: words the list does not have are classed by the hub.
            let shown = match status {
                Some(_) => Knowledge::from_status(status, 5),
                None => rt
                    .block_on(known::hub_coverage(&http, &hub, &t.form, 5))
                    .map(|c| known::class_of(&c))
                    .unwrap_or(Knowledge::New),
            };
            serde_json::json!({ "word": t.form, "lemma": t.lemma, "count": t.count, "status": status,
                "shown": name(shown) })
        })
        .collect();
    let cov = hub_cov.as_ref().copied().unwrap_or(local);
    let out = serde_json::json!({
        "lines": lines.len(),
        "hub_resolve": resolved,
        "word_list_version": words.version,
        "coverage": cov,
        "counted_by": if hub_cov.is_ok() { "hub" } else { "word list" },
        "percent": cov.percent(),
        "word_list_percent": local.percent(),
        "learnable": cov.learnable(),
        "words": list,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_music_targets() {
        let t = |a: &[&str]| ytm_target(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(t(&["ytm:lYBUbBu4W08"]), Some(YtmTarget::Song("lYBUbBu4W08".into())));
        assert_eq!(
            t(&["https://music.youtube.com/watch?v=lYBUbBu4W08&list=RDAMVM"]),
            Some(YtmTarget::Song("lYBUbBu4W08".into()))
        );
        assert_eq!(t(&["https://youtu.be/lYBUbBu4W08?si=x"]), Some(YtmTarget::Song("lYBUbBu4W08".into())));
        assert_eq!(t(&["ytm"]), Some(YtmTarget::Library(10)));
        assert_eq!(t(&["ytm", "3"]), Some(YtmTarget::Library(3)));
        assert_eq!(t(&["20"]), None);
        assert_eq!(t(&["/music/a.flac"]), None);
        assert_eq!(t(&["https://nd.example/rest/stream?id=x"]), None);
    }
}
