//! Artist page settings: sections and their order, discography release-type groups, radio size and
//! external links. Ids are Feishin's (`artistItems`, `artistReleaseTypeItems`, `artistRadioCount`,
//! external links in store/settings.store.ts), so a Feishin export imports as is.

use super::*;

/// One entry of a reorderable list (Feishin `SortableItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortableItem {
    pub id: String,
    pub enabled: bool,
}

impl SortableItem {
    fn on(id: &str) -> Self {
        Self { id: id.into(), enabled: true }
    }
}

/// A section of the artist page (Feishin `ArtistItem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtistSection {
    Biography,
    FavoriteSongs,
    /// The discography (Feishin `recentAlbums`): always shown, only its place can change.
    RecentAlbums,
    SimilarArtists,
    TopSongs,
}

impl ArtistSection {
    pub const ALL: [ArtistSection; 5] = [
        ArtistSection::Biography,
        ArtistSection::FavoriteSongs,
        ArtistSection::RecentAlbums,
        ArtistSection::SimilarArtists,
        ArtistSection::TopSongs,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ArtistSection::Biography => "biography",
            ArtistSection::FavoriteSongs => "favoriteSongs",
            ArtistSection::RecentAlbums => "recentAlbums",
            ArtistSection::SimilarArtists => "similarArtists",
            ArtistSection::TopSongs => "topSongs",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            ArtistSection::Biography => "Biography",
            ArtistSection::FavoriteSongs => "Favorite songs",
            ArtistSection::RecentAlbums => "Recent releases",
            ArtistSection::SimilarArtists => "Related artists",
            ArtistSection::TopSongs => "Top songs",
        }
    }
}

/// Release-type groups of the discography: (setting id, key used by the grouping, label).
pub const RELEASE_TYPES: [(&str, &str, &str); 18] = [
    ("appearsOn", "appears-on", "Appears on"),
    ("releaseTypeAlbum", "album", "Album"),
    ("releaseTypeAudioDrama", "audio drama", "Audio drama"),
    ("releaseTypeAudiobook", "audiobook", "Audiobook"),
    ("releaseTypeBroadcast", "broadcast", "Broadcast"),
    ("releaseTypeCompilation", "compilation", "Compilation"),
    ("releaseTypeDemo", "demo", "Demo"),
    ("releaseTypeDjMix", "dj-mix", "DJ-mix"),
    ("releaseTypeEp", "ep", "EP"),
    ("releaseTypeFieldRecording", "field recording", "Field recording"),
    ("releaseTypeInterview", "interview", "Interview"),
    ("releaseTypeLive", "live", "Live"),
    ("releaseTypeMixtapeStreet", "mixtape/street", "Mixtape / street"),
    ("releaseTypeOther", "other", "Other"),
    ("releaseTypeRemix", "remix", "Remix"),
    ("releaseTypeSingle", "single", "Single"),
    ("releaseTypeSoundtrack", "soundtrack", "Soundtrack"),
    ("releaseTypeSpokenWord", "spokenword", "Spoken word"),
];

/// The setting id of a grouping key (`"ep"` → `"releaseTypeEp"`).
pub fn release_type_id(key: &str) -> Option<&'static str> {
    RELEASE_TYPES.iter().find(|(_, k, _)| *k == key).map(|(id, _, _)| *id)
}

/// Switches for the links on artist and album pages (Feishin `externalLinks`, `lastFM`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalLinksSettings {
    /// Master switch.
    pub enabled: bool,
    pub last_fm: bool,
    /// Only where the server knows a MusicBrainz id.
    pub music_brainz: bool,
    pub listen_brainz: bool,
    pub qobuz: bool,
    pub spotify: bool,
    /// Open `spotify:` links in the desktop app instead of the web player.
    pub native_spotify: bool,
}

impl Default for ExternalLinksSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            last_fm: true,
            music_brainz: true,
            listen_brainz: true,
            qobuz: true,
            spotify: true,
            native_spotify: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistSettings {
    /// Sections of the page, in order (`ArtistSection::id`).
    pub items: Vec<SortableItem>,
    /// Priority and visibility of the discography groups (`RELEASE_TYPES` ids).
    pub release_types: Vec<SortableItem>,
    /// Songs queued by "Artist radio" (Feishin `artistRadioCount`).
    pub radio_count: u32,
    pub external_links: ExternalLinksSettings,
}

impl Default for ArtistSettings {
    fn default() -> Self {
        Self {
            items: ArtistSection::ALL.iter().map(|s| SortableItem::on(s.id())).collect(),
            release_types: RELEASE_TYPES.iter().map(|(id, _, _)| SortableItem::on(id)).collect(),
            radio_count: 20,
            external_links: ExternalLinksSettings::default(),
        }
    }
}

impl ArtistSettings {
    /// The sections to show, in the saved order. Sections missing from an old
    /// or hand-edited config follow at the end (enabled); unknown ids are dropped.
    pub fn sections(&self) -> Vec<ArtistSection> {
        let mut out: Vec<ArtistSection> = Vec::new();
        for item in &self.items {
            if let Some(s) = ArtistSection::from_id(&item.id)
                && !out.contains(&s)
                && (item.enabled || s == ArtistSection::RecentAlbums)
            {
                out.push(s);
            }
        }
        for s in ArtistSection::ALL {
            if !self.items.iter().any(|i| i.id == s.id()) {
                out.push(s);
            }
        }
        out
    }

    /// Group keys of the discography in priority order, hidden groups left out.
    /// A key that is not in the list is shown last.
    pub fn release_priority(&self) -> Vec<&'static str> {
        self.release_types
            .iter()
            .filter(|i| i.enabled)
            .filter_map(|i| RELEASE_TYPES.iter().find(|(id, _, _)| *id == i.id).map(|(_, key, _)| *key))
            .collect()
    }

    /// Whether the group with this setting id is hidden.
    pub fn release_type_hidden(&self, key: &str) -> bool {
        release_type_id(key).is_some_and(|id| self.release_types.iter().any(|i| i.id == id && !i.enabled))
    }

    /// Fills in groups added after the config was saved.
    pub fn add_missing(&mut self) {
        for (id, _, _) in RELEASE_TYPES {
            if !self.release_types.iter().any(|i| i.id == id) {
                self.release_types.push(SortableItem::on(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sections_are_feishins() {
        let s = ArtistSettings::default();
        assert_eq!(
            s.sections(),
            [
                ArtistSection::Biography,
                ArtistSection::FavoriteSongs,
                ArtistSection::RecentAlbums,
                ArtistSection::SimilarArtists,
                ArtistSection::TopSongs
            ]
        );
        assert_eq!(s.radio_count, 20);
    }

    #[test]
    fn disabled_sections_go_but_the_discography_stays() {
        let mut s = ArtistSettings::default();
        for i in &mut s.items {
            i.enabled = false;
        }
        assert_eq!(s.sections(), [ArtistSection::RecentAlbums]);
    }

    #[test]
    fn missing_and_unknown_items_are_handled() {
        let s = ArtistSettings {
            items: vec![SortableItem::on("topSongs"), SortableItem::on("bogus")],
            ..Default::default()
        };
        let sections = s.sections();
        assert_eq!(sections[0], ArtistSection::TopSongs);
        assert_eq!(sections.len(), 5);
    }

    #[test]
    fn release_type_priority_follows_the_list() {
        let mut s = ArtistSettings::default();
        s.release_types.retain(|i| i.id != "releaseTypeLive");
        let p = s.release_priority();
        assert_eq!(p[0], "appears-on");
        assert!(!p.contains(&"live"));
        s.release_types.push(SortableItem { id: "releaseTypeLive".into(), enabled: false });
        assert!(s.release_type_hidden("live"));
        assert!(!s.release_type_hidden("album"));
        assert_eq!(release_type_id("ep"), Some("releaseTypeEp"));
    }

    #[test]
    fn old_configs_get_the_defaults() {
        let s: Settings = toml::from_str("[appearance]\ntheme = \"nord\"\n").unwrap();
        assert_eq!(s.artist, ArtistSettings::default());
        let partial: Settings = toml::from_str("[artist]\nradio_count = 5\n").unwrap();
        assert_eq!(partial.artist.radio_count, 5);
        assert!(partial.artist.external_links.enabled);
    }
}
