//! Lists what the YouTube Music check (`listen --check-ytm-session --write`) left behind on the account it
//! uses (`check_ytm::ytm_server`). Usage: `ytm_state [--fix] <song title>`. Read-only unless `--fix` is
//! given, which deletes the check's playlists (`check_ytm::is_check_playlist`) and unlikes the library
//! song with that exact title, only when exactly one song has it.
use listen_core::model::ServerId;
use listen_source::{MusicSource, SongQuery};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fix = args.iter().any(|a| a == "--fix");
    let title = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let paths = listen_core::config::Paths::discover().unwrap();
    let config = listen_core::config::Config::load(&paths.config_file()).unwrap();
    let server = listen::check_ytm::ytm_server(&config).expect("no YouTube Music account configured");
    let (dir, id) = (paths.config_dir.clone(), server.id.clone());
    let secret = tokio::task::spawn_blocking(move || listen::secrets::Secrets::for_user(&dir).get(&id))
        .await
        .unwrap()
        .unwrap();
    let yt = listen_ytmusic::YouTubeMusic::connect(ServerId(server.id.0.clone()), &secret).await.unwrap();
    let lib = yt.songs(&SongQuery { limit: 100_000, ..Default::default() }).await.unwrap();
    let titled: Vec<_> = lib.items.iter().filter(|s| !title.is_empty() && s.title == title).collect();
    for s in &titled {
        println!("library: “{}” [{}] liked={}", s.title, s.id, s.favorite);
    }
    match titled.as_slice() {
        [s] if fix && s.favorite => {
            yt.set_favorite(std::slice::from_ref(&s.id), false).await.unwrap();
            println!("  → unliked");
        }
        [_, _, ..] if fix => println!("  several songs have this title: unlike the right one by hand"),
        _ => {}
    }
    let liked =
        yt.songs(&SongQuery { favorites_only: true, limit: 100_000, ..Default::default() }).await.unwrap();
    for (i, s) in liked.items.iter().enumerate().filter(|(_, s)| s.title == title) {
        println!("liked music: position {i} “{}” [{}]", s.title, s.id);
    }
    println!("liked music total: {}", liked.items.len());
    for p in yt.playlists().await.unwrap().iter().filter(|p| listen::check_ytm::is_check_playlist(&p.name)) {
        println!("playlist: “{}” [{}]", p.name, p.id);
        if fix {
            match yt.delete_playlist(&p.id).await {
                Ok(()) => println!("  → deleted"),
                Err(e) => println!("  → delete error: {e}"),
            }
        }
    }
}
