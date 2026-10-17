//! Main content area: routes to the page for the current sidebar entry.

use super::{label, label_w};
use crate::app::{Listen, Message, Route};
use crate::style::metrics;
use iced::font::Weight;
use iced::widget::{column, container};
use iced::{Element, Length, Padding};

pub fn view(state: &Listen) -> Element<'_, Message> {
    let body: Element<'_, Message> = match &state.route {
        Route::Home => super::home::view(state),
        Route::Albums => super::albums::view(state),
        Route::Album(_) => super::album_detail::view(state),
        Route::Tracks => super::songs::view(state),
        Route::AlbumArtists => super::artists::view(state, crate::app::list::ListId::AlbumArtists),
        Route::Artists => super::artists::view(state, crate::app::list::ListId::Artists),
        Route::Artist(_) => super::artist_detail::view(state),
        Route::ArtistList(id, view) => super::artist_detail::sub_page(state, id, *view),
        Route::Playlist(_) => super::playlist_page::view(state),
        Route::Playlists => super::playlists::view(state),
        Route::Favorites => super::favorites::view(state),
        Route::Search => super::search::view(state),
        Route::Recent => super::history::recent(state),
        Route::Stats => super::history::stats(state),
        Route::Mixes => super::history::mixes(state),
        Route::Mix(kind) => super::history::mix(state, *kind),
        Route::Genres => super::browse::genres(state),
        Route::Genre(_) => super::browse::genre(state),
        Route::Folders | Route::Folder(_) => super::browse::folder(state),
        Route::Settings => super::settings::view(state),
        Route::Downloads => super::downloads::view(state),
        Route::Radio => super::radio::view(state),
        Route::NowPlaying => super::queue::page(state),
        // Pages not built yet show a stub.
        #[allow(unreachable_patterns)]
        route => placeholder(state, route),
    };
    let mut page = column![container(body).width(Length::Fill).height(Length::Fill)];
    if let Some(rips) = super::rip_panel::view(state) {
        page = page.push(rips);
    }
    page.into()
}

#[allow(dead_code)]
fn placeholder<'a>(state: &'a Listen, route: &'a Route) -> Element<'a, Message> {
    let t = state.tokens;
    container(
        column![
            label_w(route.title(), 36.0, t.foreground, Weight::Bold),
            label("This page is part of an upcoming milestone.", metrics::font_md(), t.muted),
        ]
        .spacing(metrics::space_md()),
    )
    .padding(Padding {
        top: 48.0,
        right: metrics::PAGE_PADDING_X,
        bottom: 0.0,
        left: metrics::PAGE_PADDING_X,
    })
    .into()
}
