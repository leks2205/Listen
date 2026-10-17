//! Keeps the screen on / the system awake while music plays (Feishin's
//! `preventSleepOnPlayback` and `preventSuspendOnPlayback`), through the
//! desktop's Inhibit portal. One inhibitor at a time: taken when playback
//! starts, released when it pauses or stops.

/// What to hold off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    #[default]
    None,
    /// Keep the system from suspending.
    System,
    /// Keep the display on (and the system awake).
    Display,
}

impl Level {
    /// The level for the settings and playback state. Display wins over system.
    pub fn wanted(playing: bool, display: bool, system: bool) -> Self {
        match (playing, display, system) {
            (false, ..) => Level::None,
            (true, true, _) => Level::Display,
            (true, false, true) => Level::System,
            (true, false, false) => Level::None,
        }
    }
}

/// Takes inhibitors. Dropping the returned guard releases it.
pub trait Backend: Send {
    fn acquire(&mut self, level: Level) -> Option<Box<dyn Send>>;
}

/// The state machine: at most one guard, matching the last [`Level`].
pub struct Inhibitor {
    backend: Box<dyn Backend>,
    level: Level,
    guard: Option<Box<dyn Send>>,
}

impl std::fmt::Debug for Inhibitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inhibitor").field("level", &self.level).finish()
    }
}

impl Inhibitor {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend, level: Level::None, guard: None }
    }

    /// An inhibitor that never takes anything (demo, tests, other platforms).
    pub fn disabled() -> Self {
        Self::new(Box::new(Disabled))
    }

    /// The desktop's portal where there is one.
    pub fn system() -> Self {
        #[cfg(target_os = "linux")]
        return Self::new(Box::new(portal::Portal));
        #[cfg(not(target_os = "linux"))]
        Self::disabled()
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn held(&self) -> bool {
        self.guard.is_some()
    }

    /// Moves to `level`: releases the old guard first, then takes a new one.
    pub fn set(&mut self, level: Level) {
        if level == self.level {
            return;
        }
        self.guard = None;
        self.level = level;
        if level != Level::None {
            self.guard = self.backend.acquire(level);
        }
    }
}

struct Disabled;

impl Backend for Disabled {
    fn acquire(&mut self, _: Level) -> Option<Box<dyn Send>> {
        None
    }
}

#[cfg(target_os = "linux")]
mod portal {
    use super::{Backend, Level};
    use ashpd::desktop::inhibit::{InhibitFlags, InhibitProxy};
    use std::sync::OnceLock;
    use tokio::sync::oneshot;

    /// The portal talks over D-Bus on a small runtime of its own (the app's
    /// update loop is not inside one).
    fn runtime() -> &'static tokio::runtime::Runtime {
        static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .thread_name("inhibit")
                .build()
                .expect("inhibit runtime")
        })
    }

    pub(super) struct Portal;

    /// Dropping it ends the task, which closes the portal request.
    struct Guard(Option<oneshot::Sender<()>>);

    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    impl Backend for Portal {
        fn acquire(&mut self, level: Level) -> Option<Box<dyn Send>> {
            let flags = match level {
                Level::None => return None,
                Level::System => InhibitFlags::Suspend.into(),
                Level::Display => InhibitFlags::Suspend | InhibitFlags::Idle,
            };
            let (tx, rx) = oneshot::channel();
            runtime().spawn(async move {
                let request = match async {
                    let proxy = InhibitProxy::new().await?;
                    proxy.inhibit(None, flags, "Playing music").await
                }
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("sleep inhibit unavailable: {e}");
                        return;
                    }
                };
                let _ = rx.await;
                let _ = request.close().await;
            });
            Some(Box::new(Guard(Some(tx))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<String>>>);

    impl Log {
        fn events(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    struct Fake(Log);
    struct FakeGuard(Level, Log);

    impl Drop for FakeGuard {
        fn drop(&mut self) {
            self.1.0.lock().unwrap().push(format!("release {:?}", self.0));
        }
    }

    impl Backend for Fake {
        fn acquire(&mut self, level: Level) -> Option<Box<dyn Send>> {
            (self.0).0.lock().unwrap().push(format!("acquire {level:?}"));
            Some(Box::new(FakeGuard(level, self.0.clone())))
        }
    }

    #[test]
    fn playing_inhibits_and_pausing_releases() {
        let log = Log::default();
        let mut i = Inhibitor::new(Box::new(Fake(log.clone())));
        i.set(Level::wanted(false, false, true));
        assert!(!i.held());
        i.set(Level::wanted(true, false, true));
        assert!(i.held());
        // Same level again: nothing new is taken.
        i.set(Level::wanted(true, false, true));
        i.set(Level::wanted(false, false, true));
        assert!(!i.held());
        assert_eq!(log.events(), ["acquire System", "release System"]);
    }

    #[test]
    fn changing_the_kind_swaps_the_guard() {
        let log = Log::default();
        let mut i = Inhibitor::new(Box::new(Fake(log.clone())));
        i.set(Level::System);
        i.set(Level::Display);
        i.set(Level::None);
        assert_eq!(log.events(), ["acquire System", "release System", "acquire Display", "release Display"]);
    }

    #[test]
    fn nothing_is_taken_when_both_settings_are_off() {
        let log = Log::default();
        let mut i = Inhibitor::new(Box::new(Fake(log.clone())));
        i.set(Level::wanted(true, false, false));
        assert!(log.events().is_empty());
        assert_eq!(Level::wanted(true, true, true), Level::Display);
    }

    #[test]
    fn dropping_releases() {
        let log = Log::default();
        let mut i = Inhibitor::new(Box::new(Fake(log.clone())));
        i.set(Level::Display);
        drop(i);
        assert_eq!(log.events(), ["acquire Display", "release Display"]);
    }
}
