//! Local music folder backend: scans a directory once on connect, reads tags
//! and cover art with `lofty`, and serves the library from memory.

pub mod details;
mod scan;
pub mod tags;

pub use scan::{Scanned, load_cover, scan};

use async_trait::async_trait;
use listen_core::model::{
    Album, Artist, ArtistRef, Genre, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_source::{
    AlbumQuery, AlbumSort, Capabilities, ItemDetails, ItemKind, ListQuery, MusicSource, Result, SongQuery,
    SongSort, SortOrder, SourceError, StreamSource,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

const UNKNOWN_ARTIST: &str = "Unknown Artist";
const VARIOUS_ARTISTS: &str = "Various Artists";

fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}

fn artist_id(name: &str) -> String {
    format!("ar-{:016x}", fnv1a(&name.to_lowercase()))
}

fn file_url(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(String::from)
}

struct Track {
    song: Song,
    path: PathBuf,
    added: SystemTime,
    /// Lower-cased "title artist album" for search.
    haystack: String,
}

struct Entry {
    album: Album,
    added: SystemTime,
    /// Indices into `Index::tracks`, in disc/track order.
    tracks: Vec<usize>,
    artist_ids: HashSet<String>,
}

struct Index {
    tracks: Vec<Track>,
    by_id: HashMap<String, usize>,
    albums: Vec<Entry>,
    album_by_id: HashMap<String, usize>,
    artists: Vec<Artist>,
    artist_by_id: HashMap<String, usize>,
    genres: Vec<Genre>,
}

fn refs(names: &[String]) -> Vec<ArtistRef> {
    names.iter().map(|n| ArtistRef { id: Some(artist_id(n)), name: n.clone() }).collect()
}

impl Index {
    fn build(server: &ServerId, files: Vec<Scanned>) -> Self {
        // Group into albums: same album name and album artist; without an album
        // artist, tracks of the same folder (so unrelated "Greatest Hits" stay apart).
        let mut groups: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            let Some(album) = &f.album else { continue };
            let (artist, dir) = match &f.album_artist {
                Some(a) => (a.to_lowercase(), String::new()),
                None => (
                    String::new(),
                    f.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                ),
            };
            groups.entry((album.to_lowercase(), artist, dir)).or_default().push(i);
        }

        let mut album_of: HashMap<usize, String> = HashMap::new();
        let mut album_cover: HashMap<String, Option<String>> = HashMap::new();
        let mut entries: Vec<(Album, Vec<usize>)> = Vec::new();
        let mut group_list: Vec<_> = groups.into_iter().collect();
        group_list.sort_by(|a, b| a.0.cmp(&b.0));
        for ((_, _, _), mut idx) in group_list {
            idx.sort_by_key(|&i| {
                (files[i].disc.unwrap_or(1), files[i].track.unwrap_or(0), files[i].rel.clone())
            });
            let first = &files[idx[0]];
            let album_name = first.album.clone().unwrap_or_default();
            let album_artist = match &first.album_artist {
                Some(a) => a.clone(),
                None => {
                    let distinct: HashSet<String> = idx
                        .iter()
                        .map(|&i| files[i].artists.first().cloned().unwrap_or_else(|| UNKNOWN_ARTIST.into()))
                        .collect();
                    if distinct.len() > 1 {
                        VARIOUS_ARTISTS.to_string()
                    } else {
                        distinct.into_iter().next().unwrap_or_else(|| UNKNOWN_ARTIST.into())
                    }
                }
            };
            let id = format!(
                "al-{:016x}",
                fnv1a(&format!(
                    "{}\u{1}{}\u{1}{}",
                    album_name.to_lowercase(),
                    album_artist.to_lowercase(),
                    first.path.parent().map(|p| p.to_string_lossy()).unwrap_or_default()
                ))
            );
            let cover = idx
                .iter()
                .find_map(|&i| files[i].folder_art.as_ref())
                .or_else(|| idx.iter().find(|&&i| files[i].embedded_art).map(|&i| &files[i].path))
                .and_then(|p| file_url(p));
            let mut genres: Vec<String> = Vec::new();
            for &i in &idx {
                for g in &files[i].genres {
                    if !genres.contains(g) {
                        genres.push(g.clone());
                    }
                }
            }
            for &i in &idx {
                album_of.insert(i, id.clone());
            }
            album_cover.insert(id.clone(), cover.clone());
            let album = Album {
                release_types: Vec::new(),
                id,
                server_id: server.clone(),
                source: SourceKind::Local,
                name: album_name,
                album_artists: refs(&[album_artist]),
                image: cover.map(ImageRef::Url),
                year: idx.iter().find_map(|&i| files[i].year),
                release_type: None,
                genres,
                song_count: Some(idx.len() as u32),
                duration: idx.iter().map(|&i| files[i].duration).sum(),
                favorite: false,
                rating: None,
                play_count: None,
                songs: Vec::new(),
                info: Default::default(),
            };
            entries.push((album, idx));
        }

        // Tracks, in file order; `files` index -> tracks index is the identity.
        let mut tracks: Vec<Track> = Vec::with_capacity(files.len());
        for (i, f) in files.iter().enumerate() {
            let album_id = album_of.get(&i).cloned();
            let album_artists = album_id
                .as_ref()
                .and_then(|id| entries.iter().find(|(a, _)| &a.id == id))
                .map(|(a, _)| a.album_artists.clone())
                .unwrap_or_default();
            let artist_names: Vec<String> =
                if f.artists.is_empty() { vec![UNKNOWN_ARTIST.into()] } else { f.artists.clone() };
            let image = album_id
                .as_ref()
                .and_then(|id| album_cover.get(id).cloned().flatten())
                .or_else(|| {
                    f.folder_art.as_ref().or(f.embedded_art.then_some(&f.path)).and_then(|p| file_url(p))
                })
                .map(ImageRef::Url);
            let song = Song {
                mbz: Default::default(),
                extra: Default::default(),
                id: format!("{:016x}", fnv1a(&f.rel)),
                server_id: server.clone(),
                source: SourceKind::Local,
                title: f.title.clone(),
                artists: refs(&artist_names),
                album_artists,
                album: f.album.clone(),
                album_id,
                image,
                duration: f.duration,
                track_number: f.track,
                disc_number: f.disc,
                year: f.year,
                genres: f.genres.clone(),
                codec: f.codec.clone(),
                bit_rate: f.bit_rate,
                sample_rate: f.sample_rate,
                bit_depth: f.bit_depth,
                bpm: f.bpm,
                favorite: false,
                rating: None,
                play_count: None,
                gain: f.gain.clone(),
                explicit: false,
                path: None,
                size: None,
                suffix: None,
                channels: None,
                created: None,
                comment: None,
            };
            let haystack =
                format!("{} {} {}", song.title, artist_names.join(" "), f.album.clone().unwrap_or_default())
                    .to_lowercase();
            tracks.push(Track {
                song,
                path: f.path.clone(),
                added: f.modified.unwrap_or(SystemTime::UNIX_EPOCH),
                haystack,
            });
        }
        let by_id: HashMap<String, usize> =
            tracks.iter().enumerate().map(|(i, t)| (t.song.id.clone(), i)).collect();

        let mut albums: Vec<Entry> = entries
            .into_iter()
            .map(|(album, idx)| {
                let mut artist_ids: HashSet<String> =
                    album.album_artists.iter().filter_map(|a| a.id.clone()).collect();
                for &i in &idx {
                    artist_ids.extend(tracks[i].song.artists.iter().filter_map(|a| a.id.clone()));
                }
                let added = idx.iter().map(|&i| tracks[i].added).max().unwrap_or(SystemTime::UNIX_EPOCH);
                Entry { album, added, tracks: idx, artist_ids }
            })
            .collect();
        albums.sort_by_key(|e| e.album.name.to_lowercase());
        let album_by_id = albums.iter().enumerate().map(|(i, e)| (e.album.id.clone(), i)).collect();

        // Artists: every name that is a track artist or album artist.
        let mut names: HashMap<String, String> = HashMap::new();
        let mut song_counts: HashMap<String, u32> = HashMap::new();
        for t in &tracks {
            for a in &t.song.artists {
                if let Some(id) = &a.id {
                    names.entry(id.clone()).or_insert_with(|| a.name.clone());
                    *song_counts.entry(id.clone()).or_default() += 1;
                }
            }
        }
        for e in &albums {
            for a in &e.album.album_artists {
                if let Some(id) = &a.id {
                    names.entry(id.clone()).or_insert_with(|| a.name.clone());
                }
            }
        }
        let mut artists: Vec<Artist> = names
            .into_iter()
            .map(|(id, name)| Artist {
                album_count: Some(albums.iter().filter(|e| e.artist_ids.contains(&id)).count() as u32),
                song_count: song_counts.get(&id).copied(),
                id,
                server_id: server.clone(),
                source: SourceKind::Local,
                name,
                image: None,
                favorite: false,
                rating: None,
                biography: None,
                similar: Vec::new(),
            })
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        let artist_by_id = artists.iter().enumerate().map(|(i, a)| (a.id.clone(), i)).collect();

        let mut genre_map: HashMap<String, (HashSet<String>, u32)> = HashMap::new();
        for t in &tracks {
            for g in &t.song.genres {
                let e = genre_map.entry(g.clone()).or_default();
                e.1 += 1;
                if let Some(a) = &t.song.album_id {
                    e.0.insert(a.clone());
                }
            }
        }
        let mut genres: Vec<Genre> = genre_map
            .into_iter()
            .map(|(name, (albums, songs))| Genre {
                id: name.clone(),
                name,
                album_count: Some(albums.len() as u32),
                song_count: Some(songs),
            })
            .collect();
        genres.sort_by_key(|g| g.name.to_lowercase());

        Index { tracks, by_id, albums, album_by_id, artists, artist_by_id, genres }
    }

    fn album_songs(&self, entry: &Entry) -> Vec<Song> {
        entry.tracks.iter().map(|&i| self.tracks[i].song.clone()).collect()
    }
}

fn page<T: Clone>(all: Vec<T>, start: usize, limit: usize) -> Page<T> {
    let total = all.len();
    let items = all.into_iter().skip(start).take(if limit == 0 { usize::MAX } else { limit }).collect();
    Page { items, start_index: start, total: Some(total) }
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// A folder on this computer, scanned into memory.
pub struct LocalFolder {
    server_id: ServerId,
    root: PathBuf,
    index: Arc<Index>,
}

impl LocalFolder {
    /// Scans `root` (off the async executor); fails if it is not a folder.
    pub async fn connect(server_id: ServerId, root: &str) -> Result<Self> {
        let root = PathBuf::from(root.trim());
        if !root.is_dir() {
            return Err(SourceError::Other(format!("{} is not a folder.", root.display())));
        }
        let (scan_root, id) = (root.clone(), server_id.clone());
        let index = tokio::task::spawn_blocking(move || {
            let started = std::time::Instant::now();
            let files = scan(&scan_root);
            let index = Index::build(&id, files);
            tracing::info!("scanned {} songs in {:?}", index.tracks.len(), started.elapsed());
            index
        })
        .await
        .map_err(|e| SourceError::Other(e.to_string()))?;
        Ok(Self { server_id, root, index: Arc::new(index) })
    }

    pub fn server_id(&self) -> &ServerId {
        &self.server_id
    }

    pub fn song_count(&self) -> usize {
        self.index.tracks.len()
    }

    fn sorted_songs(&self, query: &SongQuery) -> Vec<Song> {
        let mut tracks: Vec<&Track> = self
            .index
            .tracks
            .iter()
            .filter(|t| {
                query.genre.as_ref().is_none_or(|g| t.song.genres.iter().any(|x| x.eq_ignore_ascii_case(g)))
            })
            .filter(|t| query.search.as_ref().is_none_or(|s| t.haystack.contains(&s.to_lowercase())))
            .filter(|_| !query.favorites_only)
            .collect();
        match query.sort {
            SongSort::Name => tracks.sort_by_key(|t| t.song.title.to_lowercase()),
            SongSort::Album => tracks.sort_by_key(|t| {
                (
                    t.song.album.clone().unwrap_or_default().to_lowercase(),
                    t.song.disc_number.unwrap_or(1),
                    t.song.track_number.unwrap_or(0),
                )
            }),
            SongSort::Artist => tracks.sort_by_key(|t| t.song.artist_line().to_lowercase()),
            SongSort::Year => tracks.sort_by_key(|t| t.song.year.unwrap_or(0)),
            SongSort::RecentlyAdded => tracks.sort_by_key(|t| std::cmp::Reverse(t.added)),
            SongSort::Random => {
                use rand::seq::SliceRandom;
                tracks.shuffle(&mut rand::rng());
            }
            // The rest has no data in a folder.
            _ => {}
        }
        if query.order == SortOrder::Descending && !matches!(query.sort, SongSort::Random) {
            tracks.reverse();
        }
        tracks.into_iter().map(|t| t.song.clone()).collect()
    }

    fn sorted_albums(&self, query: &AlbumQuery) -> Vec<Album> {
        let mut albums: Vec<&Entry> = self
            .index
            .albums
            .iter()
            .filter(|e| {
                query.genre.as_ref().is_none_or(|g| e.album.genres.iter().any(|x| x.eq_ignore_ascii_case(g)))
            })
            .filter(|e| query.artist_id.as_ref().is_none_or(|a| e.artist_ids.contains(a)))
            .filter(|e| {
                query
                    .search
                    .as_ref()
                    .is_none_or(|s| contains(&e.album.name, s) || contains(&e.album.artist_line(), s))
            })
            .filter(|_| !query.favorites_only)
            .collect();
        match query.sort {
            AlbumSort::Artist | AlbumSort::AlbumArtist => {
                albums.sort_by_key(|e| e.album.artist_line().to_lowercase())
            }
            AlbumSort::Year => albums.sort_by_key(|e| e.album.year.unwrap_or(0)),
            AlbumSort::RecentlyAdded => albums.sort_by_key(|e| std::cmp::Reverse(e.added)),
            AlbumSort::Random => {
                use rand::seq::SliceRandom;
                albums.shuffle(&mut rand::rng());
            }
            _ => {}
        }
        if query.order == SortOrder::Descending && !matches!(query.sort, AlbumSort::Random) {
            albums.reverse();
        }
        albums.into_iter().map(|e| e.album.clone()).collect()
    }
}

#[async_trait]
impl MusicSource for LocalFolder {
    fn kind(&self) -> SourceKind {
        SourceKind::Local
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::GENRES | Capabilities::ALBUM_ARTISTS | Capabilities::ITEM_DETAILS
    }

    async fn ping(&self) -> Result<()> {
        if self.root.is_dir() {
            Ok(())
        } else {
            Err(SourceError::Other(format!("{} is gone.", self.root.display())))
        }
    }

    fn album_sorts(&self) -> &'static [listen_source::SortOption<AlbumSort>] {
        listen_source::sorting::LOCAL_ALBUMS
    }

    fn song_sorts(&self) -> &'static [listen_source::SortOption<SongSort>] {
        listen_source::sorting::LOCAL_SONGS
    }

    fn artist_sorts(&self) -> &'static [listen_source::SortOption<listen_source::ArtistSort>] {
        listen_source::sorting::LIBRARY_ARTISTS
    }

    fn album_filters(&self) -> listen_source::QueryFilters {
        listen_source::QueryFilters::GENRES
            | listen_source::QueryFilters::ARTISTS
            | listen_source::QueryFilters::YEAR_RANGE
            | listen_source::QueryFilters::MULTI
    }

    fn song_filters(&self) -> listen_source::QueryFilters {
        listen_source::QueryFilters::GENRES
            | listen_source::QueryFilters::YEAR_RANGE
            | listen_source::QueryFilters::MULTI
    }

    fn artist_filters(&self) -> listen_source::QueryFilters {
        listen_source::QueryFilters::empty()
    }

    async fn albums(&self, query: &AlbumQuery) -> Result<Page<Album>> {
        // The sort is done; the year range and genre lists are the shared filters (order untouched).
        let rest = AlbumQuery { sort: AlbumSort::Id, order: SortOrder::Ascending, ..query.clone() };
        Ok(page(
            listen_source::clientside::albums(self.sorted_albums(query), &rest),
            query.start,
            query.limit,
        ))
    }

    async fn album(&self, id: &str) -> Result<Album> {
        let entry =
            self.index.album_by_id.get(id).map(|&i| &self.index.albums[i]).ok_or(SourceError::NotFound)?;
        let mut album = entry.album.clone();
        album.songs = self.index.album_songs(entry);
        Ok(album)
    }

    async fn artists(&self, query: &ListQuery) -> Result<Page<Artist>> {
        let all: Vec<Artist> = self
            .index
            .artists
            .iter()
            .filter(|a| query.search.as_ref().is_none_or(|s| contains(&a.name, s)))
            .cloned()
            .collect();
        Ok(page(all, query.start, query.limit))
    }

    async fn artist(&self, id: &str) -> Result<Artist> {
        self.index.artist_by_id.get(id).map(|&i| self.index.artists[i].clone()).ok_or(SourceError::NotFound)
    }

    async fn artist_albums(&self, id: &str) -> Result<Vec<Album>> {
        let q = AlbumQuery { sort: AlbumSort::Year, artist_id: Some(id.to_string()), ..Default::default() };
        Ok(self.sorted_albums(&q))
    }

    async fn top_songs(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        Ok(self
            .index
            .tracks
            .iter()
            .filter(|t| t.song.artists.iter().any(|a| a.id.as_deref() == Some(artist.id.as_str())))
            .take(count)
            .map(|t| t.song.clone())
            .collect())
    }

    async fn songs(&self, query: &SongQuery) -> Result<Page<Song>> {
        let rest = SongQuery { sort: SongSort::Id, order: SortOrder::Ascending, ..query.clone() };
        Ok(page(listen_source::clientside::songs(self.sorted_songs(query), &rest), query.start, query.limit))
    }

    async fn random_songs(&self, count: usize, genre: Option<&str>) -> Result<Vec<Song>> {
        let q = SongQuery {
            sort: SongSort::Random,
            genre: genre.map(String::from),
            limit: count,
            ..Default::default()
        };
        Ok(self.sorted_songs(&q).into_iter().take(count).collect())
    }

    async fn genres(&self) -> Result<Vec<Genre>> {
        Ok(self.index.genres.clone())
    }

    async fn search(&self, text: &str, limit: usize) -> Result<SearchResults> {
        let limit = if limit == 0 { usize::MAX } else { limit };
        let songs = self
            .sorted_songs(&SongQuery { search: Some(text.to_string()), ..Default::default() })
            .into_iter()
            .take(limit)
            .collect();
        let albums = self
            .sorted_albums(&AlbumQuery { search: Some(text.to_string()), ..Default::default() })
            .into_iter()
            .take(limit)
            .collect();
        let artists =
            self.index.artists.iter().filter(|a| contains(&a.name, text)).take(limit).cloned().collect();
        Ok(SearchResults { songs, albums, artists, playlists: Vec::new() })
    }

    async fn playlists(&self) -> Result<Vec<Playlist>> {
        Ok(Vec::new())
    }

    async fn playlist_songs(&self, _id: &str) -> Result<Vec<Song>> {
        Err(SourceError::NotFound)
    }

    async fn set_favorite(&self, _song_ids: &[String], _favorite: bool) -> Result<()> {
        Err(SourceError::Unsupported)
    }

    async fn stream(&self, song: &Song) -> Result<StreamSource> {
        let track =
            self.index.by_id.get(&song.id).map(|&i| &self.index.tracks[i]).ok_or(SourceError::NotFound)?;
        if !track.path.is_file() {
            return Err(SourceError::NotFound);
        }
        let url = file_url(&track.path).ok_or_else(|| SourceError::Other("invalid path".into()))?;
        Ok(StreamSource::direct(url))
    }

    fn local_path(&self, song: &Song) -> Option<PathBuf> {
        let track = self.index.by_id.get(&song.id).map(|&i| &self.index.tracks[i])?;
        Some(track.path.clone())
    }

    async fn item_details(&self, kind: ItemKind, id: &str) -> Result<ItemDetails> {
        if kind != ItemKind::Song {
            return Ok(ItemDetails::default());
        }
        let path = self
            .index
            .by_id
            .get(id)
            .map(|&i| self.index.tracks[i].path.clone())
            .ok_or(SourceError::NotFound)?;
        tokio::task::spawn_blocking(move || details::song(&path))
            .await
            .map_err(|e| SourceError::Other(e.to_string()))?
            .map_err(SourceError::Other)
    }

    fn image_url(&self, image: &ImageRef, _size: u32) -> Option<String> {
        match image {
            ImageRef::Url(u) => Some(u.clone()),
            ImageRef::Id(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
