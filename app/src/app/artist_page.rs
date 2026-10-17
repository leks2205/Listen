//! Artist page state, messages and actions: info, "appears on", favourites, videos,
//! release-type groups and biography links (as in Feishin's `features/artists`).
//! View: `ui/artist_detail.rs`; list pages: `ui/artists.rs`.

use super::menu::{MenuAction, MenuMsg};
use super::*;
use listen_core::settings::{ArtistSettings, PlayBehavior, RELEASE_TYPES};
use listen_source::{ArtistInfo, Capabilities, ItemKind};
use std::borrow::Cow;
use std::collections::HashSet;

/// Top songs: what the server says, or what you played most (the local history).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopMode {
    #[default]
    Community,
    Personal,
}

/// Favourite songs: the starred ones, or the ones you rated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FavMode {
    #[default]
    Favorite,
    Rating,
}

/// How releases are grouped: by their main type, or by every type they have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grouping {
    #[default]
    Primary,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlbumSortBy {
    #[default]
    Year,
    Name,
    SongCount,
    Duration,
    PlayCount,
    Rating,
}

impl AlbumSortBy {
    pub const ALL: [AlbumSortBy; 6] = [
        AlbumSortBy::Year,
        AlbumSortBy::Name,
        AlbumSortBy::SongCount,
        AlbumSortBy::Duration,
        AlbumSortBy::PlayCount,
        AlbumSortBy::Rating,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AlbumSortBy::Year => "Release date",
            AlbumSortBy::Name => "Name",
            AlbumSortBy::SongCount => "Song count",
            AlbumSortBy::Duration => "Duration",
            AlbumSortBy::PlayCount => "Play count",
            AlbumSortBy::Rating => "Rating",
        }
    }
}

/// A song section of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Top,
    Favorite,
    Videos,
}

/// The pages under an artist (Feishin: discography, all tracks, "View all"
/// of the top songs and of the favourite songs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtistView {
    Discography,
    Tracks,
    TopSongs,
    FavoriteSongs,
}

impl ArtistView {
    pub fn title(self) -> &'static str {
        match self {
            ArtistView::Discography => "Discography",
            ArtistView::Tracks => "All tracks",
            ArtistView::TopSongs => "Top songs",
            ArtistView::FavoriteSongs => "Favorite songs",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ArtistMsg {
    Info(u64, Result<ArtistInfo, String>),
    AppearsOn(u64, Result<Vec<Album>, String>),
    Videos(u64, Result<Vec<Song>, String>),
    Favorites(u64, Result<Vec<Song>, String>),
    Rated(u64, Result<Vec<Song>, String>),
    AllSongs(u64, Result<Vec<Song>, String>),
    /// Header: every song of the artist, with the Play-button behaviour.
    Play(PlayBehavior),
    Shuffle,
    Radio,
    ToggleFavorite,
    Rate(u8),
    /// A favourite / rating change failed: undo it.
    Undo(Box<Artist>, String),
    More,
    Refresh,
    CopyName,
    ToggleBio,
    TopMode(TopMode),
    FavMode(FavMode),
    TopSearch(String),
    FavSearch(String),
    /// A row of a song section was clicked: play the section from there.
    PlayAt(Section, usize),
    /// Play now / next / last for a whole song section.
    PlaySection(Section, PlayBehavior),
    AlbumSearch(String),
    AlbumSort(AlbumSortBy),
    AlbumOrder,
    Grouping(Grouping),
    GroupMenu,
    /// Play now / next / last for the albums of a release-type group.
    PlayGroup(String, PlayBehavior),
    ShowAll(String),
    SimilarPage(isize),
    Link(String),
}

/// What the artist page shows besides `library.artist`.
#[derive(Debug, Default)]
pub struct ArtistPage {
    /// The artist the data below belongs to.
    pub id: String,
    /// Bumped per load; answers of an older one are dropped.
    pub rev: u64,
    pub info: Option<ArtistInfo>,
    pub appears_on: Vec<Album>,
    pub videos: Vec<Song>,
    /// `None` while loading.
    pub favorites: Option<Vec<Song>>,
    pub rated: Option<Vec<Song>>,
    pub all_songs: Option<Vec<Song>>,
    /// Your most played songs by the artist, and how often you played each.
    pub personal: Vec<Song>,
    pub personal_plays: Vec<u32>,
    // Derived by `refresh_views` after every change:
    /// The top songs of the chosen mode / the same with the search applied.
    pub top_all: Vec<Song>,
    pub top_view: Vec<Song>,
    pub fav_all: Vec<Song>,
    pub fav_view: Vec<Song>,
    /// The artist's albums and the ones it appears on, in the chosen order.
    pub discography: Vec<Album>,
    pub bio_open: bool,
    pub more_open: bool,
    pub group_menu: bool,
    pub album_search: String,
    pub top_search: String,
    pub fav_search: String,
    /// Release-type groups that show all their albums.
    pub show_all: HashSet<String>,
    pub similar_page: usize,
    // Kept across artists, like Feishin's local storage:
    pub top_mode: TopMode,
    pub fav_mode: FavMode,
    pub grouping: Grouping,
    pub sort: AlbumSortBy,
    pub ascending: bool,
}

impl ArtistPage {
    /// Another artist is opened: drop its data, keep the preferences.
    fn reset(&mut self, id: String) {
        *self = ArtistPage {
            id,
            rev: self.rev,
            top_mode: self.top_mode,
            fav_mode: self.fav_mode,
            grouping: self.grouping,
            sort: self.sort,
            ascending: self.ascending,
            ..Default::default()
        };
    }
}

/// The sites of the external-links settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSite {
    All,
    LastFm,
    MusicBrainz,
    ListenBrainz,
    Qobuz,
    Spotify,
    NativeSpotify,
}

/// A change on the settings page (General → Artist page, External links).
#[derive(Debug, Clone, Copy)]
pub enum ArtistSetting {
    /// Move a page section up (-1) or down (1).
    MoveSection(usize, i8),
    SectionEnabled(usize, bool),
    MoveRelease(usize, i8),
    ReleaseEnabled(usize, bool),
    RadioCount(u32),
    Link(LinkSite, bool),
}

fn swap<T>(v: &mut [T], i: usize, dir: i8) {
    if let Some(j) = i.checked_add_signed(isize::from(dir)).filter(|j| *j < v.len())
        && i < v.len()
    {
        v.swap(i, j);
    }
}

impl ArtistSetting {
    pub fn apply(self, s: &mut ArtistSettings) {
        match self {
            ArtistSetting::MoveSection(i, d) => swap(&mut s.items, i, d),
            ArtistSetting::SectionEnabled(i, v) => {
                if let Some(item) = s.items.get_mut(i) {
                    item.enabled = v;
                }
            }
            ArtistSetting::MoveRelease(i, d) => swap(&mut s.release_types, i, d),
            ArtistSetting::ReleaseEnabled(i, v) => {
                if let Some(item) = s.release_types.get_mut(i) {
                    item.enabled = v;
                }
            }
            ArtistSetting::RadioCount(n) => s.radio_count = n.clamp(1, 100),
            ArtistSetting::Link(site, v) => {
                let l = &mut s.external_links;
                match site {
                    LinkSite::All => l.enabled = v,
                    LinkSite::LastFm => l.last_fm = v,
                    LinkSite::MusicBrainz => l.music_brainz = v,
                    LinkSite::ListenBrainz => l.listen_brainz = v,
                    LinkSite::Qobuz => l.qobuz = v,
                    LinkSite::Spotify => l.spotify = v,
                    LinkSite::NativeSpotify => l.native_spotify = v,
                }
            }
        }
    }
}

/// Songs shown in a section: the search applied.
pub fn matching(songs: &[Song], query: &str) -> Vec<Song> {
    let q = query.trim().to_lowercase();
    songs
        .iter()
        .filter(|s| {
            q.is_empty()
                || s.title.to_lowercase().contains(&q)
                || s.album.as_deref().is_some_and(|a| a.to_lowercase().contains(&q))
                || s.artist_line().to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

// ---- release-type groups ---------------------------------------------------

const PRIMARY: [&str; 5] = ["album", "broadcast", "ep", "other", "single"];
/// Joins the types of a combined group (`mixtape/street` has a slash itself).
const JOIN: char = '\u{1f}';

/// A discography section.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// `album`, `appears-on`, or several types joined for [`Grouping::All`].
    pub key: String,
    pub title: String,
    pub albums: Vec<Album>,
}

fn release_types(album: &Album) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // All the types the server sent; sources with a single string use `release_type`.
    let single = album.release_type.iter().filter(|_| album.release_types.is_empty());
    let all = album.release_types.iter().chain(single);
    for t in all.flat_map(|t| t.split([';', ',', '\u{1f}'])) {
        let t = t.trim().to_lowercase();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

pub fn sort_albums(albums: &mut [Album], by: AlbumSortBy, ascending: bool) {
    albums.sort_by(|a, b| {
        let o = match by {
            AlbumSortBy::Year => a.year.unwrap_or(0).cmp(&b.year.unwrap_or(0)),
            AlbumSortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            AlbumSortBy::SongCount => a.song_count.unwrap_or(0).cmp(&b.song_count.unwrap_or(0)),
            AlbumSortBy::Duration => a.duration.cmp(&b.duration),
            AlbumSortBy::PlayCount => a.play_count.unwrap_or(0).cmp(&b.play_count.unwrap_or(0)),
            AlbumSortBy::Rating => a.rating.unwrap_or(0).cmp(&b.rating.unwrap_or(0)),
        };
        if ascending { o } else { o.reverse() }
    });
}

fn display_name(key: &str) -> String {
    if let Some((_, _, label)) = RELEASE_TYPES.iter().find(|(_, k, _)| *k == key) {
        return (*label).to_string();
    }
    let mut c = key.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// Feishin's `groupAlbumsByReleaseType` + `getArtistAlbumsGrouped`: the groups
/// in the configured priority, hidden ones left out. `own` are the artist's
/// albums, `appears` the ones they only guest on.
pub fn group_albums(
    own: &[Album],
    appears: &[Album],
    artist_id: &str,
    grouping: Grouping,
    settings: &ArtistSettings,
) -> Vec<Group> {
    let mut map: Vec<(String, Vec<Album>)> = Vec::new();
    let mut push = |key: String, album: &Album| match map.iter_mut().find(|(k, _)| *k == key) {
        Some((_, v)) => v.push(album.clone()),
        None => map.push((key, vec![album.clone()])),
    };
    for album in own.iter().chain(appears) {
        let ids_known = album.album_artists.iter().any(|a| a.id.is_some());
        let is_album_artist =
            !ids_known || album.album_artists.iter().any(|a| a.id.as_deref() == Some(artist_id));
        if !is_album_artist {
            push("appears-on".into(), album);
            continue;
        }
        let types = release_types(album);
        let key = match grouping {
            Grouping::All => {
                if types.iter().any(|t| t == "compilation") {
                    "compilation".to_string()
                } else if types.is_empty() {
                    "album".to_string()
                } else {
                    let mut primary: Vec<&String> =
                        types.iter().filter(|t| PRIMARY.contains(&t.as_str())).collect();
                    let mut secondary: Vec<&String> =
                        types.iter().filter(|t| !PRIMARY.contains(&t.as_str())).collect();
                    primary.sort();
                    secondary.sort();
                    primary.into_iter().chain(secondary).cloned().collect::<Vec<_>>().join(&JOIN.to_string())
                }
            }
            Grouping::Primary => ["album", "single", "ep", "broadcast", "other"]
                .iter()
                .find(|p| types.iter().any(|t| t == *p))
                .map(|p| (*p).to_string())
                .or_else(|| types.first().cloned())
                .unwrap_or_else(|| "album".into()),
        };
        push(key, album);
    }

    let priority = settings.release_priority();
    let prio = |t: &str| priority.iter().position(|k| *k == t).unwrap_or(999);
    let get_priority = |key: &str| -> usize {
        if key.contains(JOIN) {
            let types: Vec<&str> = key.split(JOIN).collect();
            if let Some(p) = types.iter().find(|t| PRIMARY.contains(t)) {
                return prio(p);
            }
            types.iter().map(|t| prio(t)).filter(|p| *p != 999).min().unwrap_or(999)
        } else {
            prio(key)
        }
    };
    let secondary_key = |key: &str| -> String {
        let mut p: Vec<usize> =
            key.split(JOIN).filter(|t| !PRIMARY.contains(t)).map(prio).filter(|p| *p != 999).collect();
        p.sort_unstable();
        p.iter().map(|p| format!("{p:03}")).collect::<Vec<_>>().join(",")
    };
    let enabled = |key: &str| -> bool {
        if key.contains(JOIN) {
            key.split(JOIN).any(|t| !settings.release_type_hidden(t))
        } else {
            !settings.release_type_hidden(key)
        }
    };
    let mut groups: Vec<Group> = map
        .into_iter()
        .filter(|(k, _)| enabled(k))
        .map(|(key, albums)| Group {
            title: key.split(JOIN).map(display_name).collect::<Vec<_>>().join(" · "),
            key,
            albums,
        })
        .collect();
    groups.sort_by(|a, b| {
        get_priority(&a.key).cmp(&get_priority(&b.key)).then_with(|| {
            let (ca, cb) = (a.key.contains(JOIN), b.key.contains(JOIN));
            if ca && cb {
                let (sa, sb) = (secondary_key(&a.key), secondary_key(&b.key));
                if !sa.is_empty() && !sb.is_empty() {
                    return sa.cmp(&sb);
                }
            }
            a.key.to_lowercase().cmp(&b.key.to_lowercase())
        })
    });
    groups
}

// ---- biography -------------------------------------------------------------

/// A piece of the biography: plain text, or a link.
#[derive(Debug, Clone, PartialEq)]
pub struct BioSpan {
    pub text: String,
    pub link: Option<String>,
}

fn decode(entity: &str) -> Option<char> {
    Some(match entity {
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        "lt" => '<',
        "gt" => '>',
        "nbsp" => ' ',
        e => {
            let n = e.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(code)?
        }
    })
}

fn href(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let at = lower.find("href=")? + 5;
    let rest = &tag[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let end = rest[1..].find(quote)?;
    let url = rest[1..1 + end].trim();
    (url.starts_with("http://") || url.starts_with("https://")).then(|| url.replace("&amp;", "&"))
}

/// Last.fm / Jellyfin biographies are HTML: text with links, line breaks and
/// the odd tag. Returns the text with its links; every other tag is dropped.
pub fn parse_bio(html: &str) -> Vec<BioSpan> {
    let mut spans: Vec<BioSpan> = Vec::new();
    let mut link: Option<String> = None;
    let mut push = |text: &str, link: &Option<String>| {
        if text.is_empty() {
            return;
        }
        match spans.last_mut() {
            Some(last) if last.link == *link => last.text.push_str(text),
            _ => spans.push(BioSpan { text: text.to_string(), link: link.clone() }),
        }
    };
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let Some(end) = stripped.find('>') else {
                push("<", &link);
                rest = stripped;
                continue;
            };
            let tag = &stripped[..end];
            let name = tag
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            match name.to_lowercase().as_str() {
                "a" if tag.starts_with('/') => link = None,
                "a" => link = href(tag),
                "br" => push("\n", &link),
                "p" | "div" if tag.starts_with('/') => push("\n\n", &link),
                _ => {}
            }
            rest = &stripped[end + 1..];
        } else if let Some(stripped) = rest.strip_prefix('&') {
            match stripped.find(';').filter(|e| *e <= 8).and_then(|e| decode(&stripped[..e]).map(|c| (c, e)))
            {
                Some((c, e)) => {
                    push(&c.to_string(), &link);
                    rest = &stripped[e + 1..];
                }
                None => {
                    push("&", &link);
                    rest = stripped;
                }
            }
        } else {
            let end = rest.find(['<', '&']).unwrap_or(rest.len());
            push(&rest[..end], &link);
            rest = &rest[end..];
        }
    }
    // Trim the ends and collapse runs of blank lines.
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    for s in &mut spans {
        while s.text.contains("\n\n\n") {
            s.text = s.text.replace("\n\n\n", "\n\n");
        }
    }
    spans.retain(|s| !s.text.is_empty());
    spans
}

/// The biography as plain text.
pub fn bio_text(spans: &[BioSpan]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// "3 hr 20 min" / "45 min".
pub fn total_time(d: Duration) -> String {
    let mins = d.as_secs() / 60;
    match (mins / 60, mins % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// A link to the artist on another site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteLink {
    pub name: &'static str,
    pub url: String,
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// The links the settings allow (Feishin `AlbumArtistMetadataExternalLinks`):
/// MusicBrainz only with an id, the rest search by name.
pub fn site_links(
    settings: &ArtistSettings,
    name: &str,
    mbid: Option<&str>,
    lastfm: Option<&str>,
) -> Vec<SiteLink> {
    let l = &settings.external_links;
    let mut out = Vec::new();
    if !l.enabled || name.is_empty() {
        return out;
    }
    if l.last_fm {
        let url = lastfm
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://www.last.fm/music/{}", encode(name)));
        out.push(SiteLink { name: "Last.fm", url });
    }
    if let (true, Some(id)) = (l.music_brainz, mbid) {
        out.push(SiteLink { name: "MusicBrainz", url: format!("https://musicbrainz.org/artist/{id}") });
    }
    if l.listen_brainz {
        let url = match mbid {
            Some(id) => format!("https://listenbrainz.org/artist/{id}"),
            None => format!("https://listenbrainz.org/search/?search_term={}", encode(name)),
        };
        out.push(SiteLink { name: "ListenBrainz", url });
    }
    if l.qobuz {
        out.push(SiteLink {
            name: "Qobuz",
            url: format!("https://www.qobuz.com/us-en/search/artists/{}", encode(name)),
        });
    }
    if l.spotify {
        let url = if l.native_spotify {
            format!("spotify:search:{}", encode(name))
        } else {
            format!("https://open.spotify.com/search/{}", encode(name))
        };
        out.push(SiteLink { name: "Spotify", url });
    }
    out
}

fn open_url(url: &str) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("spotify:")) {
        return;
    }
    if let Err(e) = std::process::Command::new(program).arg(url).spawn() {
        tracing::warn!("opening {url}: {e}");
    }
}

fn action_for(b: PlayBehavior) -> MenuAction {
    match b {
        PlayBehavior::Now => MenuAction::Play,
        PlayBehavior::Next => MenuAction::PlayNext,
        PlayBehavior::Last => MenuAction::AddToQueue,
    }
}

impl Listen {
    /// What the connected source can do for artists (everything in the demo).
    pub fn artist_caps(&self) -> Capabilities {
        if self.demo {
            return Capabilities::all();
        }
        self.source.as_ref().map(|s| s.capabilities()).unwrap_or_else(Capabilities::empty)
    }

    /// Sections the page shows for this source.
    pub fn artist_sections(&self) -> Vec<listen_core::settings::ArtistSection> {
        self.config.settings.artist.sections()
    }

    /// The songs of a section, search applied.
    pub fn artist_section_songs(&self, section: Section) -> &[Song] {
        let page = &self.artist_page;
        match section {
            Section::Top => &page.top_view,
            Section::Favorite => &page.fav_view,
            Section::Videos => &page.videos,
        }
    }

    /// Recomputes what the page derives from its data and settings.
    pub(super) fn refresh_views(&mut self) {
        let Some(detail) = &self.library.artist else { return };
        let p = &mut self.artist_page;
        p.top_all = match p.top_mode {
            TopMode::Community => detail.top_songs.clone(),
            TopMode::Personal => p.personal.clone(),
        };
        p.top_view = matching(&p.top_all, &p.top_search);
        p.fav_all = match p.fav_mode {
            FavMode::Favorite => p.favorites.clone().unwrap_or_default(),
            FavMode::Rating => {
                let mut v = p.rated.clone().unwrap_or_default();
                v.sort_by_key(|s| std::cmp::Reverse(s.rating.unwrap_or(0)));
                v
            }
        };
        p.fav_view = matching(&p.fav_all, &p.fav_search);
        let mut all = detail.albums.clone();
        all.extend(p.appears_on.iter().cloned());
        sort_albums(&mut all, p.sort, p.ascending);
        p.discography = all;
    }

    /// The rows of the artist's sub-pages (`ListId::Artist`, `ListId::ArtistAlbums`).
    pub fn artist_list_data(&self, id: &list::ListId) -> Option<list::ListData<'_>> {
        let page = &self.artist_page;
        match id {
            list::ListId::ArtistAlbums(_) => Some(list::ListData::Albums(Cow::Borrowed(&page.discography))),
            list::ListId::Artist(_) => {
                let view = match &self.route {
                    Route::ArtistList(_, v) => *v,
                    _ => ArtistView::TopSongs,
                };
                Some(list::ListData::Songs(Cow::Borrowed(match view {
                    ArtistView::Tracks => page.all_songs.as_deref().unwrap_or_default(),
                    ArtistView::FavoriteSongs => &page.fav_all,
                    _ => &page.top_all,
                })))
            }
            _ => None,
        }
    }

    /// The artist arrived: load what the page shows besides it.
    pub(super) fn artist_begin(&mut self, artist: &Artist) -> Task<Message> {
        if self.artist_page.id != artist.id {
            self.artist_page.reset(artist.id.clone());
        }
        self.artist_page.rev += 1;
        let rev = self.artist_page.rev;
        self.artist_personal(artist);
        self.refresh_views();
        let Some(source) = self.source.clone() else { return Task::none() };
        let caps = source.capabilities();
        let sections = self.artist_sections();
        let mut tasks: Vec<Task<Message>> = Vec::new();
        if caps.contains(Capabilities::ARTIST_INFO) {
            let (s, id) = (source.clone(), artist.id.clone());
            tasks.push(Task::perform(
                async move { s.artist_info(&id).await.map_err(err) },
                self.scoped(move |r| Message::Artist(ArtistMsg::Info(rev, r))),
            ));
        }
        let (s, a) = (source.clone(), artist.clone());
        tasks.push(Task::perform(
            async move { s.appears_on(&a).await.map_err(err) },
            self.scoped(move |r| Message::Artist(ArtistMsg::AppearsOn(rev, r))),
        ));
        let (s, a) = (source.clone(), artist.clone());
        tasks.push(Task::perform(
            async move { s.artist_videos(&a).await.map_err(err) },
            self.scoped(move |r| Message::Artist(ArtistMsg::Videos(rev, r))),
        ));
        let on_tracks_page = matches!(self.route, Route::ArtistList(_, ArtistView::Tracks));
        if sections.contains(&listen_core::settings::ArtistSection::FavoriteSongs)
            || matches!(self.route, Route::ArtistList(_, ArtistView::FavoriteSongs))
        {
            tasks.push(self.artist_load_favorites(&source, artist, rev));
        }
        if on_tracks_page || self.artist_page.fav_mode == FavMode::Rating {
            tasks.push(self.artist_load_all(&source, artist, rev));
        }
        Task::batch(tasks)
    }

    fn artist_load_favorites(
        &mut self,
        source: &Arc<dyn listen_source::MusicSource>,
        artist: &Artist,
        rev: u64,
    ) -> Task<Message> {
        let (s, a) = (source.clone(), artist.clone());
        Task::perform(
            async move {
                let mut out: Vec<Song> = Vec::new();
                for page in 0..10 {
                    let q = listen_source::SongQuery {
                        favorites_only: true,
                        start: page * 500,
                        limit: 500,
                        ..Default::default()
                    };
                    let got = s.songs(&q).await.map_err(err)?;
                    let n = got.items.len();
                    out.extend(got.items.into_iter().filter(|x| listen_source::is_by(x, &a)));
                    if n < 500 {
                        break;
                    }
                }
                Ok(out)
            },
            self.scoped(move |r| Message::Artist(ArtistMsg::Favorites(rev, r))),
        )
    }

    fn artist_load_all(
        &mut self,
        source: &Arc<dyn listen_source::MusicSource>,
        artist: &Artist,
        rev: u64,
    ) -> Task<Message> {
        let (s, a) = (source.clone(), artist.clone());
        Task::perform(
            async move { s.artist_songs(&a).await.map_err(err) },
            self.scoped(move |r| Message::Artist(ArtistMsg::AllSongs(rev, r))),
        )
    }

    fn artist_personal(&mut self, artist: &Artist) {
        let top = listen_core::history::artist_top_songs(
            &self.history.entries,
            &artist.server_id,
            &artist.id,
            &artist.name,
            20,
        );
        self.artist_page.personal_plays = top.iter().map(|(_, n)| *n).collect();
        self.artist_page.personal = top.into_iter().map(|(s, _)| s).collect();
    }

    /// The loaded artist and its source.
    fn open_artist_source(&self) -> Option<(Artist, Arc<dyn listen_source::MusicSource>)> {
        let artist = self.library.artist.as_ref()?.artist.clone();
        Some((artist, self.source.clone()?))
    }

    /// Plays or queues every song of the artist (fetched).
    fn artist_play_all(&mut self, action: MenuAction) -> Task<Message> {
        let Some((artist, source)) = self.open_artist_source() else { return Task::none() };
        if let Some(all) = self.artist_page.all_songs.clone().filter(|a| !a.is_empty()) {
            return self.apply_to_songs(action, all);
        }
        if self.demo {
            let songs = self.library.artist.as_ref().map(|d| d.top_songs.clone()).unwrap_or_default();
            return self.apply_to_songs(action, songs);
        }
        Task::perform(async move { source.artist_songs(&artist).await.map_err(err) }, move |r| {
            Message::Menu(MenuMsg::Songs(action.clone(), r))
        })
    }

    pub(super) fn on_artist(&mut self, msg: ArtistMsg) -> Task<Message> {
        let task = self.on_artist_msg(msg);
        self.refresh_views();
        task
    }

    fn on_artist_msg(&mut self, msg: ArtistMsg) -> Task<Message> {
        let current = |page: &ArtistPage, rev: u64| page.rev == rev;
        match msg {
            ArtistMsg::Info(rev, r) if current(&self.artist_page, rev) => match r {
                Ok(info) => {
                    let refs: Vec<ImageRef> =
                        info.similar_artists.iter().filter_map(|a| a.image.clone()).collect();
                    self.artist_page.info = Some(info);
                    return self.request_images(refs.iter().map(Some));
                }
                Err(e) => tracing::debug!("artist info: {e}"),
            },
            ArtistMsg::AppearsOn(rev, r) if current(&self.artist_page, rev) => match r {
                Ok(albums) => {
                    let refs: Vec<ImageRef> = albums.iter().filter_map(|a| a.image.clone()).collect();
                    self.artist_page.appears_on = albums;
                    return self.request_images(refs.iter().map(Some));
                }
                Err(e) => tracing::debug!("appears on: {e}"),
            },
            ArtistMsg::Videos(rev, r) if current(&self.artist_page, rev) => {
                if let Ok(v) = r {
                    let refs: Vec<ImageRef> = v.iter().filter_map(|s| s.image.clone()).collect();
                    self.artist_page.videos = v;
                    return self.request_images(refs.iter().map(Some));
                }
            }
            ArtistMsg::Favorites(rev, r) if current(&self.artist_page, rev) => {
                let songs = r.unwrap_or_default();
                let refs: Vec<ImageRef> = songs.iter().filter_map(|s| s.image.clone()).collect();
                self.artist_page.favorites = Some(songs);
                return self.request_images(refs.iter().map(Some));
            }
            ArtistMsg::Rated(rev, r) if current(&self.artist_page, rev) => {
                self.artist_page.rated = Some(r.unwrap_or_default());
            }
            ArtistMsg::AllSongs(rev, r) if current(&self.artist_page, rev) => match r {
                Ok(songs) => {
                    let rated: Vec<Song> = songs.iter().filter(|s| s.rating.is_some()).cloned().collect();
                    let refs: Vec<ImageRef> = songs.iter().take(60).filter_map(|s| s.image.clone()).collect();
                    self.artist_page.rated = Some(rated);
                    self.artist_page.all_songs = Some(songs);
                    return self.request_images(refs.iter().map(Some));
                }
                Err(e) => {
                    self.artist_page.all_songs = Some(Vec::new());
                    self.artist_page.rated = Some(Vec::new());
                    self.toast_error(format!("Could not load the artist's songs: {e}"));
                }
            },
            ArtistMsg::Info(..)
            | ArtistMsg::AppearsOn(..)
            | ArtistMsg::Videos(..)
            | ArtistMsg::Favorites(..)
            | ArtistMsg::Rated(..)
            | ArtistMsg::AllSongs(..) => {}
            ArtistMsg::Play(b) => return self.artist_play_all(action_for(b)),
            ArtistMsg::Shuffle => return self.artist_play_all(MenuAction::Shuffle),
            ArtistMsg::Radio => {
                let Some((artist, source)) = self.open_artist_source() else { return Task::none() };
                if self.demo {
                    let songs = self.library.artist.as_ref().map(|d| d.top_songs.clone()).unwrap_or_default();
                    return self.apply_to_songs(MenuAction::Play, songs);
                }
                let count = self.config.settings.artist.radio_count.max(1) as usize;
                return Task::perform(
                    async move { source.artist_radio(&artist, count).await.map_err(err) },
                    |r| Message::Menu(MenuMsg::Songs(MenuAction::Play, r)),
                );
            }
            ArtistMsg::ToggleFavorite => return self.artist_set_favorite(),
            ArtistMsg::Rate(stars) => return self.artist_set_rating(stars),
            ArtistMsg::Undo(before, why) => {
                self.set_artist_local(&before);
                self.toast_error(why);
            }
            ArtistMsg::More => self.artist_page.more_open = !self.artist_page.more_open,
            ArtistMsg::Refresh => {
                self.artist_page.more_open = false;
                if let Some(id) = self.library.artist.as_ref().map(|d| d.artist.id.clone()) {
                    return self.open_artist(id);
                }
            }
            ArtistMsg::CopyName => {
                self.artist_page.more_open = false;
                if let Some(d) = &self.library.artist {
                    return iced::clipboard::write(d.artist.name.clone());
                }
            }
            ArtistMsg::ToggleBio => self.artist_page.bio_open = !self.artist_page.bio_open,
            ArtistMsg::TopMode(m) => self.artist_page.top_mode = m,
            ArtistMsg::FavMode(m) => {
                self.artist_page.fav_mode = m;
                if m == FavMode::Rating && self.artist_page.rated.is_none() {
                    let Some((artist, source)) = self.open_artist_source() else { return Task::none() };
                    let rev = self.artist_page.rev;
                    return self.artist_load_all(&source, &artist, rev);
                }
            }
            ArtistMsg::TopSearch(q) => self.artist_page.top_search = q,
            ArtistMsg::FavSearch(q) => self.artist_page.fav_search = q,
            ArtistMsg::PlayAt(section, i) => {
                let songs = self.artist_section_songs(section).to_vec();
                if i < songs.len() {
                    return self.play_picked(songs, i);
                }
            }
            ArtistMsg::PlaySection(section, b) => {
                let songs = self.artist_section_songs(section).to_vec();
                return self.apply_to_songs(action_for(b), songs);
            }
            ArtistMsg::AlbumSearch(q) => self.artist_page.album_search = q,
            ArtistMsg::AlbumSort(s) => self.artist_page.sort = s,
            ArtistMsg::AlbumOrder => self.artist_page.ascending = !self.artist_page.ascending,
            ArtistMsg::Grouping(g) => {
                self.artist_page.grouping = g;
                self.artist_page.group_menu = false;
            }
            ArtistMsg::GroupMenu => self.artist_page.group_menu = !self.artist_page.group_menu,
            ArtistMsg::PlayGroup(key, b) => {
                let groups = self.artist_groups();
                if let Some(g) = groups.into_iter().find(|g| g.key == key) {
                    let picks = g.albums.into_iter().map(list::Picked::Album).collect();
                    return self.play_items(picks, b);
                }
            }
            ArtistMsg::ShowAll(key) => {
                self.artist_page.show_all.insert(key);
            }
            ArtistMsg::SimilarPage(d) => {
                let p = &mut self.artist_page.similar_page;
                *p = p.saturating_add_signed(d);
            }
            ArtistMsg::Link(url) => open_url(&url),
        }
        Task::none()
    }

    /// The discography's groups with the page's search, sort and grouping applied.
    pub fn artist_groups(&self) -> Vec<Group> {
        let Some(detail) = &self.library.artist else { return Vec::new() };
        let page = &self.artist_page;
        let q = page.album_search.trim().to_lowercase();
        let pick = |albums: &[Album]| -> Vec<Album> {
            let mut v: Vec<Album> = albums
                .iter()
                .filter(|a| {
                    q.is_empty()
                        || a.name.to_lowercase().contains(&q)
                        || a.artist_line().to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            sort_albums(&mut v, page.sort, page.ascending);
            v
        };
        group_albums(
            &pick(&detail.albums),
            &pick(&page.appears_on),
            &detail.artist.id,
            page.grouping,
            &self.config.settings.artist,
        )
    }

    /// Updates the artist everywhere it is held (page, artist lists).
    fn set_artist_local(&mut self, artist: &Artist) {
        if let Some(d) = self.library.artist.as_mut().filter(|d| d.artist.id == artist.id) {
            d.artist.favorite = artist.favorite;
            d.artist.rating = artist.rating;
        }
        for a in self.library.artists.iter_mut().filter(|a| a.id == artist.id) {
            a.favorite = artist.favorite;
            a.rating = artist.rating;
        }
    }

    fn artist_set_favorite(&mut self) -> Task<Message> {
        let Some(before) = self.library.artist.as_ref().map(|d| d.artist.clone()) else {
            return Task::none();
        };
        let mut after = before.clone();
        after.favorite = !before.favorite;
        if self.demo {
            self.set_artist_local(&after);
            return Task::none();
        }
        let Some(source) =
            self.source.clone().filter(|s| s.capabilities().contains(Capabilities::FAVORITE_ITEMS))
        else {
            return Task::none();
        };
        self.set_artist_local(&after);
        let (id, favorite) = (after.id.clone(), after.favorite);
        let ytm = before.source == SourceKind::YouTubeMusic;
        let done = if ytm {
            if favorite { "Subscribed." } else { "Unsubscribed." }
        } else if favorite {
            "Added to favorites."
        } else {
            "Removed from favorites."
        };
        Task::perform(
            async move {
                source
                    .set_favorite_items(ItemKind::Artist, &[id], favorite)
                    .await
                    .map(|_| done.to_string())
                    .map_err(|e| format!("Could not change the favorite: {e}"))
            },
            move |r| match r {
                Ok(m) => Message::Menu(MenuMsg::Done(Ok(m))),
                Err(e) => Message::Artist(ArtistMsg::Undo(Box::new(before.clone()), e)),
            },
        )
    }

    fn artist_set_rating(&mut self, stars: u8) -> Task<Message> {
        let Some(before) = self.library.artist.as_ref().map(|d| d.artist.clone()) else {
            return Task::none();
        };
        let stars = if before.rating == Some(stars) { 0 } else { stars.min(5) };
        let mut after = before.clone();
        after.rating = (stars > 0).then_some(stars);
        if self.demo {
            self.set_artist_local(&after);
            return Task::none();
        }
        let Some(source) = self.source.clone().filter(|s| s.capabilities().contains(Capabilities::RATINGS))
        else {
            return Task::none();
        };
        self.set_artist_local(&after);
        let id = after.id.clone();
        Task::perform(
            async move {
                source
                    .set_item_rating(ItemKind::Artist, &id, stars)
                    .await
                    .map(|_| {
                        if stars == 0 { "Rating cleared.".to_string() } else { format!("Rated {stars} / 5.") }
                    })
                    .map_err(|e| format!("Could not rate the artist: {e}"))
            },
            move |r| match r {
                Ok(m) => Message::Menu(MenuMsg::Done(Ok(m))),
                Err(e) => Message::Artist(ArtistMsg::Undo(Box::new(before.clone()), e)),
            },
        )
    }

    /// Covers for the visible cards / rows of the artist list pages.
    pub(super) fn artists_visible(&mut self, id: &list::ListId) -> Task<Message> {
        let total = self.library.artists.len();
        let (first, last) = self.list_range(id, total);
        let refs: Vec<ImageRef> = self.library.artists[first.min(total)..last.min(total)]
            .iter()
            .filter_map(|a| a.image.clone())
            .collect();
        self.request_images(refs.iter().map(Some))
    }

    /// Covers for the visible rows of an artist sub-page.
    pub(super) fn artist_sub_visible(&mut self, id: &list::ListId) -> Task<Message> {
        let refs: Vec<ImageRef> = match self.artist_list_data(id) {
            Some(list::ListData::Songs(s)) => {
                let (a, b) = self.list_range(id, s.len());
                s[a.min(s.len())..b.min(s.len())].iter().filter_map(|x| x.image.clone()).collect()
            }
            Some(list::ListData::Albums(s)) => {
                let (a, b) = self.list_range(id, s.len());
                s[a.min(s.len())..b.min(s.len())].iter().filter_map(|x| x.image.clone()).collect()
            }
            _ => Vec::new(),
        };
        self.request_images(refs.iter().map(Some))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::ArtistRef;

    fn album(name: &str, year: i32, kind: Option<&str>, artist: &str) -> Album {
        Album {
            release_types: Vec::new(),
            info: Default::default(),
            id: name.into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            name: name.into(),
            album_artists: vec![ArtistRef { id: Some(artist.into()), name: artist.into() }],
            image: None,
            year: Some(year),
            release_type: kind.map(str::to_string),
            genres: vec![],
            song_count: Some(10),
            duration: Duration::from_secs(600),
            favorite: false,
            rating: None,
            play_count: None,
            songs: vec![],
        }
    }

    #[test]
    fn groups_follow_the_default_priority() {
        let own = vec![
            album("A", 2020, Some("album"), "me"),
            album("S", 2021, Some("single"), "me"),
            album("L", 2019, Some("live"), "me"),
            album("N", 2018, None, "me"),
        ];
        let appears = vec![album("C", 2015, Some("compilation"), "va")];
        let groups = group_albums(&own, &appears, "me", Grouping::Primary, &ArtistSettings::default());
        let keys: Vec<&str> = groups.iter().map(|g| g.key.as_str()).collect();
        // Feishin's default order: appears-on, album, ..., live, ..., single.
        assert_eq!(keys, ["appears-on", "album", "live", "single"]);
        assert_eq!(groups[1].albums.len(), 2); // typeless albums are albums
        assert_eq!(groups[0].title, "Appears on");
    }

    #[test]
    fn hidden_groups_and_custom_order() {
        let mut s = ArtistSettings::default();
        s.release_types.retain(|i| i.id != "releaseTypeLive");
        s.release_types
            .push(listen_core::settings::SortableItem { id: "releaseTypeLive".into(), enabled: false });
        s.release_types.retain(|i| i.id != "releaseTypeSingle");
        s.release_types
            .insert(0, listen_core::settings::SortableItem { id: "releaseTypeSingle".into(), enabled: true });
        let own = vec![
            album("A", 2020, Some("album"), "me"),
            album("S", 2021, Some("single"), "me"),
            album("L", 2019, Some("live"), "me"),
        ];
        let groups = group_albums(&own, &[], "me", Grouping::Primary, &s);
        let keys: Vec<&str> = groups.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["single", "album"]);
    }

    #[test]
    fn all_grouping_combines_types() {
        let own = vec![
            album("X", 2020, Some("album;live"), "me"),
            album("Y", 2021, Some("compilation"), "me"),
            album("Z", 2022, Some("album"), "me"),
        ];
        let groups = group_albums(&own, &[], "me", Grouping::All, &ArtistSettings::default());
        let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
        assert!(titles.contains(&"Album · Live"), "{titles:?}");
        assert!(titles.contains(&"Compilation"));
        assert!(titles.contains(&"Album"));
    }

    #[test]
    fn all_grouping_uses_every_release_type_the_server_sent() {
        let mut live = album("X", 2020, Some("album"), "me");
        live.release_types = vec!["album".into(), "live".into()];
        let mut plain = album("Y", 2021, Some("album"), "me");
        plain.release_types = vec!["album".into()];
        let groups = group_albums(&[live, plain], &[], "me", Grouping::All, &ArtistSettings::default());
        let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
        assert!(titles.contains(&"Album · Live"), "{titles:?}");
        assert!(titles.contains(&"Album"), "{titles:?}");
    }

    #[test]
    fn albums_without_artist_ids_are_the_artists_own() {
        let mut a = album("A", 2020, Some("album"), "me");
        a.album_artists = vec![ArtistRef { id: None, name: "me".into() }];
        let groups = group_albums(&[a], &[], "me", Grouping::Primary, &ArtistSettings::default());
        assert_eq!(groups[0].key, "album");
    }

    #[test]
    fn albums_sort_by_the_chosen_field() {
        let mut v =
            vec![album("b", 2001, None, "x"), album("a", 2003, None, "x"), album("c", 2002, None, "x")];
        sort_albums(&mut v, AlbumSortBy::Year, false);
        assert_eq!(v.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["a", "c", "b"]);
        sort_albums(&mut v, AlbumSortBy::Name, true);
        assert_eq!(v.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    }

    #[test]
    fn biography_keeps_links_and_drops_other_tags() {
        let html = "Neon Coast is a band. <a href=\"https://www.last.fm/music/Neon+Coast\">Read more on Last.fm</a>\
                    <br>Formed in &quot;2010&quot; &amp; <b>active</b> &#8211; still.";
        let spans = parse_bio(html);
        assert_eq!(spans[0], BioSpan { text: "Neon Coast is a band. ".into(), link: None });
        assert_eq!(
            spans[1],
            BioSpan {
                text: "Read more on Last.fm".into(),
                link: Some("https://www.last.fm/music/Neon+Coast".into())
            }
        );
        assert_eq!(
            bio_text(&spans),
            "Neon Coast is a band. Read more on Last.fm\nFormed in \"2010\" & active \u{2013} still."
        );
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn biography_ignores_unsafe_links() {
        let spans = parse_bio("<a href=\"javascript:alert(1)\">x</a> y");
        assert!(spans.iter().all(|s| s.link.is_none()));
        assert_eq!(bio_text(&spans), "x y");
        assert!(parse_bio("").is_empty());
        assert_eq!(bio_text(&parse_bio("a < b & c")), "a < b & c");
    }

    #[test]
    fn site_links_follow_the_settings() {
        let mut s = ArtistSettings::default();
        let names = |l: &[SiteLink]| l.iter().map(|x| x.name).collect::<Vec<_>>();
        let links = site_links(&s, "Neon Coast", None, None);
        assert_eq!(names(&links), ["Last.fm", "ListenBrainz", "Qobuz", "Spotify"]);
        assert_eq!(links[0].url, "https://www.last.fm/music/Neon%20Coast");
        let links = site_links(&s, "Neon Coast", Some("abc"), Some("https://last.fm/x"));
        assert_eq!(names(&links), ["Last.fm", "MusicBrainz", "ListenBrainz", "Qobuz", "Spotify"]);
        assert_eq!(links[0].url, "https://last.fm/x");
        assert_eq!(links[2].url, "https://listenbrainz.org/artist/abc");
        s.external_links.native_spotify = true;
        assert_eq!(site_links(&s, "A B", None, None).last().unwrap().url, "spotify:search:A%20B");
        s.external_links.enabled = false;
        assert!(site_links(&s, "A", None, None).is_empty());
    }

    #[test]
    fn settings_changes_apply() {
        let mut s = ArtistSettings::default();
        ArtistSetting::MoveSection(0, 1).apply(&mut s);
        assert_eq!(s.items[0].id, "favoriteSongs");
        ArtistSetting::MoveSection(0, -1).apply(&mut s); // off the top: nothing
        assert_eq!(s.items[0].id, "favoriteSongs");
        ArtistSetting::MoveSection(99, 1).apply(&mut s);
        ArtistSetting::SectionEnabled(0, false).apply(&mut s);
        assert!(!s.sections().contains(&listen_core::settings::ArtistSection::FavoriteSongs));
        ArtistSetting::ReleaseEnabled(1, false).apply(&mut s);
        assert!(s.release_type_hidden("album"));
        ArtistSetting::RadioCount(0).apply(&mut s);
        assert_eq!(s.radio_count, 1);
        ArtistSetting::Link(LinkSite::All, false).apply(&mut s);
        assert!(!s.external_links.enabled);
    }

    #[test]
    fn totals_read_like_feishin() {
        assert_eq!(total_time(Duration::from_secs(45 * 60)), "45 min");
        assert_eq!(total_time(Duration::from_secs(3 * 3600)), "3 hr");
        assert_eq!(total_time(Duration::from_secs(3 * 3600 + 20 * 60)), "3 hr 20 min");
    }

    #[test]
    fn song_search_matches_title_album_and_artist() {
        let mut s = listen_core::history::Entry::new(
            &listen_core::model::Song {
                mbz: Default::default(),
                extra: Default::default(),
                id: "1".into(),
                server_id: ServerId("s".into()),
                source: SourceKind::Subsonic,
                title: "Night Drive".into(),
                artists: vec![ArtistRef { id: None, name: "Neon Coast".into() }],
                album_artists: vec![],
                album: Some("Horizon".into()),
                album_id: None,
                image: None,
                duration: Duration::ZERO,
                track_number: None,
                disc_number: None,
                year: None,
                genres: vec![],
                codec: None,
                bit_rate: None,
                sample_rate: None,
                bit_depth: None,
                bpm: None,
                favorite: false,
                rating: None,
                play_count: None,
                gain: Default::default(),
                explicit: false,
                path: None,
                size: None,
                suffix: None,
                channels: None,
                created: None,
                comment: None,
            },
            0,
        )
        .to_song();
        s.album = Some("Horizon".into());
        let v = vec![s];
        assert_eq!(matching(&v, "").len(), 1);
        assert_eq!(matching(&v, "night").len(), 1);
        assert_eq!(matching(&v, "HORIZON").len(), 1);
        assert_eq!(matching(&v, "neon").len(), 1);
        assert!(matching(&v, "zzz").is_empty());
    }
}
