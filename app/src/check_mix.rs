//! `listen --check-mix <a> <b> [--render out.wav] [--lyrics-a a.lrc] [--lyrics-b b.lrc] [--levels out.jsonl]`: plans the DJ
//! transition from `a` into `b` as the player would and optionally renders an offline preview
//! (`listen_mix::render`). A song is a Navidrome song id (first Navidrome server), `ytm:<videoId>` (via
//! yt-dlp) or a local file / URL; both are analysed afresh. Synced lyrics given as LRC files are merged
//! into the analysis like the player does with fetched ones (`listen_mix::lyric`). `--levels` writes each song's
//! modelled local loudness (`listen_mix::loudness::LocalLevel`) next to its measured 3 s loudness, every 0.5 s.

use listen_analysis::{BeatModel, BeatSource, Cancel, Opts};
use listen_mix::plan::{Kind, PlanCtx};
use listen_source::MusicSource;

pub fn run(args: &[String]) -> i32 {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        eprintln!(
            "usage: listen --check-mix <a> <b> [--render out.wav] [--lyrics-a a.lrc] [--lyrics-b b.lrc] [--levels out.jsonl]  (Navidrome id, ytm:<videoId>, file or URL)"
        );
        return 2;
    };
    let flag = |name: &str| args.iter().position(|x| x == name).and_then(|i| args.get(i + 1)).cloned();
    let render = flag("--render");
    let lyrics = [flag("--lyrics-a"), flag("--lyrics-b")];
    let levels = flag("--levels");
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(check(a, b, render.as_deref(), &lyrics, levels.as_deref())).map_or_else(
        |e| {
            eprintln!("✗ {e}");
            1
        },
        |()| 0,
    )
}

async fn check(
    a: &str,
    b: &str,
    render: Option<&str>,
    lyrics: &[Option<String>; 2],
    levels: Option<&str>,
) -> Result<(), String> {
    let paths = listen_core::config::Paths::discover().map_err(|e| e.to_string())?;
    let config = listen_core::config::Config::load(&paths.config_file()).map_err(|e| e.to_string())?;
    let beats = match BeatModel::ensure(&paths.cache_dir.join("models")).await {
        Ok(m) => BeatSource::BeatThis(m),
        Err(e) => {
            println!("beat model unavailable ({e}); using the built-in tracker");
            BeatSource::Own
        }
    };
    let opts = Opts { beats, bpm_hint: None };
    let mut sides = Vec::new();
    for (arg, lrc) in [a, b].into_iter().zip(lyrics) {
        let input = input_for(arg, &paths, &config).await?;
        let pcm =
            listen_analysis::decode(&input, &Cancel::default()).await.map_err(|e| format!("{arg}: {e}"))?;
        let f = listen_analysis::analyze_in_background(pcm.clone(), opts.clone(), &Cancel::default())
            .await
            .ok_or("analysis cancelled")?;
        println!("{arg}: {:.1}s, {}", pcm.duration(), listen_core::analysis::FlowFeatures::of(&f).describe());
        let f = match lrc {
            Some(path) => with_lrc(&f, path)?,
            None => f,
        };
        sides.push((pcm, f));
    }
    if let Some(path) = levels {
        let rows: Vec<String> =
            [a, b].iter().zip(&sides).filter_map(|(arg, (pcm, f))| level_row(arg, pcm, f)).collect();
        std::fs::write(path, rows.join("\n") + "\n").map_err(|e| format!("{path}: {e}"))?;
    }
    let ((out_pcm, fa), (in_pcm, fb)) = (&sides[0], &sides[1]);
    // No song metadata here: genres are unknown, so Smart acts like Instrumental only.
    let ctx = PlanCtx {
        settings: &config.settings.mixing,
        out_position_s: 0.0,
        voice_break: None,
        out_genres: &[],
        in_genres: &[],
        no_echo: false,
    };
    // As the player plans: render-and-verify on the analysis curves, with the levelling gains.
    let p = &config.settings.playback;
    let gain = |f| listen_mix::track_gain(Some(f), None, None, p.levelling, true).db;
    let gains = (gain(fa), gain(fb));
    let verified = listen_mix::verify::plan_verified(fa, fb, &ctx, gains);
    for r in &verified.rejected {
        println!("rejected {r}");
    }
    let plan = verified.plan;
    let c = &verified.check;
    println!(
        "check: hole {} LU, bump {} LU, vocals clash {:.1} s, cuts a sung line: {}{}",
        c.dip_lu.map_or("-".into(), |d| format!("{d:.2}")),
        c.bump_lu.map_or("-".into(), |b| format!("{b:.2}")),
        c.clash_s,
        c.cuts_line,
        if verified.none_passed { " (no option passed; least bad)" } else { "" }
    );
    println!(
        "\n{:?}: out {:.2}s → in {:.2}s (loaded at {:.2}s), blend {:.1}s, in speed {}, handover {:.2}s, out stops {:.2}s, score {:.2}\n{}",
        plan.kind,
        plan.out_mix_start_s,
        plan.in_mix_start_s,
        plan.in_load_at_s,
        plan.blend_s,
        match plan.in_glide {
            Some(g) => format!("{:.4} → {:.4} (glide)", g.from, g.to),
            None => format!("{:.4}", plan.in_speed),
        },
        plan.handover_out_s,
        plan.out_stop_s,
        plan.score,
        plan.reason
    );
    if let Some(path) = render {
        let sr = out_pcm.sample_rate;
        let context = 15.0;
        // Each deck at its levelling gain, as the player's levelling stage plays it (before the limiter),
        // left and right apart (the render is linear per channel) so the levels are measured in stereo.
        let level = |x: &[f32], db: f32| -> Vec<f32> {
            let g = listen_mix::loudness::db_to_factor(db);
            x.iter().map(|v| v * g).collect()
        };
        let outs = [level(&out_pcm.left, gains.0), level(&out_pcm.right, gains.0)];
        let ins = [level(&in_pcm.left, gains.1), level(&in_pcm.right, gains.1)];
        let r: Vec<listen_mix::render::Rendered> = (0..2)
            .map(|c| listen_mix::render::render_decks(&outs[c], &ins[c], sr, &plan, context, gains))
            .collect();
        let samples: Vec<f32> = r[0].mix.iter().zip(&r[1].mix).map(|(l, r)| 0.5 * (l + r)).collect();
        // Hole and bump against each deck alone on the preview's timeline (its speed and ramp, its
        // levelling gain, no blend gain, trim or filters). Only blends overlap two decks.
        if !matches!(plan.kind, Kind::EchoOut { .. } | Kind::Cut | Kind::Gapless)
            && let Some(blend_from) = r[0].blend_from_s
        {
            let lv = |f: fn(&listen_mix::render::Rendered) -> &Vec<f32>| {
                listen_mix::verify::audio_levels_of(&[f(&r[0]), f(&r[1])], sr, 0.1)
            };
            let lm = lv(|r| &r.mix);
            let lo = lv(|r| &r.out_alone);
            let li = lv(|r| &r.in_alone);
            if let Some((dip, bump)) =
                listen_mix::verify::audio_dip_bump(&lm, &lo, &li, 0.1, blend_from, plan.blend_s)
            {
                println!(
                    "rendered: loudness hole {dip:.2} LU, bump {bump:.2} LU (3 s short-term, levelled, stereo)"
                );
            }
        }
        write_wav(path, sr, &samples).map_err(|e| format!("{path}: {e}"))?;
        println!("rendered {:.1}s preview to {path}", samples.len() as f32 / sr.max(1) as f32);
    }
    Ok(())
}

/// `f` with the synced lyrics of the LRC file at `path` merged in (unchanged when they don't fit).
fn with_lrc(
    f: &listen_core::analysis::Features,
    path: &str,
) -> Result<listen_core::analysis::Features, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let Some(map) = listen_lyrics::vocal_map(&listen_lyrics::parse_lrc(&text), f.audio_end) else {
        println!("  lyrics {path}: not synced or too short; not used");
        return Ok(f.clone());
    };
    let r = listen_mix::lyric::agreement(f, &map).map_or("-".into(), |r| format!("{r:.2}"));
    let merged = listen_mix::lyric::with_lyrics(f, &map);
    println!(
        "  lyrics {path}: {} sung stretches, {} lines, chorus ×{}, agreement with the audio r = {r}: {}",
        map.sung.len(),
        map.line_ends.len(),
        map.chorus.len(),
        if merged.is_some() { "used" } else { "don't fit, not used" }
    );
    Ok(merged.unwrap_or_else(|| f.clone()))
}

/// What mpv decodes for one command-line song.
async fn input_for(
    arg: &str,
    paths: &listen_core::config::Paths,
    config: &listen_core::config::Config,
) -> Result<String, String> {
    if let Some(video) = arg.strip_prefix("ytm:") {
        let r = listen_ytmusic::ytdlp::YtDlp::new().resolve(video).await?;
        return Ok(r.url);
    }
    if arg.contains("://") || std::path::Path::new(arg).exists() {
        return Ok(arg.to_string());
    }
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
    // The raw-file URL only needs the id; any song value carries it.
    let mut song = crate::demo::album_songs(0)[0].clone();
    song.id = arg.to_string();
    song.server_id = server.id.clone();
    client.analysis_url(&song).ok_or_else(|| "no analysis URL".to_string())
}

/// A song's modelled local loudness ([`listen_mix::loudness::LocalLevel`]) and its measured 3 s
/// K-weighted loudness centred on the same times (every 0.5 s, LUFS, no levelling), with the bar curves.
fn level_row(arg: &str, pcm: &listen_analysis::Pcm, f: &listen_core::analysis::Features) -> Option<String> {
    let model = listen_mix::loudness::LocalLevel::new(f)?;
    let step = 0.1;
    let measured = listen_mix::verify::audio_levels_of(&[&pcm.left, &pcm.right], pcm.sample_rate, step);
    let (a, b) = (f64::from(f.audio_start), f64::from(f.audio_end));
    let times: Vec<f64> = (0..).map(|k| a + 0.5 * f64::from(k)).take_while(|t| *t <= b).collect();
    // Index k covers the 3 s ending at k · step, so the window centred on t ends at t + 1.5.
    let at = |t: f64| measured.get(((t + 1.5) / step).round() as usize).copied();
    let bars: Vec<f64> = f
        .grid
        .as_ref()
        .map(|g| (0..=g.bar_count()).map(|k| f64::from(g.bar_time(k))).collect())
        .unwrap_or_default();
    Some(
        serde_json::json!({
            "song": arg, "lufs": f.lufs, "t": times,
            "model": times.iter().map(|&t| model.at(t)).collect::<Vec<_>>(),
            "measured": times.iter().map(|&t| at(t)).collect::<Vec<_>>(),
            "bars": bars, "energy": f.bars.energy, "low": f.bars.low, "centroid": f.bars.centroid,
        })
        .to_string(),
    )
}

/// 16-bit mono PCM WAV.
fn write_wav(path: &str, sr: u32, samples: &[f32]) -> std::io::Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sr.to_le_bytes());
    out.extend_from_slice(&(sr * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_header() {
        let path = std::env::temp_dir().join(format!("listen-check-mix-{}.wav", std::process::id()));
        super::write_wav(path.to_str().unwrap(), 22050, &[0.0, 0.5, -1.0]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 22050);
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), -32767);
    }
}
