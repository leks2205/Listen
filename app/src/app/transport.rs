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

    /// Keeps the music going when mpv went idle while songs are still queued: after a
    /// between-songs DJ clip that had nothing loaded behind it, or a song that failed to
    /// load. Nothing else would start them — the AI DJ plans only when the queue is
    /// empty and the Auto-DJ fills only on a track change — so playback would stand
    /// still until someone presses play.
    pub(super) fn keep_going(&mut self) -> Option<Task<Message>> {
        if !self.playback.idle
            || self.playback.playing
            || self.dj_airing()
            || self.dlna_casting()
            || self.jukebox_active()
        {
            return None;
        }
        // Without a refill mode a finished queue is meant to stay finished.
        if !self.ai_dj_on() && !self.config.settings.auto_dj.enabled {
            return None;
        }
        // Nothing queued: the AI DJ plans the next set, the Auto-DJ fills in.
        let next = self.queue.peek_next().map(|i| i.song.title.clone())?;
        tracing::info!("the player went idle with songs queued; playing “{next}”");
        self.playback.idle = false;
        self.queue.advance();
        Some(self.start_current())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::PlayerMsg;
    use crate::{Args, demo};
    use listen_player::PlayerEvent;

    fn boot() -> Listen {
        let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        s.queue.set(demo::album_songs(0), 0);
        s.playback.playing = true;
        s
    }

    /// mpv reports that it finished the last entry and has nothing to play.
    fn ended(s: &mut Listen) {
        let _ = s.update(Message::Deck(0, PlayerMsg::Event(PlayerEvent::Ended)));
    }

    #[test]
    fn a_finished_song_leaves_the_player_idle() {
        let mut s = boot();
        ended(&mut s);
        assert!(s.playback.idle && !s.playback.playing);
    }

    #[test]
    fn queued_songs_play_on_when_the_player_went_idle() {
        let mut s = boot();
        ended(&mut s);
        let next = s.queue.peek_next().map(|i| i.uid).expect("more songs queued");
        assert!(s.keep_going().is_some(), "the next song is started");
        assert_eq!(s.queue.current().map(|i| i.uid), Some(next));
        assert!(!s.playback.idle);
    }

    #[test]
    fn a_paused_player_is_left_alone() {
        let mut s = boot();
        s.playback.playing = false; // paused: mpv keeps its entry, so it is not idle
        assert!(s.keep_going().is_none());
    }

    #[test]
    fn a_playing_song_is_not_interrupted() {
        let mut s = boot();
        s.playback.idle = true; // a stale flag must not cut a song short
        assert!(s.keep_going().is_none());
    }

    #[test]
    fn nothing_starts_without_a_refill_mode() {
        let mut s = boot();
        s.config.settings.auto_dj.enabled = false;
        ended(&mut s);
        assert!(s.keep_going().is_none());
    }

    #[test]
    fn the_last_queued_song_is_left_to_the_dj() {
        let mut s = boot();
        let songs = demo::album_songs(0);
        s.queue.set(songs.clone(), songs.len() - 1);
        ended(&mut s);
        assert!(s.queue.peek_next().is_none());
        assert!(s.keep_going().is_none(), "the AI DJ plans the next set");
    }

    #[test]
    fn a_deliberate_stop_is_not_undone() {
        let mut s = boot();
        ended(&mut s);
        let _ = s.stop_player();
        assert!(!s.playback.idle, "stopping clears the idle flag");
    }
}
