//! The Milkdrop subscription: captures Listen's own decks like the spectrum
//! does, feeds the render thread and turns what it reports into messages.
//! It exists only while Milkdrop is on screen, the window is shown and music
//! plays; dropping it stops the capture and the render thread.

use super::engine::{Engine, HEIGHT, Out, WIDTH};
use crate::app::Message;
use crate::app::milkdrop::MilkdropMsg;
use iced::Subscription;
use iced::futures::{SinkExt, Stream};

/// PipeWire node names of the music decks (as for the spectrum).
const DECKS: [&str; 2] = ["Listen", "Listen B"];

pub fn subscription(both_decks: bool) -> Subscription<Message> {
    Subscription::run_with(both_decks, worker)
}

struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn worker(both_decks: &bool) -> impl Stream<Item = Message> + use<> {
    let decks: &'static [&'static str] = if *both_decks { &DECKS } else { &DECKS[..1] };
    iced::stream::channel(8, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Out>(4);
        let engine = Engine::start(tx);
        let (ptx, mut prx) = tokio::sync::mpsc::channel::<(usize, Vec<f32>)>(64);
        let mut capture =
            Some(tokio::spawn(async move { listen_visualizer::capture::pipewire_many(decks, ptx).await }));
        let _stop = AbortOnDrop(capture.as_ref().map(|c| c.abort_handle()).expect("just spawned"));
        let mut capturing = true;
        loop {
            let msg = tokio::select! {
                chunk = prx.recv(), if capturing => match chunk {
                    Some((_, samples)) => {
                        engine.feed(samples);
                        continue;
                    }
                    None => {
                        capturing = false;
                        let reason = match capture.take() {
                            Some(c) => match c.await {
                                Ok(Err(e)) => e,
                                _ => "audio capture stopped".to_string(),
                            },
                            None => "audio capture stopped".to_string(),
                        };
                        MilkdropMsg::Error(reason)
                    }
                },
                got = rx.recv() => match got {
                    Some(Out::Frame(rgba)) => MilkdropMsg::Frame(iced::widget::image::Handle::from_rgba(
                        WIDTH as u32,
                        HEIGHT as u32,
                        rgba,
                    )),
                    Some(Out::Preset(name)) => MilkdropMsg::Preset(name),
                    Some(Out::Failed(name, why)) => MilkdropMsg::Failed(name, why),
                    Some(Out::NoPresets) => MilkdropMsg::NoPresets,
                    Some(Out::Error(e)) => MilkdropMsg::Error(e),
                    None => break,
                },
            };
            if out.send(Message::Milkdrop(msg)).await.is_err() {
                break;
            }
        }
        std::future::pending::<()>().await;
    })
}
