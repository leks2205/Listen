//! Runs mpv for the lifetime of the app and forwards its events as messages.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream};
use listen_player::{MpvOptions, Player, PlayerEvent};
use std::hash::Hash;

/// What the mpv process is started with; changing it restarts the player.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerConfig {
    pub binary: String,
    pub extra_args: Vec<String>,
    /// Where mpv writes its log (see [`crate::logging`]).
    pub log_file: Option<std::path::PathBuf>,
    pub role: Role,
    /// Bumped to restart mpv with the same settings ("Reload mpv").
    pub generation: u32,
}

/// Which mpv this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A music deck: 0 plays normally, 1 joins for DJ transitions.
    Deck(u8),
    /// The AI DJ's voice player (talks over the music).
    Voice,
}

impl PlayerConfig {
    pub fn from_settings(s: &listen_core::settings::Playback) -> Self {
        let configured = s.mpv_path.trim();
        let binary = std::env::var("LISTEN_MPV").unwrap_or_else(|_| {
            if configured.is_empty() {
                listen_core::platform::tool("mpv").to_string_lossy().into_owned()
            } else {
                configured.to_string()
            }
        });
        // `--cover-art-auto` etc. from Feishin presets are harmless; keep them.
        let mut extra_args = s.mpv_extra_args.clone();
        // Developer/testing hook, e.g. LISTEN_MPV_ARGS="--ao=null".
        if let Ok(more) = std::env::var("LISTEN_MPV_ARGS") {
            extra_args.extend(more.split_whitespace().map(String::from));
        }
        Self { binary, extra_args, log_file: crate::logging::mpv_log(), role: Role::Deck(0), generation: 0 }
    }

    /// The same mpv, as the AI DJ's voice player.
    pub fn voice(&self) -> Self {
        // No extra args (audio filters etc. are for the music) and no log file (the music player
        // owns it); the testing hook still applies (e.g. `--speed` in headless runs).
        let extra_args = std::env::var("LISTEN_MPV_ARGS")
            .map(|a| a.split_whitespace().filter(|a| *a != "--ao=null").map(String::from).collect())
            .unwrap_or_default();
        Self {
            binary: self.binary.clone(),
            extra_args,
            log_file: None,
            role: Role::Voice,
            generation: self.generation,
        }
    }

    /// The same mpv as the second music deck (DJ transitions).
    pub fn deck_b(&self) -> Self {
        // Its own log would clash with deck 0's file.
        Self { log_file: None, role: Role::Deck(1), ..self.clone() }
    }
}

#[derive(Debug, Clone)]
pub enum PlayerMsg {
    Ready(Player),
    Failed(String),
    Event(PlayerEvent),
}

pub fn subscription(config: PlayerConfig) -> Subscription<Message> {
    Subscription::run_with(config, worker)
}

fn worker(config: &PlayerConfig) -> impl Stream<Item = Message> + use<> {
    let config = config.clone();
    iced::stream::channel(64, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
        let voice = config.role == Role::Voice;
        let wrap = move |msg| match config.role {
            Role::Deck(n) => Message::Deck(n, msg),
            Role::Voice => Message::Voice(msg),
        };
        // Tests and headless runs pass `--ao=null` for the music; the voice follows.
        let null_audio = voice && std::env::var("LISTEN_MPV_ARGS").is_ok_and(|a| a.contains("--ao=null"));
        let opts = MpvOptions {
            binary: config.binary.into(),
            extra_args: config.extra_args,
            null_audio,
            log_file: config.log_file,
            // The visualizer captures the decks by these names (never the voice).
            client_name: match config.role {
                Role::Deck(0) => "Listen".into(),
                Role::Deck(_) => "Listen B".into(),
                Role::Voice => "Listen DJ".into(),
            },
            audio_buffer: None,
            video: false,
        };
        match Player::spawn(&opts).await {
            Ok((player, mut events)) => {
                let _ = out.send(wrap(PlayerMsg::Ready(player))).await;
                while let Some(event) = events.recv().await {
                    let crashed = event == PlayerEvent::Crashed;
                    // The voice player's clock isn't needed.
                    if voice && matches!(event, PlayerEvent::Position(_)) {
                        continue;
                    }
                    if out.send(wrap(PlayerMsg::Event(event))).await.is_err() || crashed {
                        break;
                    }
                }
            }
            Err(e) => {
                let _ = out.send(wrap(PlayerMsg::Failed(e.to_string()))).await;
            }
        }
        // Keep the stream alive so iced doesn't restart the worker in a loop.
        std::future::pending::<()>().await;
    })
}
