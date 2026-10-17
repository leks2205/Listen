//! The fields a smart-playlist rule can test (Feishin `NDSongQueryFields`) and the
//! operators each field type offers (`NDSongQuery*Operators`).

/// What a field holds; it decides the operators and the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    String,
    Number,
    Boolean,
    Date,
    /// A playlist id.
    Playlist,
}

impl Kind {
    /// The names Feishin's custom-field settings use.
    pub const NAMES: [(&'static str, Kind); 6] = [
        ("string", Kind::String),
        ("number", Kind::Number),
        ("boolean", Kind::Boolean),
        ("date", Kind::Date),
        ("dateRange", Kind::Date),
        ("playlist", Kind::Playlist),
    ];

    pub fn parse(name: &str) -> Kind {
        Kind::NAMES.iter().find(|(n, _)| *n == name).map(|(_, k)| *k).unwrap_or(Kind::String)
    }

    /// The names offered in the custom-field settings.
    pub fn name(self) -> &'static str {
        match self {
            Kind::String => "string",
            Kind::Number => "number",
            Kind::Boolean => "boolean",
            Kind::Date => "date",
            Kind::Playlist => "playlist",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub label: &'static str,
    pub kind: Kind,
    /// The key in the rules JSON.
    pub key: &'static str,
}

const fn f(label: &'static str, kind: Kind, key: &'static str) -> Field {
    Field { label, kind, key }
}
use Kind as K;

pub static FIELDS: &[Field] = &[
    f("Album", K::String, "album"),
    f("Album Artist", K::String, "albumartist"),
    f("Album Artists", K::String, "albumartists"),
    f("Album Comment", K::String, "albumcomment"),
    f("Album Date Added", K::Date, "albumdateadded"),
    f("Album Date Modified", K::Date, "albumdatemodified"),
    f("Album Duration", K::Number, "albumduration"),
    f("Album Song Count", K::Number, "albumsongcount"),
    f("Album Size", K::Number, "albumsize"),
    f("Album Date Favorited", K::Date, "albumdateloved"),
    f("Album Date Last Played", K::Date, "albumlastplayed"),
    f("Album Date Rated", K::Date, "albumdaterated"),
    f("Album Is Favorite", K::Boolean, "albumloved"),
    f("Album Play Count", K::Number, "albumplaycount"),
    f("Album Rating", K::Number, "albumrating"),
    f("Album Type", K::String, "albumtype"),
    f("Album Version", K::String, "albumversion"),
    f("Arranger", K::String, "arranger"),
    f("Artist", K::String, "artist"),
    f("Artist Date Favorited", K::Date, "artistdateloved"),
    f("Artist Date Last Played", K::Date, "artistlastplayed"),
    f("Artist Date Rated", K::Date, "artistdaterated"),
    f("Artist Is Favorite", K::Boolean, "artistloved"),
    f("Artist Play Count", K::Number, "artistplaycount"),
    f("Artist Rating", K::Number, "artistrating"),
    f("Artists", K::String, "artists"),
    f("ASIN", K::String, "asin"),
    f("Average Rating", K::Number, "averagerating"),
    f("Barcode", K::String, "barcode"),
    f("Bit Depth", K::Number, "bitdepth"),
    f("Bitrate", K::Number, "bitrate"),
    f("BPM", K::Number, "bpm"),
    f("Catalog Number", K::String, "catalognumber"),
    f("Channels", K::Number, "channels"),
    f("Comment", K::String, "comment"),
    f("Composer", K::String, "composer"),
    f("Codec", K::String, "codec"),
    f("Conductor", K::String, "conductor"),
    f("Copyright", K::String, "copyright"),
    f("Date Added", K::Date, "dateadded"),
    f("Date Favorited", K::Date, "dateloved"),
    f("Date Last Played", K::Date, "lastplayed"),
    f("Date Modified", K::Date, "datemodified"),
    f("DJ Mixer", K::String, "djmixer"),
    f("Director", K::String, "director"),
    f("Disc Number", K::Number, "discnumber"),
    f("Disc Subtitle", K::String, "discsubtitle"),
    f("Disc Total", K::Number, "disctotal"),
    f("Duration", K::Number, "duration"),
    f("Encoded By", K::String, "encodedby"),
    f("Encoder Settings", K::String, "encodersettings"),
    f("Engineer", K::String, "engineer"),
    f("Explicit Status", K::String, "explicitstatus"),
    f("File Path", K::String, "filepath"),
    f("File Type", K::String, "filetype"),
    f("Genre", K::String, "genre"),
    f("Grouping", K::String, "grouping"),
    f("Has CoverArt", K::Boolean, "hascoverart"),
    f("Is Compilation", K::Boolean, "compilation"),
    f("Is Favorite", K::Boolean, "loved"),
    f("ISRC", K::String, "isrc"),
    f("Key", K::String, "key"),
    f("Language", K::String, "language"),
    f("License", K::String, "license"),
    f("Library Id", K::String, "library_id"),
    f("Lyricist", K::String, "lyricist"),
    f("Lyrics", K::String, "lyrics"),
    f("Media", K::String, "media"),
    f("Missing", K::Boolean, "missing"),
    f("Mixer", K::String, "mixer"),
    f("Mood", K::String, "mood"),
    f("Movement", K::String, "movement"),
    f("Movement Name", K::String, "movementname"),
    f("Movement Total", K::Number, "movementtotal"),
    f("MusicBrainz Album Artist Id", K::String, "mbz_album_artist_id"),
    f("MusicBrainz Album Id", K::String, "mbz_album_id"),
    f("MusicBrainz Artist Id", K::String, "mbz_artist_id"),
    f("MusicBrainz Arranger Id", K::String, "musicbrainz_arrangerid"),
    f("MusicBrainz Composer Id", K::String, "musicbrainz_composerid"),
    f("MusicBrainz Conductor Id", K::String, "musicbrainz_conductorid"),
    f("MusicBrainz Director Id", K::String, "musicbrainz_directorid"),
    f("MusicBrainz Disc Id", K::String, "musicbrainz_discid"),
    f("MusicBrainz DJ Mixer Id", K::String, "musicbrainz_djmixerid"),
    f("MusicBrainz Engineer Id", K::String, "musicbrainz_engineerid"),
    f("MusicBrainz Lyricist Id", K::String, "musicbrainz_lyricistid"),
    f("MusicBrainz Mixer Id", K::String, "musicbrainz_mixerid"),
    f("MusicBrainz Performer Id", K::String, "musicbrainz_performerid"),
    f("MusicBrainz Producer Id", K::String, "musicbrainz_producerid"),
    f("MusicBrainz Recording Id", K::String, "mbz_recording_id"),
    f("MusicBrainz Release Group Id", K::String, "mbz_release_group_id"),
    f("MusicBrainz Release Track Id", K::String, "mbz_release_track_id"),
    f("MusicBrainz Remixer Id", K::String, "musicbrainz_remixerid"),
    f("MusicBrainz Track Id", K::String, "musicbrainz_trackid"),
    f("MusicBrainz Work Id", K::String, "musicbrainz_workid"),
    f("Name", K::String, "title"),
    f("Original Date", K::Date, "originaldate"),
    f("Original Year", K::Number, "originalyear"),
    f("Performer", K::String, "performer"),
    f("Play Count", K::Number, "playcount"),
    f("Playlist", K::Playlist, "id"),
    f("Producer", K::String, "producer"),
    f("R128 Album Gain", K::Number, "r128_album_gain"),
    f("R128 Track Gain", K::Number, "r128_track_gain"),
    f("Rating", K::Number, "rating"),
    f("Record Label", K::String, "recordlabel"),
    f("Recording Date", K::Date, "recordingdate"),
    f("Release Country", K::String, "releasecountry"),
    f("Release Date", K::Date, "releasedate"),
    f("Release Status", K::String, "releasestatus"),
    f("Release Type", K::String, "releasetype"),
    f("ReplayGain Album Gain", K::Number, "replaygain_album_gain"),
    f("ReplayGain Album Peak", K::Number, "replaygain_album_peak"),
    f("ReplayGain Track Gain", K::Number, "replaygain_track_gain"),
    f("ReplayGain Track Peak", K::Number, "replaygain_track_peak"),
    f("Remixer", K::String, "remixer"),
    f("Script", K::String, "script"),
    f("Sample Rate", K::Number, "samplerate"),
    f("Size", K::Number, "size"),
    f("Sort Album", K::String, "sortalbum"),
    f("Sort Album Artist", K::String, "sortalbumartist"),
    f("Sort Album Artists", K::String, "albumartistssort"),
    f("Sort Artist", K::String, "sortartist"),
    f("Sort Artists", K::String, "artistssort"),
    f("Sort Composer", K::String, "composersort"),
    f("Sort Lyricist", K::String, "lyricistsort"),
    f("Sort Name", K::String, "sorttitle"),
    f("Subtitle", K::String, "subtitle"),
    f("Track Number", K::Number, "tracknumber"),
    f("Track Total", K::Number, "tracktotal"),
    f("Website", K::String, "website"),
    f("Work", K::String, "work"),
    f("Year", K::Number, "year"),
];

pub fn find(key: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.key == key)
}

/// An operator as the builder lists it. `beforeDate`, `afterDate` and `inTheRangeDate` are
/// the builder's own ids for the date versions of before / after / in the range (the
/// rules JSON has `before`, `after`, `inTheRange`); [`api_operator`] maps them back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpDef {
    pub id: &'static str,
    pub label: &'static str,
}

impl std::fmt::Display for OpDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

const fn o(id: &'static str, label: &'static str) -> OpDef {
    OpDef { id, label }
}

const IS: OpDef = o("is", "Is");
const IS_NOT: OpDef = o("isNot", "Is not");
const MISSING: OpDef = o("isMissing", "Is missing");
const PRESENT: OpDef = o("isPresent", "Is present");
const CONTAINS: OpDef = o("contains", "Contains");
const NOT_CONTAINS: OpDef = o("notContains", "Does not contain");
const RANGE: OpDef = o("inTheRange", "Is in the range");

pub static STRING_OPS: &[OpDef] = &[
    IS,
    IS_NOT,
    MISSING,
    PRESENT,
    CONTAINS,
    NOT_CONTAINS,
    o("startsWith", "Starts with"),
    o("endsWith", "Ends with"),
];
pub static NUMBER_OPS: &[OpDef] = &[
    IS,
    IS_NOT,
    MISSING,
    PRESENT,
    CONTAINS,
    NOT_CONTAINS,
    o("gt", "Is greater than"),
    o("lt", "Is less than"),
    RANGE,
];
pub static BOOLEAN_OPS: &[OpDef] = &[IS, IS_NOT, MISSING, PRESENT];
pub static DATE_OPS: &[OpDef] = &[
    IS,
    IS_NOT,
    MISSING,
    PRESENT,
    o("before", "Is before"),
    o("after", "Is after"),
    o("inTheLast", "Is in the last"),
    o("notInTheLast", "Is not in the last"),
    RANGE,
    o("beforeDate", "Is before (date)"),
    o("afterDate", "Is after (date)"),
    o("inTheRangeDate", "Is in the range (date)"),
];
pub static PLAYLIST_OPS: &[OpDef] = &[o("inPlaylist", "Is in"), o("notInPlaylist", "Is not in")];

pub fn operators(kind: Kind) -> &'static [OpDef] {
    match kind {
        K::String => STRING_OPS,
        K::Number => NUMBER_OPS,
        K::Boolean => BOOLEAN_OPS,
        K::Date => DATE_OPS,
        K::Playlist => PLAYLIST_OPS,
    }
}

/// Feishin `mapDatePickerOperatorToApi`.
pub fn api_operator(id: &str) -> &str {
    match id {
        "beforeDate" => "before",
        "afterDate" => "after",
        "inTheRangeDate" => "inTheRange",
        other => other,
    }
}
