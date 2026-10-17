//! Item details ("Get info"), share links, the tag editor, saving the original file and showing it in the
//! file manager. Menu entries are in `app/menu`, handlers in `Listen::item_action`, UI in `ui::item`.
//!
//! * [`details`]: rows of the "Get info" modal (pure) and its loading.
//! * [`share`]: the "Share" modal (Navidrome `createShare`) and the default
//!   expiry setting.
//! * [`tags`]: the tag editor of the local folder source (`lofty`).
//! * [`save`]: "Download" of the original file, with a progress toast, and
//!   "Show in file manager".

pub mod details;
pub mod menu;
pub mod save;
pub mod share;
pub mod tags;

use super::menu::{MenuAction, MenuTarget};
use super::{Listen, Message};
use iced::Task;
use listen_core::model::SourceKind;
use listen_source::Capabilities;

/// Messages of the item modals (one `Message` variant carries them all).
#[derive(Debug, Clone)]
pub enum ItemMsg {
    Details(details::DetailsMsg),
    Share(share::ShareMsg),
    Tags(tags::TagsMsg),
    Save(save::SaveMsg),
}

/// State of the item modals.
#[derive(Debug, Default)]
pub struct ItemState {
    pub details: Option<details::DetailsState>,
    pub share: Option<share::ShareState>,
    pub tags: Option<tags::TagsState>,
    pub save: save::SaveState,
}

impl Listen {
    pub(super) fn on_item(&mut self, msg: ItemMsg) -> Task<Message> {
        match msg {
            ItemMsg::Details(m) => self.on_details(m),
            ItemMsg::Share(m) => self.on_share(m),
            ItemMsg::Tags(m) => self.on_tags(m),
            ItemMsg::Save(m) => self.on_save(m),
        }
    }

    /// Runs a [`MenuAction`] of this module on the (still unresolved) menu target.
    pub(super) fn item_action(&mut self, action: MenuAction, target: MenuTarget) -> Task<Message> {
        match action {
            MenuAction::GetInfo => self.open_details(&target),
            MenuAction::ShareItem => self.open_share(&target),
            MenuAction::EditTags => self.open_tags(&target),
            MenuAction::ShowInFolder => self.show_in_folder(&target),
            _ => Task::none(),
        }
    }
}

/// Whether the source of the menu's items can create share links.
pub fn can_share(caps: Capabilities) -> bool {
    caps.contains(Capabilities::SHARES)
}

/// Whether the tag editor applies: every song comes from a local folder.
pub fn can_edit_tags(songs: &[listen_core::model::Song]) -> bool {
    !songs.is_empty() && songs.iter().all(|s| s.source == SourceKind::Local)
}
