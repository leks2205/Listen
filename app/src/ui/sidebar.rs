//! Left sidebar: action bar (search + menu/back/forward) and the accordion of
//! My Library, Collections, Playlists and Shared playlists; the collapsed
//! icon rail. Feishin: features/sidebar/components/{sidebar,collapsed-sidebar,
//! sidebar-playlist-list,playlist-folder-tree,sidebar-collection-list}.tsx.

use super::list::anchor::anchored;
use super::{icon, label, label_w};
use crate::app::sidebar_state::{DragKind, Node, Scope, Section, SidebarMsg, at_path};
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, mouse_area, row, scrollable, space};
use iced::{Alignment, Color, Element, Length, Padding, Point};
use listen_core::model::Playlist;
use listen_core::settings::{CollectionList, FolderView, PlaylistMode};

pub use super::app_menu::overlay;

fn nav_icons(id: &str) -> Option<(Icon, Icon)> {
    Some(match id {
        "Home" => (Icon::RiHome6Line, Icon::RiHome6Fill),
        "Favorites" => (Icon::RiHeartLine, Icon::RiHeartFill),
        "Albums" => (Icon::RiAlbumLine, Icon::RiAlbumFill),
        "Tracks" => (Icon::RiMusic2Line, Icon::RiMusic2Fill),
        "Artists" | "Artists-all" => (Icon::RiUserVoiceLine, Icon::RiUserVoiceFill),
        "Genres" => (Icon::RiFlag2Line, Icon::RiFlag2Fill),
        "Folders" => (Icon::RiFolder3Line, Icon::RiFolder3Fill),
        "Playlists" => (Icon::RiPlayListLine, Icon::RiPlayListFill),
        "Now Playing" => (Icon::RiPlayLine, Icon::RiPlayFill),
        "Search" => (Icon::RiSearchLine, Icon::RiSearchFill),
        "Recently Played" => (Icon::LuTimer, Icon::LuTimer),
        "Statistics" => (Icon::LuActivity, Icon::LuActivity),
        "Mixes" => (Icon::LuShuffle, Icon::LuShuffle),
        "Radio" => (Icon::RiRadioLine, Icon::RiRadioFill),
        "Settings" => (Icon::RiSettings2Line, Icon::RiSettings2Fill),
        "Collections" => (Icon::LuBookmark, Icon::LuBookmark),
        _ => return None,
    })
}

/// Feishin shows item labels in title case ("album artists" → "Album Artists").
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Id of the search box (focused by the search hotkeys).
pub const SEARCH_ID: &str = "sidebar-search";

/// Width of the collapsed rail.
pub const RAIL_WIDTH: f32 = 84.0;

/// Width of the compact layout's sidebar drawer (Feishin: 320 px).
pub const DRAWER_WIDTH: f32 = 320.0;

/// The pages the sidebar links to, in the order and with the visibility set
/// in the settings. Collections has no page of its own.
fn visible_items(
    state: &Listen,
) -> impl Iterator<Item = (&listen_core::settings::SidebarItem, Option<Route>)> {
    state.config.settings.sidebar.items.iter().filter(|i| i.visible).filter_map(move |item| {
        if item.id == "Collections" {
            return Some((item, None));
        }
        let route = Route::from_sidebar_id(&item.id)?;
        nav_icons(&item.id)?;
        state.supports_route(&route).then_some((item, Some(route)))
    })
}

fn collections(state: &Listen) -> &[listen_core::settings::Collection] {
    &state.config.settings.lists.collections
}

/// The collapsed rail's side buttons, like Feishin's `CollapsedSidebarItem`.
fn rail_item<'a>(
    state: &Listen,
    glyph: Icon,
    text: String,
    active: bool,
    msg: Message,
) -> iced::widget::Button<'a, Message> {
    let t = state.tokens;
    let color = if active { t.primary } else { t.foreground };
    button(
        column![
            icon(glyph, metrics::font_lg() + 4.0, color),
            super::text_in(text, super::font(Weight::Medium))
                .size(metrics::font_xs())
                .color(color)
                .align_x(iced::alignment::Horizontal::Center)
                .width(Length::Fill),
        ]
        .spacing(4)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding::from([metrics::space_sm(), 2.0]))
    .style(style::nav_item(t, active))
    .on_press(msg)
}

/// Collapsed sidebar, like Feishin's: back/forward, then icons with small
/// labels, the Menu button opens the app menu.
fn rail(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let sb = &state.config.settings.sidebar;
    let arrow = |glyph: Icon, msg: Option<Message>, name: &str, action: &str| {
        let color = if msg.is_some() { t.foreground } else { t.muted };
        super::tip::tip(
            state,
            button(container(icon(glyph, metrics::font_lg(), color)).center(Length::Fill))
                .width(Length::Fill)
                .height(metrics::CONTROL)
                .padding(0)
                .style(style::nav_item(t, false))
                .on_press_maybe(msg),
            name,
            Some(action),
            super::tip::Position::Right,
        )
    };

    let mut rail =
        Column::new().spacing(metrics::space_xs()).padding(Padding::from([0.0, metrics::space_xs()]));
    if sb.collapsed_navigation {
        rail = rail.push(
            container(
                row![
                    arrow(
                        Icon::LuChevronLeft,
                        state.can_go_back().then_some(Message::Back),
                        "Back",
                        "browserBack"
                    ),
                    arrow(
                        Icon::LuChevronRight,
                        state.can_go_forward().then_some(Message::Forward),
                        "Forward",
                        "browserForward"
                    ),
                ]
                .spacing(2),
            )
            .height(metrics::ACTION_BAR_HEIGHT)
            .center_y(metrics::ACTION_BAR_HEIGHT),
        );
    } else {
        rail = rail.push(space().height(metrics::space_sm()));
    }
    rail = rail.push(anchored(rail_item(state, Icon::LuMenu, "Menu".into(), false, Message::Noop), |b| {
        Message::Sidebar(SidebarMsg::OpenAppMenu(Point::new(b.x + b.width + 4.0, b.y)))
    }));

    let mut shown_search = false;
    let mut shown_settings = false;
    for (entry, route) in visible_items(state) {
        let (Some((line, fill)), name) = (nav_icons(&entry.id), title_case(&entry.label)) else { continue };
        let Some(route) = route else {
            // Collections: a menu of the saved queries.
            if !collections(state).is_empty() {
                rail = rail.push(anchored(rail_item(state, line, name, false, Message::Noop), |b| {
                    Message::Sidebar(SidebarMsg::OpenRailCollections(Point::new(b.x + b.width + 4.0, b.y)))
                }));
            }
            continue;
        };
        shown_search |= route == Route::Search;
        shown_settings |= route == Route::Settings;
        let active = state.route == route;
        rail = rail.push(rail_item(
            state,
            if active { fill } else { line },
            name,
            active,
            Message::Navigate(route),
        ));
    }
    let downloads_active = state.route == Route::Downloads;
    rail = rail.push(rail_item(
        state,
        Icon::LuDownload,
        "Downloads".into(),
        downloads_active,
        Message::Navigate(Route::Downloads),
    ));
    if state.rip_available() && !state.rip.active() {
        rail = rail.push(rail_item(
            state,
            Icon::LuDisc3,
            "Rip CD".into(),
            false,
            Message::Rip(crate::app::rip::RipMsg::RipNow(None)),
        ));
    }
    if !shown_search {
        let active = state.route == Route::Search;
        rail = rail.push(rail_item(
            state,
            if active { Icon::RiSearchFill } else { Icon::RiSearchLine },
            "Search".into(),
            active,
            Message::Navigate(Route::Search),
        ));
    }
    if !shown_settings {
        let active = state.route == Route::Settings;
        rail = rail.push(rail_item(
            state,
            if active { Icon::RiSettings2Fill } else { Icon::RiSettings2Line },
            "Settings".into(),
            active,
            Message::Navigate(Route::Settings),
        ));
    }
    if sb.show_playlists {
        rail = rail.push(rail_playlists(state));
    }

    container(scrollable(rail).style(style::scrollbar(t)).height(Length::Fill))
        .width(RAIL_WIDTH)
        .height(Length::Fill)
        .style(style::sidebar(t))
        .into()
}

/// The playlists under the rail's items: just their covers, mine first.
fn rail_playlists(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let folded = state.sb.rail_playlists_folded;
    let mut col = Column::new().spacing(metrics::space_xs()).align_x(Alignment::Center);
    col = col.push(
        container(super::divider_h(t)).padding(Padding::from([metrics::space_xs(), metrics::space_md()])),
    );
    col = col.push(
        button(
            crate::ui::t_text("Playlists")
                .font(super::font(Weight::Semibold))
                .size(metrics::font_xs())
                .color(t.muted)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .width(Length::Fill)
        .padding(2)
        .style(style::nav_item(t, false))
        .on_press(Message::Navigate(Route::Playlists)),
    );
    let tool = |glyph: Icon, msg: Message, name: &str| {
        super::tip::tip(
            state,
            button(icon(glyph, metrics::font_md(), t.muted))
                .padding(3)
                .style(style::icon_button(t, false))
                .on_press(msg),
            name,
            None,
            super::tip::Position::Right,
        )
    };
    let mut tools = row![tool(
        if folded { Icon::LuChevronDown } else { Icon::LuChevronUp },
        Message::Sidebar(SidebarMsg::ToggleRailPlaylists),
        if folded { "Show playlists" } else { "Hide playlists" }
    )]
    .spacing(2);
    if state.can_create_playlist() {
        tools = tools.push(tool(
            Icon::LuPlus,
            Message::PlaylistPage(crate::app::playlists::PlaylistPageMsg::Create(Vec::new())),
            "Create playlist",
        ));
    }
    col = col.push(tools);
    if !folded {
        let owned = state.sidebar_playlists(Scope::Owned);
        let shared = state.sidebar_playlists(Scope::Shared);
        let divide = !owned.is_empty() && !shared.is_empty();
        for p in owned {
            col = col.push(rail_playlist(state, p));
        }
        if divide {
            col = col.push(
                container(super::divider_h(t))
                    .padding(Padding::from([metrics::space_xs(), metrics::space_md()])),
            );
        }
        for p in shared {
            col = col.push(rail_playlist(state, p));
        }
    }
    col.width(Length::Fill).into()
}

fn rail_playlist<'a>(state: &Listen, p: &'a Playlist) -> Element<'a, Message> {
    let t = state.tokens;
    let active = state.route == Route::Playlist(p.id.clone());
    super::with_menu(
        button(super::cover(state, p.image.as_ref(), 44.0, metrics::radius_md(), Icon::LuListMusic))
            .width(Length::Fill)
            .padding(Padding::from([3.0, 0.0]))
            .style(move |theme, status| {
                let mut s = style::nav_item(t, active)(theme, status);
                if active {
                    s.background = Some(iced::Background::Color(t.nav_hover));
                }
                s
            })
            .on_press(Message::Navigate(Route::Playlist(p.id.clone()))),
        crate::app::menu::MenuTarget::playlist(p),
    )
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    if state.config.settings.sidebar.collapsed {
        return rail(state);
    }
    expanded(state, state.config.settings.sidebar.width, false)
}

/// The full sidebar in the compact layout's drawer.
pub fn drawer(state: &Listen) -> Element<'_, Message> {
    expanded(state, DRAWER_WIDTH, true)
}

/// `#rrggbb` or `#rrggbbaa`; anything else is no colour.
pub fn parse_hex(s: &str) -> Option<Color> {
    let hex = s.trim().strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(if hex.len() == 6 {
        Color::from_rgb8((n >> 16) as u8, (n >> 8) as u8, n as u8)
    } else {
        Color::from_rgba8((n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, (n & 0xff) as f32 / 255.0)
    })
}

/// An accordion header: the title, with `extra` tools and the fold chevron at its right.
fn accordion<'a>(
    state: &Listen,
    section: Section,
    title: impl Into<String>,
    extra: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let open = state.sb.section_open(section);
    let toggle = Message::Sidebar(SidebarMsg::ToggleSection(section));
    let head = button(
        container(label_w(title.into(), metrics::font_md(), t.muted, Weight::Medium)).center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(40)
    .padding(Padding::from([0.0, metrics::space_md()]))
    .style(style::nav_item(t, false))
    .on_press(toggle.clone());
    let chevron = small_tool(
        state,
        if open { Icon::LuChevronDown } else { Icon::LuChevronRight },
        toggle,
        if open { "Collapse" } else { "Expand" },
    );
    let mut tools = row![].align_y(Alignment::Center);
    if let Some(extra) = extra {
        tools = tools.push(extra);
    }
    tools = tools.push(chevron);
    iced::widget::stack![
        head,
        container(tools)
            .width(Length::Fill)
            .height(40)
            .align_x(iced::alignment::Horizontal::Right)
            .center_y(40),
    ]
    .width(Length::Fill)
    .height(40)
    .into()
}

fn small_tool<'a>(state: &Listen, glyph: Icon, msg: Message, name: &str) -> Element<'a, Message> {
    let t = state.tokens;
    super::tip::tip(
        state,
        button(icon(glyph, metrics::font_lg(), t.muted))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press(msg),
        name,
        None,
        super::tip::Position::Bottom,
    )
}

fn nav_row<'a>(
    state: &Listen,
    glyph: Icon,
    text: String,
    active: bool,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let color = if active { t.primary } else { t.foreground };
    // The row is centred in the full button height: a button puts its content
    // at the top, leaving the hover box taller than the icon and text.
    button(
        container(
            row![
                icon(glyph, metrics::font_md() + 2.0, color),
                label_w(text, metrics::font_md(), color, Weight::Medium)
            ]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
        )
        .height(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(metrics::NAV_ITEM_HEIGHT)
    .padding(Padding::from([0.0, metrics::space_md()]))
    .style(style::nav_item(t, active))
    .on_press_maybe(msg)
    .into()
}

fn expanded(state: &Listen, width: f32, drawer: bool) -> Element<'_, Message> {
    let t = state.tokens;
    let sb = &state.config.settings.sidebar;

    let square = |glyph: Icon, msg: Option<Message>, name: &str, action: Option<&str>| {
        super::tip::tip(
            state,
            button(container(icon(glyph, metrics::font_lg(), t.foreground)).center(Length::Fill))
                .width(Length::Fill)
                .height(metrics::CONTROL)
                .padding(0)
                .style(style::default_button(t))
                .on_press_maybe(msg),
            name,
            action,
            super::tip::Position::Bottom,
        )
    };
    // Like Feishin: the search box opens the command palette (Ctrl+K).
    let search = button(label("Search", metrics::font_md(), t.muted))
        .width(Length::Fill)
        .padding(Padding { top: 9.0, bottom: 9.0, left: 12.0, right: 12.0 })
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(t.surface.into()),
                text_color: t.muted,
                border: iced::Border {
                    color: if hovered { t.primary } else { t.border },
                    width: 1.0,
                    radius: t.radius_sm.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::Palette(crate::app::palette::PaletteMsg::Open));
    // The menu button opens the app menu under it.
    let menu_button = anchored(square(Icon::LuMenu, Some(Message::Noop), "Menu", None), |b| {
        Message::Sidebar(SidebarMsg::OpenAppMenu(Point::new(b.x, b.y + b.height + 4.0)))
    });
    let search = super::tip::tip(state, search, "Search", Some("globalSearch"), super::tip::Position::Bottom);
    let action_bar = container(
        row![
            container(search).width(Length::FillPortion(6)),
            row![
                square(
                    Icon::LuPanelLeftClose,
                    Some(if drawer { Message::CloseDrawer } else { Message::ToggleSidebar }),
                    "Collapse sidebar",
                    None
                ),
                menu_button,
                square(
                    Icon::LuChevronLeft,
                    state.can_go_back().then_some(Message::Back),
                    "Back",
                    Some("browserBack")
                ),
                square(
                    Icon::LuChevronRight,
                    state.can_go_forward().then_some(Message::Forward),
                    "Forward",
                    Some("browserForward")
                ),
            ]
            .spacing(metrics::space_sm())
            .width(Length::FillPortion(6)),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .height(metrics::ACTION_BAR_HEIGHT)
    .padding(Padding::from([0.0, metrics::space_md()]))
    .center_y(metrics::ACTION_BAR_HEIGHT);

    // My Library
    let mut nav = Column::new().spacing(0);
    for (item, route) in visible_items(state) {
        let (Some(route), Some((line, fill))) = (route, nav_icons(&item.id)) else { continue };
        let active = state.route == route;
        nav = nav.push(nav_row(
            state,
            if active { fill } else { line },
            title_case(&item.label),
            active,
            Some(Message::Navigate(route)),
        ));
    }
    nav = nav.push(nav_row(
        state,
        Icon::LuDownload,
        "Downloads".into(),
        state.route == Route::Downloads,
        Some(Message::Navigate(Route::Downloads)),
    ));
    // A CD drive is connected (Linux): rip the disc in it.
    if state.rip_available() {
        let busy = state.rip.active();
        nav = nav.push(nav_row(
            state,
            Icon::LuDisc3,
            if busy { "Ripping CD…" } else { "Rip CD" }.into(),
            false,
            (!busy).then_some(Message::Rip(crate::app::rip::RipMsg::RipNow(None))),
        ));
    }

    // Servers: click to browse another one (playback continues), or add one.
    let mut servers = Column::new();
    if !state.demo {
        for server in &state.config.servers {
            let active = state.config.active_server.as_ref() == Some(&server.id);
            let color = if active { t.primary } else { t.foreground };
            let glyph = match server.kind {
                listen_core::model::SourceKind::YouTubeMusic => Icon::LuMusic,
                listen_core::model::SourceKind::Local => Icon::RiFolder3Line,
                _ => Icon::LuServer,
            };
            let connected = state.sources.contains_key(&server.id);
            let entry = button(
                row![
                    icon(glyph, metrics::font_md() + 2.0, color),
                    column![
                        label_w(
                            super::ellipsize(&server.name, 30),
                            metrics::font_md(),
                            color,
                            Weight::Medium
                        ),
                        label(
                            if connected || active { server.kind.label() } else { "not connected" },
                            metrics::font_xs(),
                            t.muted
                        ),
                    ]
                    .spacing(2),
                ]
                .spacing(metrics::space_md())
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([metrics::space_xs(), metrics::space_md()]))
            .style(style::nav_item(t, active))
            .on_press(Message::SwitchServer(server.id.clone()));
            servers = servers.push(
                row![entry, super::server_remove(state, &server.id)]
                    .spacing(metrics::space_xs())
                    .align_y(Alignment::Center),
            );
        }
        servers = servers.push(
            button(
                row![
                    icon(Icon::LuPlus, metrics::font_md() + 2.0, t.muted),
                    label_w("Add server", metrics::font_md(), t.muted, Weight::Medium),
                ]
                .spacing(metrics::space_md())
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(metrics::NAV_ITEM_HEIGHT)
            .padding(Padding::from([0.0, metrics::space_md()]))
            .style(style::nav_item(t, false))
            .on_press(Message::AddServer),
        );
    }

    let mut body = column![].spacing(metrics::space_xs());
    if !state.demo {
        body = body.push(static_header(state, "Servers")).push(servers);
    }
    body = body.push(accordion(state, Section::Library, "My Library", None));
    if state.sb.section_open(Section::Library) {
        body = body.push(nav);
    }
    if !collections(state).is_empty() {
        body = body.push(accordion(state, Section::Collections, "Collections", None));
        if state.sb.section_open(Section::Collections) {
            body = body.push(collection_rows(state));
        }
    }
    if sb.show_playlists {
        body = body.push(playlists_section(state, Scope::Owned));
        if !state.sidebar_playlists(Scope::Shared).is_empty() {
            body = body.push(playlists_section(state, Scope::Shared));
        }
    }

    let mut column = column![
        action_bar,
        scrollable(container(body).padding(Padding {
            top: 0.0,
            right: metrics::space_md(),
            bottom: metrics::space_md(),
            left: metrics::space_md(),
        }))
        .style(style::scrollbar(t))
        .height(Length::Fill),
    ];
    // The playing song's cover, as wide as the sidebar (but leave room for the list).
    let size = (width - 2.0 * metrics::space_md()).min(state.window_size.height * 0.45).max(80.0);
    if let Some(cover) = super::expanded_cover(state, size).filter(|_| !drawer) {
        column = column.push(container(cover).padding(metrics::space_md()).center_x(Length::Fill));
    }
    container(column).width(width).height(Length::Fill).style(style::sidebar(t)).into()
}

fn static_header<'a>(state: &Listen, title: &'static str) -> Element<'a, Message> {
    container(label_w(title, metrics::font_md(), state.tokens.muted, Weight::Medium))
        .height(40)
        .padding(Padding::from([0.0, metrics::space_md()]))
        .center_y(40)
        .into()
}

// ---- collections -----------------------------------------------------------------

fn collection_rows(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let mut col = Column::new();
    for (i, c) in collections(state).iter().enumerate() {
        let glyph = match c.list {
            CollectionList::Albums => Icon::RiAlbumLine,
            CollectionList::Songs => Icon::RiMusic2Line,
        };
        let open = button(
            row![
                icon(glyph, metrics::font_md() + 2.0, t.foreground),
                label_w(super::ellipsize(&c.name, 28), metrics::font_md(), t.foreground, Weight::Medium),
            ]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(metrics::NAV_ITEM_HEIGHT)
        .padding(Padding::from([0.0, metrics::space_md()]))
        .style(style::nav_item(t, false))
        .on_press(Message::ListQuery(crate::app::list_query::ListQueryMsg::ApplyCollection(i)));
        let more = anchored(
            super::tip::tip(
                state,
                button(icon(Icon::LuEllipsisVertical, metrics::font_md(), t.muted))
                    .padding(6)
                    .style(style::icon_button(t, false))
                    .on_press(Message::Noop),
                "More options",
                None,
                super::tip::Position::Right,
            ),
            move |b| Message::Sidebar(SidebarMsg::OpenCollection(i, Point::new(b.x + b.width + 4.0, b.y))),
        );
        col = col.push(row![open, more].align_y(Alignment::Center));
    }
    col.into()
}

// ---- playlists -------------------------------------------------------------------

fn playlists_section(state: &Listen, scope: Scope) -> Element<'_, Message> {
    let t = state.tokens;
    let sb = &state.config.settings.sidebar;
    let section = if scope == Scope::Owned { Section::Playlists } else { Section::Shared };
    let navigating = sb.playlist_folders && sb.playlist_folder_view == FolderView::Navigation;
    let stack = state.sb.nav(scope);
    let nodes = state.sidebar_nodes(scope);

    let mut extra: Vec<Element<'_, Message>> = Vec::new();
    if let Some(nodes) = &nodes
        && !navigating
    {
        let paths = crate::app::sidebar_state::folder_paths(nodes);
        if !paths.is_empty() {
            let all_open = paths.iter().all(|p| state.sb.folder_open(scope, p));
            extra.push(small_tool(
                state,
                if all_open { Icon::LuChevronUp } else { Icon::LuChevronDown },
                Message::Sidebar(SidebarMsg::SetAllFolders(scope, !all_open)),
                if all_open { "Collapse all folders" } else { "Expand all folders" },
            ));
        }
    }
    if scope == Scope::Owned && state.can_create_playlist() {
        extra.push(small_tool(
            state,
            Icon::LuPlus,
            Message::PlaylistPage(crate::app::playlists::PlaylistPageMsg::Create(Vec::new())),
            "Create playlist",
        ));
    }
    let extra = (!extra.is_empty()).then(|| row(extra).into());

    // In the navigation view the header shows the folder you are in, with a way back.
    let (title, back) = match (navigating, stack.last()) {
        (true, Some(name)) => (name.clone(), true),
        _ => (if scope == Scope::Owned { "Playlists" } else { "Shared Playlists" }.to_string(), false),
    };
    let header: Element<'_, Message> = if back {
        let up = small_tool(
            state,
            Icon::LuChevronLeft,
            Message::Sidebar(SidebarMsg::NavUp(scope)),
            "Up one folder",
        );
        row![up, container(accordion(state, section, title, extra)).width(Length::Fill)]
            .align_y(Alignment::Center)
            .into()
    } else {
        accordion(state, section, title, extra)
    };

    let mut col = Column::new().spacing(metrics::space_xs()).push(header);
    if !state.sb.section_open(section) {
        return col.into();
    }
    if scope == Scope::Owned && state.library.playlists.is_empty() {
        col = col.push(
            container(label("No playlists yet", metrics::font_sm(), t.muted))
                .padding(Padding::from([metrics::space_sm(), metrics::space_md()])),
        );
        return col.into();
    }
    match nodes {
        None => {
            for p in state.sidebar_playlists(scope) {
                col = col.push(playlist_row(state, p, &p.name, scope));
            }
        }
        Some(nodes) if navigating => {
            for node in at_path(&nodes, stack) {
                col = col.push(match node {
                    Node::Folder { name, leaves, .. } => folder_row(
                        state,
                        scope,
                        name,
                        *leaves,
                        false,
                        Message::Sidebar(SidebarMsg::NavEnter(scope, name.clone())),
                    ),
                    Node::Leaf { name, playlist } => playlist_row(state, playlist, name, scope),
                });
            }
        }
        Some(nodes) => col = col.push(tree(state, scope, &nodes)),
    }
    col.into()
}

fn tree<'a>(state: &'a Listen, scope: Scope, nodes: &[Node<'a>]) -> Element<'a, Message> {
    let t = state.tokens;
    let sb = &state.config.settings.sidebar;
    let line = parse_hex(&sb.folder_tree_line_color).unwrap_or(t.border);
    let mut col = Column::new().spacing(metrics::space_xs());
    for node in nodes {
        match node {
            Node::Leaf { name, playlist } => col = col.push(playlist_row(state, playlist, name, scope)),
            Node::Folder { name, path, leaves, children } => {
                let open = state.sb.folder_open(scope, path);
                col = col.push(folder_row(
                    state,
                    scope,
                    name,
                    *leaves,
                    open,
                    Message::Sidebar(SidebarMsg::ToggleFolder(scope, path.clone())),
                ));
                if open {
                    let guide = container(space()).width(1).height(Length::Fill).style(move |_| {
                        container::Style { background: Some(line.into()), ..Default::default() }
                    });
                    col = col.push(
                        row![guide, tree(state, scope, children)]
                            .spacing(f32::from(sb.folder_tree_indent))
                            .padding(Padding { left: metrics::space_md(), ..Padding::ZERO }),
                    );
                }
            }
        }
    }
    col.into()
}

fn folder_row<'a>(
    state: &Listen,
    scope: Scope,
    name: &str,
    leaves: usize,
    open: bool,
    msg: Message,
) -> Element<'a, Message> {
    let t = state.tokens;
    let navigating = state.config.settings.sidebar.playlist_folder_view == FolderView::Navigation;
    let _ = scope;
    button(
        row![
            icon(
                if open { Icon::LuFolderOpen } else { Icon::RiFolder3Line },
                metrics::font_md() + 2.0,
                t.foreground
            ),
            label_w(super::ellipsize(name, 28), metrics::font_md(), t.foreground, Weight::Medium),
            label(format!("{leaves}"), metrics::font_xs(), t.muted),
            space::horizontal(),
            icon(
                if navigating {
                    Icon::LuChevronRight
                } else if open {
                    Icon::LuChevronDown
                } else {
                    Icon::LuChevronRight
                },
                metrics::font_md(),
                t.muted
            ),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(metrics::NAV_ITEM_HEIGHT - 4.0)
    .padding(Padding::from([0.0, metrics::space_md()]))
    .style(style::nav_item(t, false))
    .on_press(msg)
    .into()
}

/// One playlist: opens on click, right-click menu, a place to drop songs on;
/// with manual sorting it is dragged to its new place instead.
fn playlist_row<'a>(state: &Listen, p: &'a Playlist, name: &str, scope: Scope) -> Element<'a, Message> {
    let t = state.tokens;
    let sb = &state.config.settings.sidebar;
    let active = state.route == Route::Playlist(p.id.clone());
    let color = if active { t.primary } else { t.foreground };
    let compact = sb.playlist_mode == PlaylistMode::Compact;
    let content: Element<'_, Message> = if compact {
        row![
            icon(Icon::LuListMusic, metrics::font_md(), color),
            label_w(super::ellipsize(name, 34), metrics::font_md(), color, Weight::Medium),
        ]
        .spacing(metrics::space_md())
        .align_y(Alignment::Center)
        .into()
    } else {
        let mut sub = p.song_count.map(|n| format!("{n} songs")).unwrap_or_default();
        if scope == Scope::Shared
            && let Some(owner) = p.owner.as_deref().filter(|o| !o.is_empty())
        {
            sub = if sub.is_empty() { owner.to_string() } else { format!("{sub} · {owner}") };
        }
        row![
            super::cover(state, p.image.as_ref(), 40.0, metrics::radius_md(), Icon::LuListMusic),
            column![
                label_w(super::ellipsize(name, 34), metrics::font_md(), color, Weight::Medium),
                label(sub, metrics::font_xs(), t.muted),
            ]
            .spacing(2),
        ]
        .spacing(metrics::space_md())
        .align_y(Alignment::Center)
        .into()
    };
    let padding = if compact {
        Padding::from([metrics::space_xs() + 2.0, metrics::space_md()])
    } else {
        Padding::from([metrics::space_xs(), metrics::space_md()])
    };
    let menu = crate::app::menu::MenuTarget::playlist(p);

    if sb.playlist_sorting {
        let target = state
            .sb
            .drag
            .as_ref()
            .is_some_and(|d| d.kind == DragKind::Playlist(scope) && d.over == p.id && d.from != p.id);
        let dragged =
            state.sb.drag.as_ref().is_some_and(|d| d.kind == DragKind::Playlist(scope) && d.from == p.id);
        let row = container(content).width(Length::Fill).padding(padding).style(move |_| container::Style {
            background: (target || dragged || active).then_some(t.nav_hover.into()),
            border: iced::Border {
                color: if target { t.primary } else { Color::TRANSPARENT },
                width: if target { 2.0 } else { 0.0 },
                radius: t.radius_sm.into(),
            },
            ..Default::default()
        });
        let id = p.id.clone();
        return super::with_menu(
            mouse_area(row)
                .interaction(iced::mouse::Interaction::Grab)
                .on_press(Message::Sidebar(SidebarMsg::DragStart(DragKind::Playlist(scope), id.clone())))
                .on_enter(Message::Sidebar(SidebarMsg::DragOver(id))),
            menu,
        );
    }

    let row = super::with_menu(
        button(content)
            .width(Length::Fill)
            .padding(padding)
            .style(style::nav_item(t, active))
            .on_press(Message::Navigate(Route::Playlist(p.id.clone()))),
        menu,
    );
    if p.editable {
        super::list::drop_target(
            row,
            crate::app::list::DropTarget::Playlist {
                server: p.server_id.clone(),
                id: p.id.clone(),
                name: p.name.clone(),
            },
        )
    } else {
        row
    }
}
