//! The preview footer of the main page (Feishin: context-menu-preview): the
//! cover of the first targeted item with its name, or "N items".

use super::boxed_width;
use crate::app::Listen;
use crate::app::Message;
use crate::app::menu::{MenuTarget, TargetKind};
use crate::icons::Icon;
use crate::style::metrics;
use crate::ui::{cover_on, label, label_w};
use iced::font::Weight;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::{ImageRef, ServerId};

/// Height of the footer including its divider.
pub const HEIGHT: f32 = 1.0 + 8.0 + 36.0 + 8.0;

/// What the footer says about a target.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// The first item's name.
    pub title: String,
    /// One item: its artist (songs, albums); several: "+2 more".
    pub subtitle: Option<String>,
    /// The first item's cover.
    pub image: Option<ImageRef>,
    pub server: Option<ServerId>,
    /// The first item's album id (demo cover colour).
    pub album_id: Option<String>,
    pub glyph: Icon,
}

pub fn preview(target: &MenuTarget) -> Preview {
    let n = target.len();
    let (image, server, album_id, artist) = match target {
        MenuTarget::Songs { songs, .. } => {
            let s = songs.first();
            (
                s.and_then(|s| s.image.clone()),
                s.map(|s| s.server_id.clone()),
                s.and_then(|s| s.album_id.clone()),
                s.map(|s| s.artist_line()),
            )
        }
        MenuTarget::Albums(v) => {
            let a = v.first();
            (
                a.and_then(|a| a.image.clone()),
                a.map(|a| a.server_id.clone()),
                a.map(|a| a.id.clone()),
                a.map(|a| listen_core::model::join_names(&a.album_artists)),
            )
        }
        MenuTarget::Artists(v) => {
            let a = v.first();
            (a.and_then(|a| a.image.clone()), a.map(|a| a.server_id.clone()), None, None)
        }
        MenuTarget::Playlists(v) => {
            let p = v.first();
            (
                p.and_then(|p| p.image.clone()),
                p.map(|p| p.server_id.clone()),
                None,
                p.and_then(|p| p.owner.clone()),
            )
        }
        MenuTarget::Genres(_) | MenuTarget::Folders(_) => (None, None, None, None),
    };
    let first = target.first_name();
    // Feishin: the first item's name, with "+N more" under it for several.
    let (title, subtitle) = if n > 1 {
        (first, Some(format!("+{} more", n - 1)))
    } else {
        (first, artist.filter(|a| !a.is_empty()))
    };
    let glyph = match target.kind() {
        TargetKind::Song => Icon::LuMusic,
        TargetKind::Album => Icon::LuDisc3,
        TargetKind::Artist => Icon::LuUser,
        TargetKind::Playlist => Icon::LuListMusic,
        TargetKind::Genre => Icon::LuMusic,
        TargetKind::Folder => Icon::RiFolder3Line,
    };
    Preview { title, subtitle, image, server, album_id, glyph }
}

pub fn view<'a>(state: &'a Listen, target: &MenuTarget) -> Element<'a, Message> {
    let t = state.tokens;
    let p = preview(target);
    let art: Element<'a, Message> = if state.demo {
        crate::ui::art(crate::demo::color_for_album(p.album_id.as_deref()), 36.0, 4.0, p.glyph)
    } else if let Some(server) = &p.server {
        cover_on(state, server, p.image.as_ref(), 36.0, 4.0, p.glyph)
    } else {
        crate::ui::art((90, 90, 110), 36.0, 4.0, p.glyph)
    };
    let width = boxed_width() - 8.0 - 2.0 * metrics::space_md() - 36.0 - metrics::space_md();
    let mut text = column![label_w(
        crate::ui::ellipsize_px(&p.title, width, metrics::font_sm()),
        metrics::font_sm(),
        t.foreground,
        Weight::Semibold
    )];
    if let Some(sub) = &p.subtitle {
        text = text.push(label(
            crate::ui::ellipsize_px(sub, width, metrics::font_xs()),
            metrics::font_xs(),
            t.muted,
        ));
    }
    column![
        crate::ui::divider_h(t),
        container(row![art, text.spacing(2)].spacing(metrics::space_md()).align_y(Alignment::Center))
            .padding(Padding::from([8.0, metrics::space_md()]))
            .width(Length::Fill)
            .height(HEIGHT - 1.0),
    ]
    .height(HEIGHT)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn one_item_shows_its_name_and_artist() {
        let song = demo::album_songs(0).remove(0);
        let p = preview(&MenuTarget::song(&song));
        assert_eq!(p.title, song.title);
        assert_eq!(p.subtitle.as_deref(), Some(song.artist_line().as_str()));
    }

    #[test]
    fn several_items_show_the_first_name_and_how_many_more() {
        let songs = demo::album_songs(0);
        let p = preview(&MenuTarget::songs(&songs[..3]));
        assert_eq!(p.title, songs[0].title);
        assert_eq!(p.subtitle.as_deref(), Some("+2 more"));
        let albums = demo::albums()[..2].to_vec();
        assert_eq!(preview(&MenuTarget::Albums(albums.clone())).title, albums[0].name);
        assert_eq!(preview(&MenuTarget::Albums(albums)).subtitle.as_deref(), Some("+1 more"));
    }
}
