//! `listen --check-mix <a> <b> [--render out.wav]`: plans the DJ transition from `a` into `b` as the player
//! would and optionally renders an offline preview (`listen_mix::render`). A song is a Navidrome song id
//! (first Navidrome server), `ytm:<videoId>` (via yt-dlp) or a local file / URL; both are analysed afresh.

use listen_analysis::{BeatModel, BeatSource, Cancel, Opts};
use listen_mix::plan::PlanCtx;
use listen_source::MusicSource;

pub fn run(args: &[String]) -> i32 {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        eprintln!(
            "usage: listen --check-mix <a> <b> [--render out.wav]  (Navidrome id, ytm:<videoId>, file or URL)"
        );
        return 2;
    };
    let render = args.iter().position(|x| x == "--render").and_then(|i| args.get(i + 1)).cloned();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(check(a, b, render.as_deref())).map_or_else(
        |e| {
            eprintln!("✗ {e}");
            1
        },
        |()| 0,
    )
}

async fn check(a: &str, b: &str, render: Option<&str>) -> Result<(), String> {
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
    for arg in [a, b] {
        let input = input_for(arg, &paths, &config).await?;
        let pcm =
            listen_analysis::decode(&input, &Cancel::default()).await.map_err(|e| format!("{arg}: {e}"))?;
        let f = listen_analysis::analyze_in_background(pcm.clone(), opts.clone(), &Cancel::default())
            .await
            .ok_or("analysis cancelled")?;
        println!("{arg}: {:.1}s, {}", pcm.duration(), listen_core::analysis::FlowFeatures::of(&f).describe());
        sides.push((pcm, f));
    }
    let ((out_pcm, fa), (in_pcm, fb)) = (&sides[0], &sides[1]);
    let ctx = PlanCtx { settings: &config.settings.mixing, out_position_s: 0.0, voice_break: None };
    let plan = listen_mix::plan(fa, fb, &ctx);
    println!(
        "\n{:?}: out {:.2}s → in {:.2}s (loaded at {:.2}s), blend {:.1}s, in speed {:.4}, handover {:.2}s, out stops {:.2}s, score {:.2}\n{}",
        plan.kind,
        plan.out_mix_start_s,
        plan.in_mix_start_s,
        plan.in_load_at_s,
        plan.blend_s,
        plan.in_speed,
        plan.handover_out_s,
        plan.out_stop_s,
        plan.score,
        plan.reason
    );
    if let Some(path) = render {
        let sr = out_pcm.sample_rate;
        let samples = listen_mix::render(&out_pcm.mono(), &in_pcm.mono(), sr, &plan, 15.0);
        write_wav(path, sr, &samples).map_err(|e| format!("{path}: {e}"))?;
        println!("rendered {:.1}s preview to {path}", samples.len() as f32 / sr.max(1) as f32);
    }
    Ok(())
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
