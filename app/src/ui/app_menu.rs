//! Sidebar popups: the app menu (Feishin's `titlebar/components/app-menu.tsx`), the collapsed
//! rail's Collections menu, and a collection's menu / rename box.

use super::{icon, label, label_w};
use crate::app::sidebar_state::{Popup, SidebarMsg};
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, opaque, row, space, text_input};
use iced::{Alignment, Element, Length, Padding, Point, Size};
use listen_core::settings::{CollectionList, SideQueueLayout};

const MENU_W: f32 = 260.0;
const ITEM: f32 = 34.0;
const DIVIDER: f32 = 9.0;

/// `msg` after the popup closed.
fn then(msg: Message) -> Message {
    Message::Sidebar(SidebarMsg::Then(Box::new(msg)))
}

fn item<'a>(
    state: &Listen,
    glyph: Option<Icon>,
    text: impl Into<String>,
    active: bool,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let color = if msg.is_none() {
        t.muted
    } else if active {
        t.primary
    } else {
        t.foreground
    };
    let mut content = row![].spacing(metrics::space_md()).align_y(Alignment::Center);
    if let Some(glyph) = glyph {
        content = content.push(icon(glyph, metrics::font_lg(), color));
    }
    content = content.push(label_w(text.into(), metrics::font_md(), color, Weight::Medium));
    button(container(content).center_y(Length::Fill))
        .width(Length::Fill)
        .height(ITEM)
        .padding(Padding::from([0.0, metrics::space_md()]))
        .style(style::nav_item(state.tokens, active))
        .on_press_maybe(msg)
        .into()
}

fn divider<'a>(state: &Listen) -> Element<'a, Message> {
    container(super::divider_h(state.tokens)).padding(Padding::from([4.0, 0.0])).into()
}

fn panel<'a>(state: &Listen, body: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    let t = state.tokens;
    opaque(container(body).width(width).padding(4).style(move |_| container::Style {
        background: Some(iced::Background::Color(t.background_alt)),
        border: iced::border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: iced::Shadow {
            color: style::alpha(iced::Color::BLACK, 0.45),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    }))
}

/// Entries of the app menu: `(widget, height)`.
fn app_menu_entries<'a>(state: &Listen) -> Vec<(Element<'a, Message>, f32)> {
    let sb = &state.config.settings.sidebar;
    let t = state.tokens;
    let compact = state.layout_mode() == super::LayoutMode::Compact;
    let mut out: Vec<(Element<'a, Message>, f32)> = Vec::new();
    let mut add = |e: Element<'a, Message>, h: f32| out.push((e, h));

    if !state.demo && !state.config.servers.is_empty() {
        add(
            container(label("Select server", metrics::font_sm(), t.muted))
                .padding(Padding::from([4.0, metrics::space_md()]))
                .into(),
            26.0,
        );
        for server in &state.config.servers {
            let active = state.config.active_server.as_ref() == Some(&server.id);
            let glyph = match server.kind {
                listen_core::model::SourceKind::YouTubeMusic => Icon::LuMusic,
                listen_core::model::SourceKind::Local => Icon::RiFolder3Line,
                _ => Icon::LuServer,
            };
            add(
                item(
                    state,
                    Some(glyph),
                    super::ellipsize(&server.name, 28),
                    active,
                    Some(then(Message::SwitchServer(server.id.clone()))),
                ),
                ITEM,
            );
        }
        add(item(state, Some(Icon::LuPlus), "Add server", false, Some(then(Message::AddServer))), ITEM);
        add(
            item(
                state,
                Some(Icon::LuServer),
                "Manage servers",
                false,
                Some(then(Message::Navigate(Route::Settings))),
            ),
            ITEM,
        );
        add(divider(state), DIVIDER);
    }
    add(
        item(
            state,
            Some(Icon::LuSearch),
            "Open command palette",
            false,
            Some(then(Message::Palette(crate::app::palette::PaletteMsg::Open))),
        ),
        ITEM,
    );
    add(divider(state), DIVIDER);
    if sb.collapsed && !compact {
        add(
            item(
                state,
                Some(Icon::LuChevronLeft),
                "Go back",
                false,
                state.can_go_back().then(|| then(Message::Back)),
            ),
            ITEM,
        );
        add(
            item(
                state,
                Some(Icon::LuChevronRight),
                "Go forward",
                false,
                state.can_go_forward().then(|| then(Message::Forward)),
            ),
            ITEM,
        );
    }
    if !compact {
        add(
            item(
                state,
                Some(if sb.collapsed { Icon::LuPanelLeftOpen } else { Icon::LuPanelLeftClose }),
                if sb.collapsed { "Expand sidebar" } else { "Collapse sidebar" },
                false,
                Some(then(Message::ToggleSidebar)),
            ),
            ITEM,
        );
        add(divider(state), DIVIDER);
    }
    // Private mode (Feishin's app menu): no scrobbling, Discord or local history.
    add(
        item(
            state,
            Some(Icon::LuUser),
            "Private mode",
            state.private_mode(),
            Some(then(Message::Integrations(crate::app::integrations::IntegrationsMsg::TogglePrivate))),
        ),
        ITEM,
    );
    add(
        item(
            state,
            Some(Icon::RiSettings2Line),
            "Settings",
            false,
            Some(then(Message::Navigate(Route::Settings))),
        ),
        ITEM,
    );
    add(divider(state), DIVIDER);
    add(
        item(
            state,
            Some(Icon::LuExternalLink),
            format!("Version {}", env!("CARGO_PKG_VERSION")),
            false,
            Some(Message::Sidebar(SidebarMsg::OpenReleases)),
        ),
        ITEM,
    );
    add(item(state, Some(Icon::LuX), "Quit", false, Some(Message::Sidebar(SidebarMsg::Quit))), ITEM);
    if sb.show_queue && !compact {
        add(divider(state), DIVIDER);
        let layout = state.config.settings.queue.side_layout;
        let choice = |glyph: Icon, which: SideQueueLayout| {
            let on = layout == which;
            button(
                container(icon(glyph, metrics::font_lg() + 2.0, if on { t.primary } else { t.muted }))
                    .center(Length::Fill),
            )
            .width(Length::Fill)
            .height(ITEM)
            .padding(0)
            .style(style::nav_item(t, on))
            .on_press(Message::Sidebar(SidebarMsg::SideQueueLayout(which)))
        };
        add(
            row![
                choice(Icon::LuPanelRightOpen, SideQueueLayout::Horizontal),
                choice(Icon::LuPanelRightClose, SideQueueLayout::Vertical),
            ]
            .spacing(metrics::space_xs())
            .into(),
            ITEM,
        );
    }
    out
}

fn collection_menu<'a>(state: &Listen, index: usize) -> (Element<'a, Message>, f32) {
    let col = column![
        item(
            state,
            Some(Icon::LuPencil),
            "Rename",
            false,
            Some(Message::Sidebar(SidebarMsg::RenameCollection(index)))
        ),
        item(
            state,
            Some(Icon::LuTrash2),
            "Delete",
            false,
            Some(Message::Sidebar(SidebarMsg::DeleteCollection(index)))
        ),
    ];
    (col.into(), 2.0 * ITEM + 8.0)
}

fn rename_box<'a>(state: &Listen, name: &str) -> (Element<'a, Message>, f32) {
    let t = state.tokens;
    let input = text_input(crate::i18n::tr("Name"), name)
        .font(crate::ui::font_for(name, crate::appearance::current_font()))
        .on_input(|s| Message::Sidebar(SidebarMsg::RenameInput(s)))
        .on_submit(Message::Sidebar(SidebarMsg::RenameSave))
        .padding(8)
        .style(style::input(t));
    let btn = |text: &'static str, msg: Message, primary: bool| {
        let b = button(label_w(text, metrics::font_md(), t.foreground, Weight::Medium))
            .padding(Padding::from([6.0, 12.0]));
        if primary { b.style(style::primary_button(t)) } else { b.style(style::default_button(t)) }
            .on_press(msg)
    };
    let col = column![
        input,
        row![
            space::horizontal(),
            btn("Cancel", Message::Sidebar(SidebarMsg::ClosePopup), false),
            btn("Save", Message::Sidebar(SidebarMsg::RenameSave), true),
        ]
        .spacing(metrics::space_sm()),
    ]
    .spacing(metrics::space_sm())
    .padding(metrics::space_sm());
    (col.into(), 100.0)
}

fn rail_collections<'a>(state: &Listen) -> (Element<'a, Message>, f32) {
    let mut col = Column::new();
    let all = &state.config.settings.lists.collections;
    for (i, c) in all.iter().enumerate() {
        let glyph = match c.list {
            CollectionList::Albums => Icon::RiAlbumLine,
            CollectionList::Songs => Icon::RiMusic2Line,
        };
        col = col.push(item(
            state,
            Some(glyph),
            super::ellipsize(&c.name, 28),
            false,
            Some(then(Message::ListQuery(crate::app::list_query::ListQueryMsg::ApplyCollection(i)))),
        ));
    }
    let height = (all.len() as f32 * ITEM + 8.0).min(state.window_size.height * 0.5);
    (iced::widget::scrollable(col).style(style::scrollbar(state.tokens)).height(height).into(), height)
}

/// `root` with the open sidebar popup over it.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    let Some(popup) = &state.sb.popup else { return iced::widget::stack![root].into() };
    let (at, width, (body, height)) = match popup {
        Popup::AppMenu(at) => {
            let entries = app_menu_entries(state);
            let height: f32 = entries.iter().map(|(_, h)| h).sum::<f32>() + 8.0;
            let col = entries.into_iter().fold(Column::new(), |c, (e, _)| c.push(e));
            (*at, MENU_W, (col.into(), height))
        }
        Popup::RailCollections(at) => (*at, MENU_W, rail_collections(state)),
        Popup::Collection { index, at } => (*at, 180.0, collection_menu(state, *index)),
        Popup::Rename { at, name, .. } => (*at, 280.0, rename_box(state, name)),
    };
    let at: Point = super::popover::place(at, Size::new(width, height), state.window_size);
    super::popover::over(root, panel(state, body, width), at, Message::Sidebar(SidebarMsg::ClosePopup))
}
