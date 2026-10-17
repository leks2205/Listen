//! What the Favorite, radio and queue-order entries do (the kind files only
//! declare them): one function per `MenuAction`, applied to every targeted item.

use super::{MenuAction, MenuMsg, MenuTarget, PlayHow, QueueMove};
use crate::app::{Listen, Message, err};
use iced::Task;
use listen_core::model::SourceKind;
use listen_source::ItemKind;

/// Songs fetched for an album radio or an artist radio (Feishin: the artist radio count).
const ALBUM_RADIO_COUNT: usize = 20;

impl PlayHow {
    /// The queue action that plays songs this way.
    pub fn action(self) -> MenuAction {
        match self {
            PlayHow::Now => MenuAction::Play,
            PlayHow::Next => MenuAction::PlayNext,
            PlayHow::Last => MenuAction::AddToQueue,
        }
    }
}

impl Listen {
    /// `Favorite`, `Radio`, `MoveQueue` and `ShuffleQueue` on `target`.
    pub(in crate::app) fn menu_extra(&mut self, action: MenuAction, target: MenuTarget) -> Task<Message> {
        match action {
            MenuAction::Favorite(favorite) => self.favorite_target(target, favorite),
            MenuAction::Radio(how) => self.radio_target(target, how),
            MenuAction::MoveQueue(to, uids) => {
                match to {
                    QueueMove::Top => self.queue.move_to_top(&uids),
                    QueueMove::Next => self.queue.move_to_next(&uids),
                    QueueMove::Bottom => self.queue.move_to_bottom(&uids),
                }
                self.refresh_next()
            }
            MenuAction::ShuffleQueue(selected) => {
                match selected {
                    Some(uids) => self.queue.shuffle_selected(&uids),
                    None => self.queue.shuffle_all(),
                }
                self.refresh_next()
            }
            _ => Task::none(),
        }
    }

    /// Favorites (or un-favorites) every targeted song, album or artist.
    fn favorite_target(&mut self, target: MenuTarget, favorite: bool) -> Task<Message> {
        let tasks: Vec<_> = match target {
            MenuTarget::Songs { songs, .. } => songs
                .iter()
                .filter(|s| s.favorite != favorite)
                .map(|s| self.set_song_favorite(s, favorite))
                .collect(),
            MenuTarget::Albums(albums) => albums
                .iter()
                .map(|a| self.set_item_favorite(ItemKind::Album, &a.server_id, &a.id, favorite))
                .collect(),
            MenuTarget::Artists(artists) => artists
                .iter()
                .map(|a| self.set_item_favorite(ItemKind::Artist, &a.server_id, &a.id, favorite))
                .collect(),
            _ => Vec::new(),
        };
        Task::batch(tasks)
    }

    /// The radio of the (single) album or artist, played / queued as `how`.
    fn radio_target(&mut self, target: MenuTarget, how: PlayHow) -> Task<Message> {
        let play = how.action();
        let done = move |r| Message::Menu(MenuMsg::Songs(play.clone(), r));
        match target {
            MenuTarget::Albums(albums) if albums.len() == 1 => {
                let album = albums[0].clone();
                if self.demo {
                    // No server: the other demo albums stand in for the radio.
                    let songs = crate::demo::albums()
                        .into_iter()
                        .filter(|a| a.id != album.id)
                        .flat_map(|a| a.songs)
                        .take(ALBUM_RADIO_COUNT)
                        .collect();
                    return self.apply_to_songs(how.action(), songs);
                }
                let Some(source) = self.sources.get(&album.server_id).cloned() else { return Task::none() };
                Task::perform(
                    async move {
                        let seed = match album.songs.first() {
                            Some(s) => s.clone(),
                            None => source
                                .album(&album.id)
                                .await
                                .map_err(err)?
                                .songs
                                .into_iter()
                                .next()
                                .ok_or("the album has no songs")?,
                        };
                        let mut songs = Vec::new();
                        // YouTube Music's radio starts from the song itself.
                        if source.kind() == SourceKind::YouTubeMusic {
                            songs.push(seed.clone());
                        }
                        songs.extend(source.similar_songs(&seed, ALBUM_RADIO_COUNT).await.map_err(err)?);
                        songs.truncate(ALBUM_RADIO_COUNT);
                        Ok(songs)
                    },
                    done,
                )
            }
            MenuTarget::Artists(artists) if artists.len() == 1 => {
                let artist = artists[0].clone();
                if self.demo {
                    let songs = crate::demo::albums()
                        .into_iter()
                        .flat_map(|a| a.songs)
                        .take(ALBUM_RADIO_COUNT)
                        .collect();
                    return self.apply_to_songs(how.action(), songs);
                }
                let Some(source) = self.sources.get(&artist.server_id).cloned() else { return Task::none() };
                let count = self.config.settings.artist.radio_count.max(1) as usize;
                Task::perform(async move { source.artist_radio(&artist, count).await.map_err(err) }, done)
            }
            _ => Task::none(),
        }
    }
}
