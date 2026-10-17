//! Watches the desktop theme (KDE colour scheme / rice theme switcher) for "Follow desktop theme" by
//! polling two files once a second.

use crate::app::Message;
use iced::Subscription;
use iced::futures::SinkExt;
use listen_core::theme::desktop;
use std::time::Duration;

pub fn subscription() -> Subscription<Message> {
    Subscription::run(watch)
}

fn watch() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(4, async |mut out: iced::futures::channel::mpsc::Sender<Message>| {
        let mut last_stamp = None;
        let mut last_theme = None;
        loop {
            if let Some(dir) = desktop::config_dir() {
                let stamp = desktop::stamp(&dir);
                if last_stamp.as_ref() != Some(&stamp) {
                    last_stamp = Some(stamp);
                    let theme = desktop::read(&dir);
                    if last_theme.as_ref() != Some(&theme) {
                        tracing::info!(
                            "desktop theme: {}",
                            theme.as_ref().map_or("none", |t| t.name.as_str())
                        );
                        last_theme = Some(theme.clone());
                        if out.send(Message::DesktopTheme(theme)).await.is_err() {
                            return;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}
