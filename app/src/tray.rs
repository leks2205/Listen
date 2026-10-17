//! System tray icon (StatusNotifierItem via `ksni`; GNOME needs the AppIndicator extension), with
//! Feishin's menu.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};
use ksni::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    PlayPause,
    Next,
    Previous,
    Stop,
    /// "Open main window".
    Show,
    /// A click on the icon.
    Toggle,
    Quit,
}

/// The menu's labels in order (`None` = separator).
pub fn menu_labels(playing: bool) -> Vec<Option<&'static str>> {
    vec![
        Some(if playing { "Pause" } else { "Play" }),
        Some("Next Track"),
        Some("Previous Track"),
        Some("Stop"),
        None,
        Some("Open main window"),
        Some("Quit"),
    ]
}

/// The app icon (rendered from `packaging/io.github.leks2205.Listen.svg`) at 64 and
/// 32 px, as ARGB32 in network byte order.
fn tray_pixmaps() -> &'static Vec<ksni::Icon> {
    static ICONS: std::sync::OnceLock<Vec<ksni::Icon>> = std::sync::OnceLock::new();
    ICONS.get_or_init(|| {
        let Ok(img) = image::load_from_memory(include_bytes!("../../packaging/windows/tray-64.png")) else {
            return Vec::new();
        };
        let img = img.to_rgba8();
        [64, 32]
            .into_iter()
            .map(|size| {
                let px = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                let data = px.pixels().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
                ksni::Icon { width: size as i32, height: size as i32, data }
            })
            .collect()
    })
}

struct ListenTray {
    tx: mpsc::UnboundedSender<TrayCmd>,
    playing: bool,
    title: String,
}

impl ksni::Tray for ListenTray {
    fn id(&self) -> String {
        listen_core::config::APP_ID.into()
    }
    fn title(&self) -> String {
        "Listen".into()
    }
    // A pixmap, not a theme icon name: Plasma caches theme icons until re-login.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        tray_pixmaps().clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Listen".into(), description: self.title.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.unbounded_send(TrayCmd::Toggle);
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: &str, cmd: TrayCmd| -> MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: Box::new(move |t: &mut Self| {
                    let _ = t.tx.unbounded_send(cmd);
                }),
                ..Default::default()
            }
            .into()
        };
        const CMDS: [Option<TrayCmd>; 7] = [
            Some(TrayCmd::PlayPause),
            Some(TrayCmd::Next),
            Some(TrayCmd::Previous),
            Some(TrayCmd::Stop),
            None,
            Some(TrayCmd::Show),
            Some(TrayCmd::Quit),
        ];
        menu_labels(self.playing)
            .into_iter()
            .zip(CMDS)
            .map(|(label, cmd)| match (label, cmd) {
                (Some(label), Some(cmd)) => item(label, cmd),
                _ => MenuItem::Separator,
            })
            .collect()
    }
}

/// Handle for updating the tray from the app.
#[derive(Clone)]
pub struct TrayHandle(ksni::Handle<ListenTray>);

impl std::fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrayHandle")
    }
}

impl TrayHandle {
    pub async fn set(self, playing: bool, title: String) {
        self.0
            .update(move |t| {
                t.playing = playing;
                t.title = title;
            })
            .await;
    }
}

#[derive(Debug, Clone)]
pub enum TrayMsg {
    Ready(TrayHandle),
    Command(TrayCmd),
    /// No tray host on the desktop.
    Unavailable,
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async move |mut out: mpsc::Sender<Message>| {
        let (tx, mut rx) = mpsc::unbounded();
        let tray = ListenTray { tx, playing: false, title: String::new() };
        match tray.spawn().await {
            Ok(handle) => {
                let _ = out.send(Message::Tray(TrayMsg::Ready(TrayHandle(handle)))).await;
                while let Some(cmd) = rx.next().await {
                    if out.send(Message::Tray(TrayMsg::Command(cmd))).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::info!("tray unavailable: {e}");
                let _ = out.send(Message::Tray(TrayMsg::Unavailable)).await;
            }
        }
        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_has_feishins_entries() {
        let labels = menu_labels(false);
        assert_eq!(
            labels,
            [
                Some("Play"),
                Some("Next Track"),
                Some("Previous Track"),
                Some("Stop"),
                None,
                Some("Open main window"),
                Some("Quit")
            ]
        );
        assert_eq!(menu_labels(true)[0], Some("Pause"));
    }
}
