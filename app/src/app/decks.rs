//! The music decks: two mpv players. The active deck plays the queue's current song; the other is
//! idle, or mixes the next song in during a DJ transition (see `mix`). Without transitions only
//! deck 0 runs and playback is gapless through mpv's playlist.

use listen_player::Player;

/// One mpv music player.
#[derive(Debug, Default)]
pub struct Deck {
    pub player: Option<Player>,
}

#[derive(Debug, Default)]
pub struct Decks {
    pub d: [Deck; 2],
    /// Index of the deck playing the queue's current song.
    pub active: usize,
}

impl Decks {
    pub fn active_player(&self) -> Option<Player> {
        self.d[self.active].player.clone()
    }

    pub fn idle_player(&self) -> Option<Player> {
        self.d[1 - self.active].player.clone()
    }

    pub fn has_active(&self) -> bool {
        self.d[self.active].player.is_some()
    }

    /// The deck's mpv started (or is gone: `None`).
    pub fn set(&mut self, deck: usize, player: Option<Player>) {
        self.d[deck.min(1)].player = player;
    }
}
