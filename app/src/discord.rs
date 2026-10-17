//! Discord Rich Presence, off by default like Feishin. [`build`] turns the
//! playback state and the Discord settings into a [`Presence`] (Feishin's
//! `use-discord-rpc.ts`); [`Discord::apply`] sends it over Discord's IPC.

use discord_rich_presence::activity::{Activity, ActivityType, Assets, StatusDisplayType, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use listen_core::settings::{DiscordDisplay, DiscordLink, DiscordSettings};
use std::sync::{Arc, Mutex};

/// Feishin's default application id (a custom one can be set in settings).
pub const DEFAULT_CLIENT_ID: &str = "1560790551337640127";

/// Discord shows at most this many characters per line (Feishin `MAX_FIELD_LENGTH`).
pub const MAX_FIELD_LENGTH: usize = 127;
/// Longest link Discord accepts (Feishin `MAX_URL_LENGTH`).
pub const MAX_URL_LENGTH: usize = 256;
/// The asset every Feishin-compatible application carries.
pub const FALLBACK_IMAGE: &str = "icon";
/// Interval at which Feishin re-sends the presence: 15 s.
pub const REFRESH_SECS: u64 = 15;

/// Cuts a line to Discord's limit with an ellipsis (Feishin `truncate`).
pub fn truncate(field: &str) -> String {
    if field.chars().count() <= MAX_FIELD_LENGTH {
        field.to_string()
    } else {
        let mut s: String = field.chars().take(MAX_FIELD_LENGTH - 1).collect();
        s.push('…');
        s
    }
}

/// Discord rejects lines under two characters (Feishin `padEnd(2, ' ')`).
fn pad(field: &str, fallback: &str) -> String {
    let s = if field.is_empty() { fallback } else { field };
    truncate(&format!("{s:<2}"))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// What plays, as far as the presence needs it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    /// MusicBrainz ids from the server, for the MusicBrainz link types.
    pub mbz: listen_core::model::MbzIds,
}

/// The playback state the presence is built from.
#[derive(Debug, Clone)]
pub struct Input {
    pub track: Track,
    pub playing: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
    /// Current time in unix seconds.
    pub now: i64,
    /// A public https URL of the cover, if there is one.
    pub image: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Playing,
    Listening,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLine {
    Name,
    State,
    Details,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Presence {
    pub kind: Kind,
    pub status: StatusLine,
    pub details: String,
    pub state: String,
    pub details_url: Option<String>,
    pub state_url: Option<String>,
    pub large_image: String,
    pub large_text: String,
    pub large_url: Option<String>,
    pub small_image: Option<&'static str>,
    pub small_text: Option<&'static str>,
    /// Unix seconds; only while playing.
    pub started_at: Option<i64>,
    pub ends_at: Option<i64>,
}

/// The presence for the state, or `None` when it must be cleared (paused and
/// "show presence when paused" is off).
pub fn build(opts: &DiscordSettings, input: &Input) -> Option<Presence> {
    if !input.playing && !opts.show_paused {
        return None;
    }
    let t = &input.track;
    let mut p = Presence {
        kind: if opts.show_as_listening { Kind::Listening } else { Kind::Playing },
        status: match opts.display {
            DiscordDisplay::Feishin => StatusLine::Name,
            DiscordDisplay::Song => StatusLine::Details,
            DiscordDisplay::Artist => StatusLine::State,
        },
        details: pad(&t.title, "Idle"),
        state: pad(&t.artists.join(", "), "Unknown artist"),
        details_url: None,
        state_url: None,
        large_image: input.image.clone().unwrap_or_else(|| FALLBACK_IMAGE.into()),
        large_text: pad(t.album.as_deref().unwrap_or(""), "Unknown album"),
        large_url: None,
        small_image: None,
        small_text: None,
        started_at: None,
        ends_at: None,
    };
    if matches!(opts.link, DiscordLink::LastFm | DiscordLink::MusicBrainzLastFm)
        && let Some(artist) = t.artists.first()
    {
        p.state_url = Some(format!("https://www.last.fm/music/{}", encode(artist)));
        let album_artist = t.album_artists.first().unwrap_or(artist);
        let album = format!(
            "https://www.last.fm/music/{}/{}",
            encode(album_artist),
            encode(t.album.as_deref().unwrap_or("_"))
        );
        let details = format!("{album}/{}", encode(&t.title));
        // Discord drops lines whose link is too long.
        if album.len() <= MAX_URL_LENGTH {
            p.large_url = Some(album);
        }
        if details.len() <= MAX_URL_LENGTH {
            p.details_url = Some(details);
        }
    }
    // MusicBrainz links override the Last.fm ones where the server gave ids
    // (Feishin: track id, else recording id, for the details line; release for the cover).
    if matches!(opts.link, DiscordLink::MusicBrainz | DiscordLink::MusicBrainzLastFm) {
        if let Some(id) = &t.mbz.track {
            p.details_url = Some(format!("https://musicbrainz.org/track/{id}"));
        } else if let Some(id) = &t.mbz.recording {
            p.details_url = Some(format!("https://musicbrainz.org/recording/{id}"));
        }
        if let Some(id) = &t.mbz.album {
            p.large_url = Some(format!("https://musicbrainz.org/release/{id}"));
        }
    }
    if input.playing {
        let start = input.now - input.position_secs as i64;
        p.started_at = Some(start);
        p.ends_at = Some(start + input.duration_secs as i64);
        if opts.state_icon {
            p.small_image = Some("playing");
            p.small_text = Some("Playing");
        }
    } else {
        p.small_image = Some("paused");
        p.small_text = Some("Paused");
    }
    Some(p)
}

/// Whether a server address is reachable from the internet, so Discord can
/// fetch its cover: https, and not a LAN name or address.
pub fn is_public_url(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    if u.scheme() != "https" {
        return false;
    }
    match u.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            d.contains('.')
                && !d.ends_with(".local")
                && !d.ends_with(".lan")
                && !d.ends_with(".internal")
                && !d.ends_with(".home.arpa")
                && d != "localhost"
        }
        Some(url::Host::Ipv4(ip)) => {
            !(ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified())
        }
        Some(url::Host::Ipv6(ip)) => !(ip.is_loopback() || ip.is_unspecified()),
        None => false,
    }
}

#[derive(Clone, Default)]
pub struct Discord {
    client: Arc<Mutex<Option<DiscordIpcClient>>>,
}

impl std::fmt::Debug for Discord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Discord")
    }
}

impl Discord {
    /// Sets (or clears, with `None`) the presence. Blocking IPC: run off the UI thread.
    pub fn apply(&self, client_id: &str, presence: Option<Presence>) -> Result<(), String> {
        let mut guard = self.client.lock().map_err(|e| e.to_string())?;
        let Some(p) = presence else {
            if let Some(c) = guard.as_mut() {
                let _ = c.clear_activity();
            }
            return Ok(());
        };
        if guard.is_none() {
            let mut c = DiscordIpcClient::new(client_id);
            c.connect().map_err(|e| format!("Discord not running? ({e})"))?;
            *guard = Some(c);
        }
        let client = guard.as_mut().expect("connected above");
        if let Err(e) = client.set_activity(activity(&p)) {
            *guard = None; // reconnect next time
            return Err(e.to_string());
        }
        Ok(())
    }

    /// Drops the connection (a new application id, or Discord switched off).
    pub fn quit(&self) {
        if let Ok(mut guard) = self.client.lock()
            && let Some(mut c) = guard.take()
        {
            let _ = c.clear_activity();
            let _ = c.close();
        }
    }
}

fn activity(p: &Presence) -> Activity<'_> {
    let mut a = Activity::new()
        .activity_type(match p.kind {
            Kind::Playing => ActivityType::Playing,
            Kind::Listening => ActivityType::Listening,
        })
        .status_display_type(match p.status {
            StatusLine::Name => StatusDisplayType::Name,
            StatusLine::State => StatusDisplayType::State,
            StatusLine::Details => StatusDisplayType::Details,
        })
        .details(&p.details)
        .state(&p.state);
    if let Some(u) = &p.details_url {
        a = a.details_url(u);
    }
    if let Some(u) = &p.state_url {
        a = a.state_url(u);
    }
    if let (Some(start), Some(end)) = (p.started_at, p.ends_at) {
        a = a.timestamps(Timestamps::new().start(start).end(end));
    }
    let mut assets = Assets::new().large_image(&p.large_image).large_text(&p.large_text);
    if let Some(u) = &p.large_url {
        assets = assets.large_url(u);
    }
    if let Some(i) = p.small_image {
        assets = assets.small_image(i);
    }
    if let Some(t) = p.small_text {
        assets = assets.small_text(t);
    }
    a.assets(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(playing: bool) -> Input {
        Input {
            track: Track {
                title: "Song".into(),
                artists: vec!["A & B".into(), "C".into()],
                album: Some("Album".into()),
                album_artists: vec!["A & B".into()],
                mbz: Default::default(),
            },
            playing,
            position_secs: 30.0,
            duration_secs: 200.0,
            now: 1_000_000,
            image: None,
        }
    }

    #[test]
    fn defaults_play_with_timestamps_and_icon() {
        let p = build(&DiscordSettings::default(), &input(true)).unwrap();
        assert_eq!(p.kind, Kind::Playing);
        assert_eq!(p.status, StatusLine::Name);
        assert_eq!((p.details.as_str(), p.state.as_str()), ("Song", "A & B, C"));
        assert_eq!((p.started_at, p.ends_at), (Some(999_970), Some(1_000_170)));
        assert_eq!(p.small_image, Some("playing"));
        assert_eq!(p.large_image, "icon");
        assert_eq!(p.large_text, "Album");
        assert!(p.details_url.is_none() && p.state_url.is_none());
    }

    #[test]
    fn listening_and_display_type() {
        let mut o = DiscordSettings { show_as_listening: true, ..Default::default() };
        let p = build(&o, &input(true)).unwrap();
        assert_eq!(p.kind, Kind::Listening);
        o.display = DiscordDisplay::Song;
        assert_eq!(build(&o, &input(true)).unwrap().status, StatusLine::Details);
        o.display = DiscordDisplay::Artist;
        assert_eq!(build(&o, &input(true)).unwrap().status, StatusLine::State);
    }

    #[test]
    fn paused_shows_the_paused_icon_without_timer_or_clears() {
        let mut o = DiscordSettings::default();
        let p = build(&o, &input(false)).unwrap();
        assert_eq!(p.small_image, Some("paused"));
        assert_eq!(p.small_text, Some("Paused"));
        assert!(p.started_at.is_none() && p.ends_at.is_none());
        // The paused icon is not optional; the playing one is.
        o.state_icon = false;
        assert_eq!(build(&o, &input(false)).unwrap().small_image, Some("paused"));
        assert_eq!(build(&o, &input(true)).unwrap().small_image, None);
        o.show_paused = false;
        assert!(build(&o, &input(false)).is_none());
        assert!(build(&o, &input(true)).is_some());
    }

    #[test]
    fn last_fm_links() {
        let o = DiscordSettings { link: DiscordLink::LastFm, ..Default::default() };
        let p = build(&o, &input(true)).unwrap();
        assert_eq!(p.state_url.as_deref(), Some("https://www.last.fm/music/A%20%26%20B"));
        assert_eq!(p.large_url.as_deref(), Some("https://www.last.fm/music/A%20%26%20B/Album"));
        assert_eq!(p.details_url.as_deref(), Some("https://www.last.fm/music/A%20%26%20B/Album/Song"));
        // MusicBrainz alone has no ids to link; with the fallback it links Last.fm.
        let mb = DiscordSettings { link: DiscordLink::MusicBrainz, ..Default::default() };
        assert!(build(&mb, &input(true)).unwrap().details_url.is_none());
        let fb = DiscordSettings { link: DiscordLink::MusicBrainzLastFm, ..Default::default() };
        assert!(build(&fb, &input(true)).unwrap().details_url.is_some());
    }

    #[test]
    fn music_brainz_links_use_the_servers_ids() {
        let mut i = input(true);
        i.track.mbz = listen_core::model::MbzIds {
            track: None,
            recording: Some("rec-1".into()),
            album: Some("rel-1".into()),
        };
        let mb = DiscordSettings { link: DiscordLink::MusicBrainz, ..Default::default() };
        let p = build(&mb, &i).unwrap();
        assert_eq!(p.details_url.as_deref(), Some("https://musicbrainz.org/recording/rec-1"));
        assert_eq!(p.large_url.as_deref(), Some("https://musicbrainz.org/release/rel-1"));
        assert!(p.state_url.is_none());
        // A track id wins over the recording id.
        i.track.mbz.track = Some("trk-1".into());
        let p = build(&mb, &i).unwrap();
        assert_eq!(p.details_url.as_deref(), Some("https://musicbrainz.org/track/trk-1"));
        // With the fallback: MusicBrainz where ids exist, Last.fm elsewhere.
        let fb = DiscordSettings { link: DiscordLink::MusicBrainzLastFm, ..Default::default() };
        i.track.mbz.album = None;
        let p = build(&fb, &i).unwrap();
        assert_eq!(p.details_url.as_deref(), Some("https://musicbrainz.org/track/trk-1"));
        assert_eq!(p.large_url.as_deref(), Some("https://www.last.fm/music/A%20%26%20B/Album"));
    }

    #[test]
    fn long_links_are_dropped_and_long_lines_truncated() {
        let mut i = input(true);
        i.track.title = "x".repeat(300);
        let o = DiscordSettings { link: DiscordLink::LastFm, ..Default::default() };
        let p = build(&o, &i).unwrap();
        assert!(p.details_url.is_none());
        assert!(p.large_url.is_some());
        assert_eq!(p.details.chars().count(), MAX_FIELD_LENGTH);
        assert!(p.details.ends_with('…'));
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn short_and_missing_fields_are_padded() {
        let mut i = input(true);
        i.track = Track { title: "x".into(), ..Default::default() };
        let p = build(&DiscordSettings::default(), &i).unwrap();
        assert_eq!(p.details, "x ");
        assert_eq!(p.state, "Unknown artist");
        assert_eq!(p.large_text, "Unknown album");
    }

    #[test]
    fn the_cover_is_used_when_given() {
        let mut i = input(true);
        i.image = Some("https://img.example/c.jpg".into());
        assert_eq!(build(&DiscordSettings::default(), &i).unwrap().large_image, "https://img.example/c.jpg");
    }

    #[test]
    fn only_public_servers_share_their_cover() {
        for (url, public) in [
            ("https://music.example.com/rest/x", true),
            ("https://8.8.8.8:4533", true),
            ("http://music.example.com", false),
            ("https://192.168.1.5:4533", false),
            ("https://10.0.0.2", false),
            ("https://127.0.0.1", false),
            ("https://navidrome", false),
            ("https://nas.local", false),
            ("https://localhost:4533", false),
            ("not a url", false),
        ] {
            assert_eq!(is_public_url(url), public, "{url}");
        }
    }
}
