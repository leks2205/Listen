//! Windows tray icon (`tray-icon`), same shape as the Linux `tray` module: Feishin's
//! menu, and a click on the icon toggles the window. The icon runs on its own thread
//! with a Win32 message loop (required on the creating thread); the app talks to it
//! through a channel.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};
use std::sync::mpsc as std_mpsc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

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

/// The 64 px app icon (rendered from `packaging/io.github.leks2205.Listen.svg`).
const ICON_PNG: &[u8] = include_bytes!("../../packaging/windows/tray-64.png");

fn icon() -> Option<tray_icon::Icon> {
    let img = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Handle for updating the tray from the app.
#[derive(Clone)]
pub struct TrayHandle {
    updates: std_mpsc::Sender<(bool, String)>,
    thread_id: u32,
}

impl std::fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrayHandle")
    }
}

impl TrayHandle {
    pub async fn set(self, playing: bool, title: String) {
        if self.updates.send((playing, title)).is_ok() {
            // SAFETY: plain Win32 call; a thread that is gone just fails.
            unsafe { wm::PostThreadMessageW(self.thread_id, wm::WM_APP, 0, 0) };
        }
    }
}

#[derive(Debug, Clone)]
pub enum TrayMsg {
    Ready(TrayHandle),
    Command(TrayCmd),
    /// The tray icon couldn't be created.
    Unavailable,
}

fn run_thread(
    commands: mpsc::UnboundedSender<TrayCmd>,
    updates: std_mpsc::Receiver<(bool, String)>,
    ready: std_mpsc::Sender<Result<u32, String>>,
) {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    let menu = Menu::new();
    let items: Vec<(Option<MenuItem>, Option<TrayCmd>)> = menu_labels(false)
        .into_iter()
        .zip([
            Some(TrayCmd::PlayPause),
            Some(TrayCmd::Next),
            Some(TrayCmd::Previous),
            Some(TrayCmd::Stop),
            None,
            Some(TrayCmd::Show),
            Some(TrayCmd::Quit),
        ])
        .map(|(label, cmd)| (label.map(|l| MenuItem::new(l, true, None)), cmd))
        .collect();
    for (item, _) in &items {
        let added = match item {
            Some(item) => menu.append(item),
            None => menu.append(&PredefinedMenuItem::separator()),
        };
        if let Err(e) = added {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    }
    let ids: Vec<(tray_icon::menu::MenuId, TrayCmd)> =
        items.iter().filter_map(|(i, c)| Some((i.as_ref()?.id().clone(), (*c)?))).collect();
    let menu_tx = commands.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, cmd)) = ids.iter().find(|(id, _)| *id == event.id) {
            let _ = menu_tx.unbounded_send(*cmd);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left, button_state: MouseButtonState::Up, ..
        } = event
        {
            let _ = commands.unbounded_send(TrayCmd::Toggle);
        }
    }));
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Listen");
    if let Some(icon) = icon() {
        builder = builder.with_icon(icon);
    }
    let tray = match builder.build() {
        Ok(t) => t,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    // SAFETY: plain Win32 calls; creating the queue before anyone posts to it.
    let thread_id = unsafe {
        let mut msg = std::mem::zeroed();
        wm::PeekMessageW(&mut msg, std::ptr::null_mut(), wm::WM_USER, wm::WM_USER, wm::PM_NOREMOVE);
        GetCurrentThreadId()
    };
    let _ = ready.send(Ok(thread_id));
    // SAFETY: the standard message loop; `msg` is only used by these calls.
    unsafe {
        let mut msg = std::mem::zeroed();
        while wm::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == wm::WM_APP {
                while let Ok((playing, title)) = updates.try_recv() {
                    if let Some(Some(play)) = items.first().map(|(i, _)| i.as_ref()) {
                        play.set_text(menu_labels(playing)[0].unwrap_or("Play"));
                    }
                    let tip =
                        if title.is_empty() { "Listen".to_string() } else { format!("Listen\n{title}") };
                    let _ = tray.set_tooltip(Some(tip));
                }
            } else {
                wm::TranslateMessage(&msg);
                wm::DispatchMessageW(&msg);
            }
        }
    }
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async move |mut out: mpsc::Sender<Message>| {
        let (tx, mut rx) = mpsc::unbounded();
        let (update_tx, update_rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || run_thread(tx, update_rx, ready_tx));
        let ready = match spawned {
            Ok(_) => tokio::task::spawn_blocking(move || ready_rx.recv()).await.ok().and_then(Result::ok),
            Err(e) => Some(Err(e.to_string())),
        };
        match ready {
            Some(Ok(thread_id)) => {
                let handle = TrayHandle { updates: update_tx, thread_id };
                let _ = out.send(Message::Tray(TrayMsg::Ready(handle))).await;
                while let Some(cmd) = rx.next().await {
                    if out.send(Message::Tray(TrayMsg::Command(cmd))).await.is_err() {
                        break;
                    }
                }
            }
            other => {
                tracing::info!("tray unavailable: {:?}", other.and_then(Result::err));
                let _ = out.send(Message::Tray(TrayMsg::Unavailable)).await;
            }
        }
        std::future::pending::<()>().await;
    })
}
