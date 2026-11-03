//! `LASTFM_API_KEY=… cargo run -p listen-lastfm --example resolve -- "<artist line>" "<title>" ["<album>"]`:
//! the names Listen would send to Last.fm for a song (dev tool).

use listen_lastfm::{Client, Creds, Resolver, Track};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let key = std::env::var("LASTFM_API_KEY").expect("LASTFM_API_KEY");
    let r = Resolver::new(Client::new(Creds { api_key: key, secret: String::new() }), None);
    for chunk in args.chunks(3) {
        let line = chunk[0].clone();
        let first = line.split([',', '&']).next().unwrap_or(&line).trim().to_string();
        let t = Track {
            artist: first,
            title: chunk.get(1).cloned().unwrap_or_default(),
            album: chunk.get(2).cloned().filter(|a| !a.is_empty()),
            duration_s: None,
            artist_line: line,
        };
        match r.resolve(&t).await {
            Ok(o) => println!(
                "{} – {} [{}]  →  {} – {} [{}]",
                t.artist_line,
                t.title,
                t.album.as_deref().unwrap_or(""),
                o.artist,
                o.title,
                o.album.as_deref().unwrap_or("")
            ),
            Err(e) => println!("{} – {}: {e}", t.artist_line, t.title),
        }
    }
}
