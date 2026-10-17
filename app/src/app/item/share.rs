//! Sharing, as in Feishin's `features/sharing`: Navidrome `createShare` with a description and
//! expiry, then copy the link. YouTube Music copies its public link instead.

use super::super::menu::MenuTarget;
use super::super::{Listen, Message};
use super::ItemMsg;
use super::details::DetailsMsg;
use crate::app::overlay::Toast;
use iced::Task;
use listen_core::model::{ServerId, SourceKind};
use listen_core::settings::{ExpiryUnit, ShareExpiration, SharingSettings};
use listen_source::{ItemKind, Share};
use std::time::SystemTime;

pub const MODAL: &str = "item-share";

/// The expiry presets next to the date field (Feishin's buttons).
pub const PRESETS: [(&str, Option<(u32, ExpiryUnit)>); 5] = [
    ("Server default", None),
    ("1 day", Some((1, ExpiryUnit::Day))),
    ("1 week", Some((1, ExpiryUnit::Week))),
    ("1 month", Some((1, ExpiryUnit::Month))),
    ("1 year", Some((1, ExpiryUnit::Year))),
];

#[derive(Debug, Clone)]
pub struct ShareState {
    pub server: ServerId,
    pub kind: ItemKind,
    pub ids: Vec<String>,
    /// What is shared, for the modal's title line.
    pub title: String,
    pub description: String,
    pub expiry: ShareExpiration,
    /// The amount as typed.
    pub amount: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl ShareState {
    /// When the share expires: `Ok(None)` for the server's default.
    pub fn expires_at(&self, now: SystemTime) -> Result<Option<SystemTime>, String> {
        if self.expiry.use_server_default {
            return Ok(None);
        }
        let amount: u32 =
            self.amount.trim().parse().map_err(|_| "The expiry must be a whole number.".to_string())?;
        if amount == 0 {
            return Err("The expiry must be in the future.".into());
        }
        Ok(Some(listen_core::settings::add(now, amount, self.expiry.unit)))
    }
}

#[derive(Debug, Clone)]
pub enum ShareMsg {
    Description(String),
    ServerDefault(bool),
    Amount(String),
    Unit(ExpiryUnit),
    Preset(Option<(u32, ExpiryUnit)>),
    /// Keep the chosen expiry as the default of new shares.
    MakeDefault,
    Submit,
    Created(Result<Share, String>),
}

/// A change on the settings page.
#[derive(Debug, Clone)]
pub enum SharingSetting {
    UseServerDefault(bool),
    Amount(u32),
    Unit(ExpiryUnit),
}

impl SharingSetting {
    pub fn apply(self, s: &mut SharingSettings) {
        match self {
            SharingSetting::UseServerDefault(v) => s.expiration.use_server_default = v,
            SharingSetting::Amount(n) => s.expiration.amount = n.max(1),
            SharingSetting::Unit(u) => s.expiration.unit = u,
        }
    }
}

/// The public YouTube Music link of the (single) target, if it has one.
pub fn ytm_link(target: &MenuTarget) -> Option<String> {
    let id = |id: &str| id.to_string();
    Some(match target {
        MenuTarget::Songs { songs, .. } => format!("https://music.youtube.com/watch?v={}", songs.first()?.id),
        MenuTarget::Albums(v) => format!("https://music.youtube.com/browse/{}", id(&v.first()?.id)),
        MenuTarget::Artists(v) => format!("https://music.youtube.com/channel/{}", id(&v.first()?.id)),
        MenuTarget::Playlists(v) => {
            let p = &v.first()?.id;
            format!("https://music.youtube.com/playlist?list={}", p.strip_prefix("VL").unwrap_or(p))
        }
        MenuTarget::Genres(_) | MenuTarget::Folders(_) => return None,
    })
}

/// (kind, ids, title) of what a share link points at.
fn shared_items(target: &MenuTarget) -> Option<(ItemKind, Vec<String>, String)> {
    let title = match target.len() {
        0 => return None,
        1 => target.first_name(),
        n => format!("{n} items"),
    };
    Some(match target {
        MenuTarget::Songs { songs, .. } => {
            (ItemKind::Song, songs.iter().map(|s| s.id.clone()).collect(), title)
        }
        MenuTarget::Albums(v) => (ItemKind::Album, v.iter().map(|a| a.id.clone()).collect(), title),
        MenuTarget::Artists(v) => (ItemKind::Artist, v.iter().map(|a| a.id.clone()).collect(), title),
        // The server looks the kind up from the id.
        MenuTarget::Playlists(v) => (ItemKind::Album, v.iter().map(|a| a.id.clone()).collect(), title),
        MenuTarget::Folders(v) => (ItemKind::Album, v.iter().map(|a| a.id.clone()).collect(), title),
        MenuTarget::Genres(_) => return None,
    })
}

impl Listen {
    /// "Share": the modal for servers with shares, the copied link for YouTube Music.
    pub(in crate::app) fn open_share(&mut self, target: &MenuTarget) -> Task<Message> {
        let server = target.server().or(self.config.active_server.as_ref()).cloned();
        let source =
            server.as_ref().and_then(|s| self.sources.get(s)).cloned().or_else(|| self.source.clone());
        let Some(source) = source else { return Task::none() };
        if source.capabilities().contains(listen_source::Capabilities::SHARES) {
            let (Some(server), Some((kind, ids, title))) = (server, shared_items(target)) else {
                return Task::none();
            };
            let expiry = self.config.settings.sharing.expiration;
            self.item.share = Some(ShareState {
                server,
                kind,
                ids,
                title,
                description: String::new(),
                amount: expiry.amount.to_string(),
                expiry,
                busy: false,
                error: None,
            });
            self.open_modal(MODAL);
            return Task::none();
        }
        if source.kind() == SourceKind::YouTubeMusic
            && let Some(link) = ytm_link(target)
        {
            self.toast_success("Link copied.".into());
            return iced::clipboard::write(link);
        }
        self.toast_info("This server can't create share links.".into());
        Task::none()
    }

    pub(super) fn on_share(&mut self, msg: ShareMsg) -> Task<Message> {
        let Some(state) = self.item.share.as_mut() else { return Task::none() };
        match msg {
            ShareMsg::Description(d) => state.description = d,
            ShareMsg::ServerDefault(v) => state.expiry.use_server_default = v,
            ShareMsg::Amount(a) => {
                state.amount = a.chars().filter(char::is_ascii_digit).take(5).collect();
                state.error = None;
            }
            ShareMsg::Unit(u) => state.expiry.unit = u,
            ShareMsg::Preset(None) => state.expiry.use_server_default = true,
            ShareMsg::Preset(Some((n, unit))) => {
                state.expiry = ShareExpiration { use_server_default: false, amount: n, unit };
                state.amount = n.to_string();
            }
            ShareMsg::MakeDefault => {
                let mut expiry = state.expiry;
                if !expiry.use_server_default {
                    match state.amount.trim().parse::<u32>() {
                        Ok(n) if n > 0 => expiry.amount = n,
                        _ => {
                            state.error = Some("The expiry must be a whole number above zero.".into());
                            return Task::none();
                        }
                    }
                }
                self.config.settings.sharing.expiration = expiry;
                self.toast_success("Default share expiration saved.".into());
                self.save_config();
                return Task::none();
            }
            ShareMsg::Submit => {
                if state.busy {
                    return Task::none();
                }
                let expires = match state.expires_at(SystemTime::now()) {
                    Ok(e) => e,
                    Err(e) => {
                        state.error = Some(e);
                        return Task::none();
                    }
                };
                let Some(source) = self.sources.get(&state.server).cloned() else {
                    state.error = Some("The server is not connected.".into());
                    return Task::none();
                };
                state.busy = true;
                state.error = None;
                let (kind, ids) = (state.kind, state.ids.clone());
                let description = Some(state.description.clone()).filter(|d| !d.trim().is_empty());
                return Task::perform(
                    async move {
                        source
                            .create_share(kind, &ids, description.as_deref(), expires)
                            .await
                            .map_err(|e| format!("Could not create the share link: {e}"))
                    },
                    |r| Message::Item(ItemMsg::Share(ShareMsg::Created(r))),
                );
            }
            ShareMsg::Created(Ok(share)) => {
                self.item.share = None;
                self.close_modal();
                let open = Message::Item(ItemMsg::Details(DetailsMsg::OpenLink(share.url.clone())));
                self.toast(Toast::success("Share link copied to the clipboard.").action("Open link", open));
                return iced::clipboard::write(share.url);
            }
            ShareMsg::Created(Err(e)) => {
                state.busy = false;
                state.error = Some(e.clone());
                self.toast_error(e);
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn state(expiry: ShareExpiration, amount: &str) -> ShareState {
        ShareState {
            server: ServerId("s".into()),
            kind: ItemKind::Song,
            ids: vec!["a".into()],
            title: "t".into(),
            description: String::new(),
            expiry,
            amount: amount.into(),
            busy: false,
            error: None,
        }
    }

    #[test]
    fn expiry_comes_from_the_amount_and_unit() {
        let now = SystemTime::UNIX_EPOCH;
        let week = ShareExpiration { use_server_default: false, amount: 1, unit: ExpiryUnit::Week };
        assert_eq!(state(week, "1").expires_at(now), Ok(Some(now + Duration::from_secs(7 * 86_400))));
        assert_eq!(state(week, "3").expires_at(now), Ok(Some(now + Duration::from_secs(21 * 86_400))));
        let server = ShareExpiration { use_server_default: true, ..week };
        assert_eq!(state(server, "").expires_at(now), Ok(None));
        assert!(state(week, "0").expires_at(now).is_err());
        assert!(state(week, "").expires_at(now).is_err());
    }

    #[test]
    fn settings_changes_apply() {
        let mut s = SharingSettings::default();
        SharingSetting::Amount(0).apply(&mut s);
        assert_eq!(s.expiration.amount, 1);
        SharingSetting::Unit(ExpiryUnit::Month).apply(&mut s);
        SharingSetting::UseServerDefault(true).apply(&mut s);
        assert!(s.expiration.use_server_default && s.expiration.unit == ExpiryUnit::Month);
    }

    #[test]
    fn youtube_music_links() {
        let song = crate::demo::album_songs(0).remove(0);
        let t = MenuTarget::song(&song);
        assert_eq!(ytm_link(&t), Some(format!("https://music.youtube.com/watch?v={}", song.id)));
        let mut a = crate::demo::ytm_album();
        a.id = "MPREb_x".into();
        assert_eq!(
            ytm_link(&MenuTarget::album(&a)).as_deref(),
            Some("https://music.youtube.com/browse/MPREb_x")
        );
        assert_eq!(ytm_link(&MenuTarget::Genres(vec![])), None);
    }

    #[test]
    fn shared_items_name_the_target() {
        let songs = crate::demo::album_songs(0);
        let (kind, ids, title) = shared_items(&MenuTarget::songs(&songs[..2])).unwrap();
        assert_eq!((kind, ids.len(), title.as_str()), (ItemKind::Song, 2, "2 items"));
        assert!(shared_items(&MenuTarget::Genres(vec![])).is_none());
    }
}
