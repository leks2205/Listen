//! Favorites page: Tracks, Albums and Artists tabs (YouTube Music: liked songs, saved albums,
//! subscribed artists). Sources that can't favourite albums and artists show tracks only.

use super::item_controls::tabs;
use super::list::ListView;
use super::{icon, label, plural};
use crate::app::library_extra::{ExtraMsg, Load};
use crate::app::list::{FavoritesTab, ListId};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::widget::{button, row};
use iced::{Alignment, Element, Padding};
use listen_core::model::SourceKind;
use listen_source::Capabilities;

/// The tab labels: Feishin's, with YouTube Music's own words.
pub fn tab_label(tab: FavoritesTab, ytm: bool) -> &'static str {
    match (tab, ytm) {
        (FavoritesTab::Songs, false) => "Tracks",
        (FavoritesTab::Songs, true) => "Liked Music",
        (FavoritesTab::Albums, false) => "Albums",
        (FavoritesTab::Albums, true) => "Saved albums",
        (FavoritesTab::Artists, false) => "Artists",
        (FavoritesTab::Artists, true) => "Subscriptions",
    }
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let x = &state.extra;
    let source = state.source.as_ref();
    let ytm = source.is_some_and(|s| s.kind() == SourceKind::YouTubeMusic);
    let items = state.demo || source.is_some_and(|s| s.capabilities().contains(Capabilities::FAVORITE_ITEMS));
    let tab = if items { x.fav_tab } else { FavoritesTab::Songs };
    let id = ListId::Favorites(tab);

    let mut extra = row![].spacing(metrics::space_md()).align_y(Alignment::Center);
    if items {
        let segments = [FavoritesTab::Songs, FavoritesTab::Albums, FavoritesTab::Artists]
            .into_iter()
            .map(|c| (tab_label(c, ytm).to_string(), c == tab, Message::Extra(ExtraMsg::FavTab(c))))
            .collect();
        extra = extra.push(tabs(state, segments));
    }
    extra = extra.push(
        button(
            row![
                icon(Icon::LuPlay, metrics::font_md(), t.foreground),
                label(state.play_behavior().button_label(), metrics::font_md(), t.foreground)
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([6.0, 14.0]))
        .style(style::primary_button(t))
        .on_press(Message::Extra(ExtraMsg::FavPlay(state.play_behavior()))),
    );

    let empty = if ytm { "Nothing liked yet." } else { "No favorites yet." };
    match tab {
        FavoritesTab::Songs => {
            let songs = &state.library.favorites;
            ListView::new(state, id, songs)
                .title("Favorites")
                .subtitle(plural(songs.len(), "track"))
                .loading(songs.is_empty() && !state.library.favorites_loaded)
                .extra(extra)
                .empty(empty)
                .view()
        }
        FavoritesTab::Albums => ListView::new(state, id, &x.fav_albums)
            .title("Favorites")
            .subtitle(plural(x.fav_albums.len(), "album"))
            .loading(x.fav_albums_load == Load::Loading && x.fav_albums.is_empty())
            .extra(extra)
            .empty(empty)
            .view(),
        FavoritesTab::Artists => ListView::new(state, id, &x.fav_artists)
            .title("Favorites")
            .subtitle(plural(x.fav_artists.len(), "artist"))
            .loading(x.fav_artists_load == Load::Loading && x.fav_artists.is_empty())
            .extra(extra)
            .empty(empty)
            .view(),
    }
}
