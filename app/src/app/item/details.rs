//! The "Get info" modal (Feishin `features/item-details`): every fact about a
//! song, album, artist or playlist. The rows are built from what the list
//! model already has plus what the server sends for [`ItemDetails`]
//! (`MusicSource::item_details`); empty values are left out.

use super::super::menu::MenuTarget;
use super::super::{Listen, Message, album_page};
use iced::Task;
use listen_core::model::{Album, Artist, Playlist, Song, SourceKind, format_duration, join_names};
use listen_source::{Capabilities, ItemDetails, ItemKind};
use std::path::PathBuf;

pub const MODAL: &str = "item-details";

/// What the modal is about.
#[derive(Debug, Clone)]
pub enum Subject {
    Song(Box<Song>),
    Album(Box<Album>),
    Artist(Box<Artist>),
    Playlist(Box<Playlist>),
}

impl Subject {
    /// The items of a menu target (genres and folders have no details).
    pub fn from_target(target: &MenuTarget) -> Vec<Subject> {
        match target {
            MenuTarget::Songs { songs, .. } => {
                songs.iter().map(|s| Subject::Song(Box::new(s.clone()))).collect()
            }
            MenuTarget::Albums(v) => v.iter().map(|a| Subject::Album(Box::new(a.clone()))).collect(),
            MenuTarget::Artists(v) => v.iter().map(|a| Subject::Artist(Box::new(a.clone()))).collect(),
            MenuTarget::Playlists(v) => v.iter().map(|p| Subject::Playlist(Box::new(p.clone()))).collect(),
            MenuTarget::Genres(_) | MenuTarget::Folders(_) => Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Subject::Song(s) => &s.title,
            Subject::Album(a) => &a.name,
            Subject::Artist(a) => &a.name,
            Subject::Playlist(p) => &p.name,
        }
    }

    pub fn server(&self) -> &listen_core::model::ServerId {
        match self {
            Subject::Song(s) => &s.server_id,
            Subject::Album(a) => &a.server_id,
            Subject::Artist(a) => &a.server_id,
            Subject::Playlist(p) => &p.server_id,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Subject::Song(s) => &s.id,
            Subject::Album(a) => &a.id,
            Subject::Artist(a) => &a.id,
            Subject::Playlist(p) => &p.id,
        }
    }

    pub fn source_kind(&self) -> SourceKind {
        match self {
            Subject::Song(s) => s.source,
            Subject::Album(a) => a.source,
            Subject::Artist(a) => a.source,
            Subject::Playlist(p) => p.source,
        }
    }

    /// What `item_details` is asked for; playlists have no server-side extras.
    pub fn item_kind(&self) -> Option<ItemKind> {
        match self {
            Subject::Song(_) => Some(ItemKind::Song),
            Subject::Album(_) => Some(ItemKind::Album),
            Subject::Artist(_) => Some(ItemKind::Artist),
            Subject::Playlist(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Text,
    /// A file path: openable in the file manager when the file is local.
    Path,
    /// Long text (comment, biography): wraps.
    Long,
}

/// One line of the modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub kind: RowKind,
    /// A page the value is about (MusicBrainz).
    pub link: Option<String>,
}

/// What the server said about an entry.
#[derive(Debug, Clone, Default)]
pub enum Load {
    /// Not asked (yet).
    #[default]
    Idle,
    Loading,
    Done,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub subject: Subject,
    pub load: Load,
    pub details: Option<ItemDetails>,
    pub rows: Vec<Row>,
    /// The file on this computer (local folder songs).
    pub local_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DetailsState {
    pub entries: Vec<Entry>,
    pub selected: usize,
}

impl DetailsState {
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }
}

#[derive(Debug, Clone)]
pub enum DetailsMsg {
    /// Show another item (several were selected).
    Select(usize),
    Loaded(usize, Box<Result<ItemDetails, String>>),
    Copy(String),
    OpenLink(String),
    ShowFile(PathBuf),
}

// --- rows ---------------------------------------------------------------

struct Rows(Vec<Row>);

impl Rows {
    fn add(&mut self, label: &str, value: Option<String>) {
        if let Some(v) = value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            self.0.push(Row { label: label.into(), value: v, kind: RowKind::Text, link: None });
        }
    }

    fn long(&mut self, label: &str, value: Option<String>) {
        self.add(label, value);
        if let Some(r) = self.0.last_mut().filter(|r| r.label == label) {
            r.kind = RowKind::Long;
        }
    }

    fn path(&mut self, value: Option<String>) {
        self.add("Path", value);
        if let Some(r) = self.0.last_mut().filter(|r| r.label == "Path") {
            r.kind = RowKind::Path;
        }
    }

    fn link(&mut self, label: &str, value: String, url: String) {
        self.add(label, Some(value));
        if let Some(r) = self.0.last_mut().filter(|r| r.label == label) {
            r.link = Some(url);
        }
    }

    fn num<T: ToString>(&mut self, label: &str, value: Option<T>) {
        self.add(label, value.map(|v| v.to_string()));
    }

    fn yes_no(&mut self, label: &str, value: Option<bool>) {
        self.add(label, value.map(|v| if v { "Yes" } else { "No" }.to_string()));
    }
}

/// "2024-03-01T10:00:00Z" as "2024-03-01 10:00"; anything else as sent.
pub fn date_text(iso: &str) -> String {
    let iso = iso.trim();
    match (iso.get(..10), iso.get(10..11), iso.get(11..16)) {
        (Some(d), Some("T"), Some(t)) if d.matches('-').count() == 2 => format!("{d} {t}"),
        _ => iso.to_string(),
    }
}

/// 1536 -> "1.5 KiB".
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

fn db(v: Option<f32>) -> Option<String> {
    v.map(|g| format!("{g:.2} dB"))
}

fn peak(v: Option<f32>) -> Option<String> {
    v.map(|p| format!("{p:.4}"))
}

fn stars(rating: Option<u8>) -> Option<String> {
    rating.filter(|r| *r > 0).map(|r| format!("{} ({r}/5)", "★".repeat(usize::from(r.min(5)))))
}

fn khz(rate: u32) -> String {
    if rate.is_multiple_of(1000) {
        format!("{} kHz", rate / 1000)
    } else {
        format!("{:.1} kHz", f64::from(rate) / 1000.0)
    }
}

fn list(items: &[String]) -> Option<String> {
    (!items.is_empty()).then(|| items.join(", "))
}

fn explicit_text(model: bool, server: Option<bool>) -> Option<String> {
    match (model, server) {
        (true, _) | (_, Some(true)) => Some("Explicit".into()),
        (false, Some(false)) => Some("Clean".into()),
        _ => None,
    }
}

/// Strips HTML tags from a Last.fm style biography.
pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").trim().to_string()
}

fn musicbrainz(rows: &mut Rows, label: &str, entity: &str, id: Option<&String>) {
    if let Some(id) = id {
        rows.link(label, id.clone(), format!("https://musicbrainz.org/{entity}/{id}"));
    }
}

/// The rows of `subject`, with the server's `details` when there are any.
pub fn rows(subject: &Subject, details: Option<&ItemDetails>) -> Vec<Row> {
    let d = details.cloned().unwrap_or_default();
    let mut r = Rows(Vec::new());
    match subject {
        Subject::Song(s) => song_rows(&mut r, s, &d),
        Subject::Album(a) => album_rows(&mut r, a, &d),
        Subject::Artist(a) => artist_rows(&mut r, a, &d),
        Subject::Playlist(p) => playlist_rows(&mut r, p),
    }
    for (label, value) in &d.tags {
        r.add(label, Some(value.clone()));
    }
    r.add("Id", Some(subject.id().to_string()));
    r.0
}

fn song_rows(r: &mut Rows, s: &Song, d: &ItemDetails) {
    r.add("Title", Some(s.title.clone()));
    r.path(d.path.clone().or_else(|| s.path.clone()));
    r.add("Album artist", Some(join_names(&s.album_artists)));
    r.add("Artist", Some(join_names(&s.artists)));
    r.add("Album", s.album.clone());
    r.num("Disc", s.disc_number);
    r.num("Track", s.track_number);
    r.num("Year", s.year);
    r.add("Explicit", explicit_text(s.explicit, d.explicit));
    r.add("Genres", list(&s.genres));
    r.add("Duration", Some(format_duration(d.duration.filter(|d| !d.is_zero()).unwrap_or(s.duration))));
    r.yes_no("Compilation", d.compilation);
    r.add(
        "Codec",
        d.suffix.clone().or_else(|| s.suffix.clone()).or_else(|| s.codec.clone()).map(|c| c.to_uppercase()),
    );
    r.add("Bitrate", d.bit_rate.or(s.bit_rate).filter(|b| *b > 0).map(|b| format!("{b} kbps")));
    r.add("Sample rate", d.sample_rate.or(s.sample_rate).filter(|b| *b > 0).map(khz));
    r.add("Bit depth", d.bit_depth.or(s.bit_depth).filter(|b| *b > 0).map(|b| format!("{b} bit")));
    r.add(
        "Channels",
        d.channels.or(s.channels).filter(|c| *c > 0).map(|c| match c {
            1 => "Mono".to_string(),
            2 => "Stereo".to_string(),
            n => format!("{n} channels"),
        }),
    );
    r.add("Size", d.size.or(s.size).map(size_text));
    r.num("BPM", d.bpm.or(s.bpm).filter(|b| *b > 0));
    r.yes_no("Favorite", (s.source != SourceKind::Local).then_some(s.favorite));
    r.add("Rating", stars(s.rating));
    r.num("Play count", d.play_count.or(s.play_count));
    r.add("Last played", d.last_played.as_deref().map(date_text));
    r.add("Added", d.created.as_deref().or(s.created.as_deref()).map(date_text));
    r.add("Modified", d.updated.as_deref().map(date_text));
    let gain = if d.gain == Default::default() { &s.gain } else { &d.gain };
    r.add("Album gain", db(gain.album_gain));
    r.add("Track gain", db(gain.track_gain));
    r.add("Album peak", peak(gain.album_peak));
    r.add("Track peak", peak(gain.track_peak));
    r.yes_no("Lyrics", d.has_lyrics);
    r.long("Comment", d.comment.clone().or_else(|| s.comment.clone()));
    musicbrainz(r, "MusicBrainz ID", "recording", d.musicbrainz_id.as_ref());
    for (label, id) in &d.mbids {
        let entity = match label.as_str() {
            "Album" => "release",
            "Release group" => "release-group",
            "Artist" | "Album artist" => "artist",
            _ => "recording",
        };
        musicbrainz(r, &format!("MusicBrainz {}", label.to_lowercase()), entity, Some(id));
    }
    r.add("Record label", list(&d.record_labels));
    r.add("Version", d.version.clone());
    for (role, name) in &d.participants {
        let mut label = role.clone();
        if let Some(first) = label.get(..1) {
            label = first.to_uppercase() + &label[1..];
        }
        r.add(&label, Some(name.clone()));
    }
}

fn album_rows(r: &mut Rows, a: &Album, d: &ItemDetails) {
    r.add("Title", Some(a.name.clone()));
    r.add("Album artist", Some(join_names(&a.album_artists)));
    r.add("Release type", a.release_type.clone().or_else(|| d.release_type.clone()));
    r.add("Genres", list(&a.genres));
    r.add("Duration", (!a.duration.is_zero()).then(|| format_duration(a.duration)));
    r.num("Year", a.year);
    r.add("Original date", a.info.original_date.clone());
    r.add("Release date", a.info.release_date.clone());
    r.num("Songs", a.song_count.or((!a.songs.is_empty()).then_some(a.songs.len() as u32)));
    r.add("Explicit", explicit_text(a.info.explicit, d.explicit));
    r.yes_no("Compilation", d.compilation);
    r.add("Size", a.info.size.map(size_text));
    r.yes_no("Favorite", (a.source != SourceKind::Local).then_some(a.favorite));
    r.add("Rating", stars(a.rating));
    r.num("Play count", a.play_count.or(d.play_count));
    r.add("Last played", d.last_played.as_deref().map(date_text));
    r.add("Added", d.created.as_deref().map(date_text));
    r.add("Modified", d.updated.as_deref().map(date_text));
    r.long("Comment", a.info.comment.clone().or_else(|| d.comment.clone()));
    musicbrainz(r, "MusicBrainz ID", "release", a.info.musicbrainz_id.as_ref().or(d.musicbrainz_id.as_ref()));
    r.add("Version", a.info.version.clone().or_else(|| d.version.clone()));
    let labels = if a.info.record_labels.is_empty() { &d.record_labels } else { &a.info.record_labels };
    r.add("Record label", list(labels));
    r.add("Moods", list(&a.info.moods));
    r.add("Groupings", list(&a.info.groupings));
    for (name, values) in &a.info.tags {
        r.add(name, list(values));
    }
}

fn artist_rows(r: &mut Rows, a: &Artist, d: &ItemDetails) {
    r.add("Name", Some(a.name.clone()));
    r.num("Albums", a.album_count);
    r.num("Songs", a.song_count);
    r.yes_no("Favorite", (a.source != SourceKind::Local).then_some(a.favorite));
    r.add("Rating", stars(a.rating));
    r.num("Play count", d.play_count);
    r.add("Last played", d.last_played.as_deref().map(date_text));
    musicbrainz(r, "MusicBrainz ID", "artist", d.musicbrainz_id.as_ref());
    r.long("Biography", a.biography.as_deref().map(plain_text));
}

fn playlist_rows(r: &mut Rows, p: &Playlist) {
    r.add("Title", Some(p.name.clone()));
    r.long("Description", p.description.clone());
    r.num("Songs", p.song_count);
    r.add("Duration", (!p.duration.is_zero()).then(|| format_duration(p.duration)));
    r.add("Owner", p.owner.clone());
    r.yes_no("Public", (p.source != SourceKind::YouTubeMusic).then_some(p.public));
}

impl Entry {
    fn new(state: &Listen, subject: Subject) -> Self {
        let local_path = match &subject {
            Subject::Song(s) => state.song_file(s),
            _ => None,
        };
        let rows = rows(&subject, None);
        Entry { subject, load: Load::Idle, details: None, rows, local_path }
    }
}

impl Listen {
    /// Opens the modal for the items of `target`.
    pub(in crate::app) fn open_details(&mut self, target: &MenuTarget) -> Task<Message> {
        let subjects = Subject::from_target(target);
        if subjects.is_empty() {
            return Task::none();
        }
        let entries = subjects.into_iter().map(|s| Entry::new(self, s)).collect();
        self.item.details = Some(DetailsState { entries, selected: 0 });
        self.open_modal(MODAL);
        self.load_details(0)
    }

    /// Asks the server for the extra facts of entry `index` (once).
    fn load_details(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self.item.details.as_ref().and_then(|d| d.entries.get(index)) else {
            return Task::none();
        };
        if !matches!(entry.load, Load::Idle) {
            return Task::none();
        }
        let source = self.sources.get(entry.subject.server()).cloned();
        let (Some(kind), Some(source)) = (entry.subject.item_kind(), source) else { return Task::none() };
        if !source.capabilities().contains(Capabilities::ITEM_DETAILS) {
            return Task::none();
        }
        let id = entry.subject.id().to_string();
        if let Some(e) = self.item.details.as_mut().and_then(|d| d.entries.get_mut(index)) {
            e.load = Load::Loading;
        }
        Task::perform(
            async move { source.item_details(kind, &id).await.map_err(super::super::err) },
            move |r| Message::Item(super::ItemMsg::Details(DetailsMsg::Loaded(index, Box::new(r)))),
        )
    }

    pub(super) fn on_details(&mut self, msg: DetailsMsg) -> Task<Message> {
        match msg {
            DetailsMsg::Select(i) => {
                if let Some(d) = &mut self.item.details {
                    d.selected = i.min(d.entries.len().saturating_sub(1));
                }
                let i = self.item.details.as_ref().map_or(0, |d| d.selected);
                return self.load_details(i);
            }
            DetailsMsg::Loaded(i, result) => {
                if let Some(e) = self.item.details.as_mut().and_then(|d| d.entries.get_mut(i)) {
                    match *result {
                        Ok(details) => {
                            e.rows = rows(&e.subject, Some(&details));
                            e.details = Some(details);
                            e.load = Load::Done;
                        }
                        Err(err) => e.load = Load::Failed(err),
                    }
                }
            }
            DetailsMsg::Copy(text) => {
                self.toast_info("Copied.".into());
                return iced::clipboard::write(text);
            }
            DetailsMsg::OpenLink(url) => album_page::open_url(&url),
            DetailsMsg::ShowFile(path) => super::save::show_in_file_manager(&path),
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn value<'a>(rows: &'a [Row], label: &str) -> Option<&'a str> {
        rows.iter().find(|r| r.label == label).map(|r| r.value.as_str())
    }

    fn song() -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.title = "Song".into();
        s.favorite = true;
        s.rating = Some(4);
        s.duration = Duration::from_secs(245);
        s
    }

    #[test]
    fn a_subsonic_song_shows_server_facts_over_the_model() {
        let d = ItemDetails {
            path: Some("A/B/01.flac".into()),
            size: Some(31_457_280),
            suffix: Some("flac".into()),
            bit_rate: Some(912),
            sample_rate: Some(96_000),
            bit_depth: Some(24),
            channels: Some(2),
            bpm: Some(128),
            created: Some("2024-03-01T10:00:00Z".into()),
            last_played: Some("2025-01-02T03:04:05Z".into()),
            play_count: Some(7),
            comment: Some("live take".into()),
            musicbrainz_id: Some("mb-1".into()),
            gain: listen_core::model::GainInfo {
                track_gain: Some(-6.5),
                album_peak: Some(1.0),
                ..Default::default()
            },
            has_lyrics: Some(true),
            participants: vec![("producer".into(), "Pat".into())],
            tags: vec![("ISRC".into(), "USX123".into())],
            ..Default::default()
        };
        let r = rows(&Subject::Song(Box::new(song())), Some(&d));
        assert_eq!(value(&r, "Title"), Some("Song"));
        assert_eq!(value(&r, "Path"), Some("A/B/01.flac"));
        assert_eq!(value(&r, "Codec"), Some("FLAC"));
        assert_eq!(value(&r, "Bitrate"), Some("912 kbps"));
        assert_eq!(value(&r, "Sample rate"), Some("96 kHz"));
        assert_eq!(value(&r, "Bit depth"), Some("24 bit"));
        assert_eq!(value(&r, "Channels"), Some("Stereo"));
        assert_eq!(value(&r, "Size"), Some("30.0 MiB"));
        assert_eq!(value(&r, "BPM"), Some("128"));
        assert_eq!(value(&r, "Duration"), Some("4:05"));
        assert_eq!(value(&r, "Added"), Some("2024-03-01 10:00"));
        assert_eq!(value(&r, "Last played"), Some("2025-01-02 03:04"));
        assert_eq!(value(&r, "Play count"), Some("7"));
        assert_eq!(value(&r, "Track gain"), Some("-6.50 dB"));
        assert_eq!(value(&r, "Album peak"), Some("1.0000"));
        assert_eq!(value(&r, "Lyrics"), Some("Yes"));
        assert_eq!(value(&r, "Comment"), Some("live take"));
        assert_eq!(value(&r, "Producer"), Some("Pat"));
        assert_eq!(value(&r, "ISRC"), Some("USX123"));
        assert_eq!(value(&r, "Favorite"), Some("Yes"));
        assert_eq!(value(&r, "Rating"), Some("★★★★ (4/5)"));
        let mb = r.iter().find(|r| r.label == "MusicBrainz ID").unwrap();
        assert_eq!(mb.link.as_deref(), Some("https://musicbrainz.org/recording/mb-1"));
        assert_eq!(r.iter().find(|r| r.label == "Path").unwrap().kind, RowKind::Path);
        assert_eq!(r.last().map(|r| r.label.as_str()), Some("Id"));
    }

    #[test]
    fn without_server_details_the_model_still_fills_the_modal() {
        let mut s = song();
        s.codec = Some("opus".into());
        s.bit_rate = Some(160);
        s.favorite = false;
        s.rating = None;
        let r = rows(&Subject::Song(Box::new(s)), None);
        assert_eq!(value(&r, "Codec"), Some("OPUS"));
        assert_eq!(value(&r, "Bitrate"), Some("160 kbps"));
        assert_eq!(value(&r, "Favorite"), Some("No"));
        assert!(value(&r, "Rating").is_none() && value(&r, "Path").is_none() && value(&r, "Size").is_none());
    }

    #[test]
    fn a_youtube_music_song_shows_its_link() {
        let mut s = song();
        s.source = SourceKind::YouTubeMusic;
        let d = ItemDetails {
            tags: vec![("YouTube Music link".into(), "https://music.youtube.com/watch?v=x".into())],
            ..Default::default()
        };
        let r = rows(&Subject::Song(Box::new(s)), Some(&d));
        assert_eq!(value(&r, "YouTube Music link"), Some("https://music.youtube.com/watch?v=x"));
    }

    #[test]
    fn albums_artists_and_playlists_have_their_own_rows() {
        let mut a = crate::demo::albums().remove(0);
        a.info.record_labels = vec!["Lab".into()];
        a.info.version = Some("Deluxe".into());
        a.info.explicit = true;
        let d = ItemDetails {
            compilation: Some(false),
            musicbrainz_id: Some("rel-1".into()),
            play_count: Some(3),
            ..Default::default()
        };
        let r = rows(&Subject::Album(Box::new(a)), Some(&d));
        assert_eq!(value(&r, "Record label"), Some("Lab"));
        assert_eq!(value(&r, "Version"), Some("Deluxe"));
        assert_eq!(value(&r, "Explicit"), Some("Explicit"));
        assert_eq!(value(&r, "Compilation"), Some("No"));
        assert_eq!(value(&r, "MusicBrainz ID"), Some("rel-1"));

        let artist = Artist {
            id: "ar1".into(),
            server_id: listen_core::model::ServerId("s".into()),
            source: SourceKind::Subsonic,
            name: "Artist".into(),
            image: None,
            album_count: Some(2),
            song_count: Some(20),
            favorite: false,
            rating: Some(5),
            biography: Some("<p>Hello &amp; <a href=\"x\">bye</a></p>".into()),
            similar: vec![],
        };
        let d = ItemDetails { musicbrainz_id: Some("ar-mb".into()), ..Default::default() };
        let r = rows(&Subject::Artist(Box::new(artist)), Some(&d));
        assert_eq!(value(&r, "Biography"), Some("Hello & bye"));
        assert_eq!(value(&r, "Songs"), Some("20"));
        assert_eq!(value(&r, "MusicBrainz ID"), Some("ar-mb"));

        let p = Playlist {
            id: "p1".into(),
            server_id: listen_core::model::ServerId("s".into()),
            source: SourceKind::Subsonic,
            name: "Mix".into(),
            description: Some("my mix".into()),
            owner: Some("alex".into()),
            image: None,
            song_count: Some(12),
            duration: Duration::from_secs(3600),
            public: true,
            editable: true,
        };
        let r = rows(&Subject::Playlist(Box::new(p)), None);
        assert_eq!(value(&r, "Description"), Some("my mix"));
        assert_eq!(value(&r, "Owner"), Some("alex"));
        assert_eq!(value(&r, "Public"), Some("Yes"));
        assert_eq!(value(&r, "Duration"), Some("1:00:00"));
    }

    #[test]
    fn formats_sizes_and_dates() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(1536), "1.5 KiB");
        assert_eq!(date_text("2024-03-01T10:00:00Z"), "2024-03-01 10:00");
        assert_eq!(date_text("yesterday"), "yesterday");
        assert_eq!(khz(44_100), "44.1 kHz");
    }
}
