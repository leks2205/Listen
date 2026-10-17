//! Lists what the YouTube Music check left behind. Read-only unless `--fix` is given, which deletes the
//! "Listen check" playlists and unlikes the library songs titled like the first argument.
use listen_core::model::{ServerId, SourceKind};
use listen_source::{MusicSource, SongQuery};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fix = args.iter().any(|a| a == "--fix");
    let title = args.get(1).cloned().unwrap_or_default();
    let paths = listen_core::config::Paths::discover().unwrap();
    let config = listen_core::config::Config::load(&paths.config_file()).unwrap();
    let server = config.servers.iter().find(|s| s.kind == SourceKind::YouTubeMusic).unwrap();
    let (dir, id) = (paths.config_dir.clone(), server.id.clone());
    let secret = tokio::task::spawn_blocking(move || listen::secrets::Secrets::new(&dir).get(&id))
        .await
        .unwrap()
        .unwrap();
    let yt = listen_ytmusic::YouTubeMusic::connect(ServerId(server.id.0.clone()), &secret).await.unwrap();
    let lib = yt.songs(&SongQuery { limit: 100_000, ..Default::default() }).await.unwrap();
    for s in lib.items.iter().filter(|s| s.title == title) {
        println!("library: “{}” [{}] liked={}", s.title, s.id, s.favorite);
        if fix && s.favorite {
            yt.set_favorite(std::slice::from_ref(&s.id), false).await.unwrap();
            println!("  → unliked");
        }
    }
    let liked =
        yt.songs(&SongQuery { favorites_only: true, limit: 100_000, ..Default::default() }).await.unwrap();
    for (i, s) in liked.items.iter().enumerate().filter(|(_, s)| s.title == title) {
        println!("liked music: position {i} “{}” [{}]", s.title, s.id);
    }
    println!("liked music total: {}", liked.items.len());
    for p in yt.playlists().await.unwrap().iter().filter(|p| p.name.starts_with("Listen check")) {
        println!("playlist: “{}” [{}]", p.name, p.id);
        if fix {
            match yt.delete_playlist(&p.id).await {
                Ok(()) => println!("  → deleted"),
                Err(e) => println!("  → delete error: {e}"),
            }
        }
    }
}
