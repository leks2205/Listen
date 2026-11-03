//! `LASTFM_API_KEY=… LASTFM_SECRET=… cargo run -p listen-lastfm --example token`: checks that signed calls work
//! (asks for a sign-in token; prints only whether it came).
use listen_lastfm::{Client, Creds};

#[tokio::main]
async fn main() {
    let c = Client::new(Creds {
        api_key: std::env::var("LASTFM_API_KEY").expect("LASTFM_API_KEY"),
        secret: std::env::var("LASTFM_SECRET").expect("LASTFM_SECRET"),
    });
    match c.get_token().await {
        Ok(t) => println!("signed call ok (token of {} chars)", t.len()),
        Err(e) => println!("signed call failed: {e}"),
    }
}
