//! The About tab: version, license, and credits for the projects and
//! services Listen builds on.

use super::{line, section};
use crate::app::album_page::AlbumPageMsg;
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::label_w;
use iced::font::Weight;
use iced::widget::button;
use iced::{Element, Padding};

pub const SOURCE_URL: &str = "https://github.com/leks2205/Listen";

/// (name, what Listen uses it for, website)
type Credit = (&'static str, &'static str, &'static str);

const DESIGN: &[Credit] = &[(
    "Feishin",
    "Listen's interface is closely modelled on Feishin by jeffvli and contributors. Its themes and UI \
     translations are used under the GPL-3.0.",
    "https://github.com/jeffvli/feishin",
)];

const SOURCES: &[Credit] = &[
    ("Navidrome", "Music server, through the OpenSubsonic API.", "https://www.navidrome.org"),
    ("OpenSubsonic", "The Subsonic API extensions Listen speaks.", "https://opensubsonic.netlify.app"),
    ("Jellyfin", "Media server.", "https://jellyfin.org"),
    (
        "YouTube Music",
        "Streaming with your own account. Listen is not affiliated with or endorsed by Google or YouTube; \
         YouTube Music is a trademark of Google LLC.",
        "https://music.youtube.com",
    ),
];

const SERVICES: &[Credit] = &[
    ("LRCLIB", "Synced lyrics.", "https://lrclib.net"),
    ("SimpMusic Lyrics", "Synced lyrics.", "https://github.com/maxrave-dev/SimpMusic"),
    ("NetEase Cloud Music", "Lyrics.", "https://music.163.com"),
    ("Genius", "Lyrics search.", "https://genius.com"),
    (
        "Microsoft Azure Translator / Google Cloud Translation",
        "Lyrics translation with your own key.",
        "https://learn.microsoft.com/azure/ai-services/translator/",
    ),
    ("Google Gemini API", "The AI DJ, with your own API key.", "https://ai.google.dev"),
    ("MusicBrainz", "Tags and release data for CD rips.", "https://musicbrainz.org"),
    ("Cover Art Archive", "Album covers for CD rips.", "https://coverartarchive.org"),
    (
        "iTunes Search API",
        "Fallback album covers for CD rips.",
        "https://performance-partners.apple.com/search-api",
    ),
    ("AccurateRip", "Rip verification (through whipper).", "http://www.accuraterip.com"),
    ("Last.fm", "Cover lookup and links for the Discord status.", "https://www.last.fm"),
    ("ListenBrainz", "Links to artists and albums.", "https://listenbrainz.org"),
    ("Discord", "Rich Presence status.", "https://discord.com"),
    ("GitHub", "Release downloads for the updater.", SOURCE_URL),
];

const SOFTWARE: &[Credit] = &[
    ("mpv", "Audio and video playback.", "https://mpv.io"),
    ("yt-dlp", "Resolves YouTube Music streams.", "https://github.com/yt-dlp/yt-dlp"),
    ("Deno", "JavaScript runtime yt-dlp uses for YouTube.", "https://deno.com"),
    ("ytmapi-rs", "YouTube Music API client.", "https://github.com/nick42d/youtui"),
    ("iced", "The GUI toolkit.", "https://iced.rs"),
    (
        "projectM",
        "Milkdrop visualizer (loaded when installed).",
        "https://github.com/projectM-visualizer/projectm",
    ),
    (
        "Beat This! and beat-this-rs",
        "Beat and downbeat tracking for DJ transitions (CPJKU model, Rust port by danigb).",
        "https://github.com/CPJKU/beat_this",
    ),
    ("bliss", "Timbre and mood features for song ordering.", "https://github.com/Polochon-street/bliss-rs"),
    ("whipper", "Secure CD ripping.", "https://github.com/whipper-team/whipper"),
    ("Inter and Poppins", "Fonts, under the SIL Open Font License.", "https://rsms.me/inter/"),
    ("Lucide, Remix Icon, Simple Icons", "Icons.", "https://lucide.dev"),
];

fn link<'a>(state: &'a Listen, url: &'static str) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w("Website", metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(Padding::from([8.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::AlbumPage(AlbumPageMsg::OpenUrl(url.into())))
        .into()
}

fn credits<'a>(state: &'a Listen, title: &'static str, list: &'static [Credit]) -> Element<'a, Message> {
    let rows = list.iter().map(|&(name, what, url)| line(state, name, what, link(state, url))).collect();
    section(state, title, rows)
}

pub(super) fn about(state: &Listen) -> Vec<Element<'_, Message>> {
    let rows = vec![
        line(
            state,
            "Listen",
            format!("Version {}. A native desktop music player.", env!("CARGO_PKG_VERSION")),
            link(state, SOURCE_URL),
        ),
        line(
            state,
            "License",
            "GNU Affero General Public License v3.0 or later. You can use, change and share Listen; \
             changed versions must stay under the same license.",
            link(state, "https://www.gnu.org/licenses/agpl-3.0.html"),
        ),
    ];
    vec![
        section(state, "About Listen", rows),
        credits(state, "Design", DESIGN),
        credits(state, "Music sources", SOURCES),
        credits(state, "Online services", SERVICES),
        credits(state, "Software, fonts and icons", SOFTWARE),
    ]
}
