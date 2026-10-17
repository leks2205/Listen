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

/// `listen --check-ai-dj [--no-voice]`: two AI DJ rounds against Gemini on a fixed song pool; prints picks,
/// lines, timings and tokens, saves the spoken breaks as WAVs in the temp dir. Key: `$LISTEN_GEMINI_API_KEY`
/// or the app's keyring entry. Costs 2 text + 2 speech requests (no speech with `--no-voice`).
pub fn ai_dj(voice: bool) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(run_ai_dj(voice)).map_or_else(
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

async fn run_ai_dj(voice: bool) -> Result<(), String> {
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
    let songs = [
        ("Teardrop", "Massive Attack", 1998, "Trip-Hop"),
        ("Roads", "Portishead", 1994, "Trip-Hop"),
        ("Angel", "Massive Attack", 1998, "Trip-Hop"),
        ("Hyperballad", "Björk", 1995, "Electronic"),
        ("Cherry Blossom Girl", "Air", 2004, "Downtempo"),
        ("Intro", "The xx", 2009, "Indie"),
        ("Breathe", "Télépopmusik", 2001, "Downtempo"),
        ("Midnight City", "M83", 2011, "Synthpop"),
        ("Mr. Brightside", "The Killers", 2004, "Rock"),
        ("Strobe", "deadmau5", 2009, "Progressive House"),
        ("Holocene", "Bon Iver", 2011, "Indie Folk"),
        ("Nightcall", "Kavinsky", 2010, "Synthwave"),
        ("Kids", "MGMT", 2007, "Indie"),
        ("Sour Times", "Portishead", 1994, "Trip-Hop"),
        ("Sweet Disposition", "The Temper Trap", 2008, "Indie Rock"),
        ("Bloodstream", "Stateless", 2007, "Trip-Hop"),
        ("Porcelain", "Moby", 1999, "Downtempo"),
        ("Smells Like Teen Spirit", "Nirvana", 1991, "Grunge"),
        ("Unfinished Sympathy", "Massive Attack", 1991, "Trip-Hop"),
        ("Glory Box", "Portishead", 1994, "Trip-Hop"),
    ];
    let all: Vec<_> = songs.iter().enumerate().map(|(n, s)| check_song(n, s.0, s.1, s.2, s.3)).collect();
    let seed = all[0].clone();
    println!(
        "persona: {} (voice {}), text {}, speech {}",
        persona.name, persona.voice, settings.text_model, settings.tts_model
    );
    println!("seed: {} – {}", seed.title, seed.artist_line());

    let mut played: Vec<listen_dj::Played> = Vec::new();
    let mut current = seed.clone();
    let (mut mood, mut last_line) = (None::<String>, None::<String>);
    let mut seed_mood = None::<String>;
    let mut used: std::collections::HashSet<String> = [seed.id.clone()].into();
    let notes = std::collections::HashMap::new();
    for round in 1..=2 {
        let pool: Vec<_> = all.iter().filter(|s| !used.contains(&s.id)).cloned().collect();
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
            set_min: settings.set_min as usize,
            set_max: settings.set_max as usize,
            first: round == 1,
            mode: listen_dj::BreakMode::ALL[round % 4],
            // Two-host shows: the co-host joins in the second round.
            duo: round == 2,
            show: listen_dj::Show {
                language: crate::i18n::english_name(crate::i18n::dj_language(&settings.language)),
                location: &settings.location,
                notes: &settings.notes,
            },
        };
        let t = std::time::Instant::now();
        let (plan, usage) =
            gemini.plan(&settings.text_model, &req).await.map_err(|e| format!("plan: {e}"))?;
        println!("\n— round {round}: plan in {:.1}s ({usage})", t.elapsed().as_secs_f32());
        println!("break: {}", req.mode.name());
        println!("mood: {}", plan.mood);
        for &i in &plan.picks {
            println!("  ♪ {} – {}", pool[i].title, pool[i].artist_line());
        }
        for t in &plan.turns {
            println!("{}: {}  [{}]", if t.speaker.is_empty() { "line" } else { &t.speaker }, t.text, t.style);
        }
        if voice {
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
        // The set "plays"; the last song of it is playing when the next plan is made.
        let picks: Vec<_> = plan.picks.iter().map(|&i| pool[i].clone()).collect();
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
    println!("\n✓ AI DJ works");
    Ok(())
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
