//! General tab: the artist page (sections, release-type groups, radio size)
//! and the links to other sites. Labels follow Feishin's.

use super::{line, section};
use crate::app::artist_page::{ArtistSetting, LinkSite};
use crate::app::{Listen, Message, Setting};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label};
use iced::widget::{button, checkbox, row, slider, toggler};
use iced::{Alignment, Element, Padding};
use listen_core::settings::{ArtistSection, RELEASE_TYPES, SortableItem};

fn change(c: ArtistSetting) -> Message {
    Message::Setting(Setting::Artist(c))
}

/// One reorderable row: a checkbox (or the label alone) and the up / down arrows.
fn item_row<'a>(
    state: &Listen,
    name: &'static str,
    enabled: Option<bool>,
    on_toggle: impl Fn(bool) -> ArtistSetting + 'a,
    up: Option<ArtistSetting>,
    down: Option<ArtistSetting>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let arrow = |glyph: Icon, to: Option<ArtistSetting>| {
        crate::ui::tip::tip_icon(
            state,
            button(icon(glyph, metrics::font_md(), t.muted))
                .padding(4)
                .style(style::icon_button(t, false))
                .on_press_maybe(to.map(change)),
            glyph,
            crate::ui::tip::Position::Left,
        )
    };
    let name_cell: Element<'a, Message> = match enabled {
        Some(on) => checkbox(on)
            .label(name)
            .on_toggle(move |v| change(on_toggle(v)))
            .size(18)
            .text_size(metrics::font_md())
            .into(),
        None => label(name, metrics::font_md(), t.foreground).into(),
    };
    row![
        name_cell,
        iced::widget::space().width(iced::Length::Fill),
        arrow(Icon::LuChevronUp, up),
        arrow(Icon::LuChevronDown, down)
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .padding(Padding::from([0.0, 8.0]))
    .into()
}

fn list_rows<'a>(
    state: &Listen,
    items: &'a [SortableItem],
    name_of: impl Fn(&str) -> Option<&'static str>,
    always_on: &[&str],
    toggle: fn(usize, bool) -> ArtistSetting,
    mv: fn(usize, i8) -> ArtistSetting,
) -> Vec<Element<'a, Message>> {
    let last = items.len().saturating_sub(1);
    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let name = name_of(&item.id)?;
            let enabled = (!always_on.contains(&item.id.as_str())).then_some(item.enabled);
            Some(item_row(
                state,
                name,
                enabled,
                move |v| toggle(i, v),
                (i > 0).then_some(mv(i, -1)),
                (i < last).then_some(mv(i, 1)),
            ))
        })
        .collect()
}

pub(super) fn artist_page(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let a = &state.config.settings.artist;
    let mut rows = vec![line(
        state,
        "Artist page configuration",
        "Select the items and order in which they appear on the artist page",
        row![].into(),
    )];
    rows.extend(list_rows(
        state,
        &a.items,
        |id| ArtistSection::from_id(id).map(ArtistSection::label),
        &["recentAlbums"],
        ArtistSetting::SectionEnabled,
        ArtistSetting::MoveSection,
    ));
    rows.push(line(
        state,
        "Artist release type configuration",
        "Select the release types and the order in which they are grouped on the artist page",
        row![].into(),
    ));
    rows.extend(list_rows(
        state,
        &a.release_types,
        |id| RELEASE_TYPES.iter().find(|(i, _, _)| *i == id).map(|(_, _, label)| *label),
        &[],
        ArtistSetting::ReleaseEnabled,
        ArtistSetting::MoveRelease,
    ));
    rows.push(line(
        state,
        "Artist radio count",
        "The number of songs an artist radio queues",
        row![
            slider(1.0..=100.0, a.radio_count as f32, |v| change(
                ArtistSetting::RadioCount(v.round() as u32)
            ))
            .width(160),
            label(a.radio_count.to_string(), metrics::font_md(), t.muted),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .into(),
    ));
    section(state, "Artist page", rows)
}

fn site<'a>(
    state: &'a Listen,
    name: &'static str,
    hint: &'static str,
    value: bool,
    which: LinkSite,
) -> Element<'a, Message> {
    line(
        state,
        name,
        hint,
        toggler(value).on_toggle(move |v| change(ArtistSetting::Link(which, v))).size(20).into(),
    )
}

pub(super) fn external_links(state: &Listen) -> Element<'_, Message> {
    let l = &state.config.settings.artist.external_links;
    let mut rows = vec![site(
        state,
        "External links",
        "Show links to other sites on artist and album pages",
        l.enabled,
        LinkSite::All,
    )];
    if l.enabled {
        rows.extend([
            site(
                state,
                "Last.fm",
                "Show a link to the artist or album on Last.fm",
                l.last_fm,
                LinkSite::LastFm,
            ),
            site(
                state,
                "MusicBrainz",
                "Show a link on MusicBrainz (only where the server knows the MusicBrainz id)",
                l.music_brainz,
                LinkSite::MusicBrainz,
            ),
            site(
                state,
                "ListenBrainz",
                "Show a link on ListenBrainz",
                l.listen_brainz,
                LinkSite::ListenBrainz,
            ),
            site(state, "Qobuz", "Show a search link on Qobuz", l.qobuz, LinkSite::Qobuz),
            site(state, "Spotify", "Show a search link on Spotify", l.spotify, LinkSite::Spotify),
            site(
                state,
                "Open in the Spotify app",
                "Open the Spotify link in the desktop app instead of the web player",
                l.native_spotify,
                LinkSite::NativeSpotify,
            ),
        ]);
    }
    section(state, "External links", rows)
}
