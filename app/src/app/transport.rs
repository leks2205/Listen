//! Player-bar transport extras: seek-on-release, Alt+click album jumps and
//! the shuffle-all button.

use super::menu::{MenuAction, MenuMsg};
use super::{Listen, Message};
use iced::Task;
use std::time::Duration;

/// Songs fetched for the shuffle-all button.
const SHUFFLE_ALL_COUNT: usize = 100;

impl Listen {
    pub(super) fn handle_transport(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SeekPreview(secs) => self.seek_preview = Some(secs),
            Message::SeekRelease => {
                if let Some(secs) = self.seek_preview.take() {
                    return self.update(Message::Seek(secs));
                }
            }
            Message::NextClick if self.modifiers.alt() => return self.handle(Message::NextAlbum),
            Message::NextClick => return self.handle(Message::Next),
            Message::PreviousClick if self.modifiers.alt() => return self.handle(Message::PreviousAlbum),
            Message::PreviousClick => return self.handle(Message::Previous),
            Message::NextAlbum => {
                self.dj_on_next();
                if self.queue.skip_next_album().is_some() {
                    return self.start_current();
                }
                return self.stop_player();
            }
            Message::PreviousAlbum => {
                // Like Previous: more than 10 s in restarts the song.
                if self.playback.position > Duration::from_secs(10) {
                    return self.update(Message::Seek(0.0));
                }
                self.queue.skip_previous_album();
                return self.start_current();
            }
            Message::ShuffleAll => return self.shuffle_all(),
            _ => {}
        }
        Task::none()
    }

    /// Feishin's "shuffle all": random songs from the playing song's server (or
    /// any), played through the existing "Shuffle" menu action.
    fn shuffle_all(&mut self) -> Task<Message> {
        let server = self.queue.current().map(|i| i.song.server_id.clone());
        let source = server
            .and_then(|id| self.sources.get(&id).cloned())
            .or_else(|| self.sources.iter().min_by_key(|(id, _)| id.0.clone()).map(|(_, s)| s.clone()));
        let Some(source) = source else { return Task::none() };
        Task::perform(
            async move { source.random_songs(SHUFFLE_ALL_COUNT, None).await.map_err(|e| e.to_string()) },
            |r| Message::Menu(MenuMsg::Songs(MenuAction::Shuffle, r)),
        )
    }
}
