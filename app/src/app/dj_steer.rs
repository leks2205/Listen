//! "Tell the DJ": the request box and chips under the queue while the AI DJ is on. A changed request
//! replans the not-yet-started part of the set ([`Listen::dj_replan`]); the same request again changes
//! nothing (no Gemini call). Requests are saved and stay until the listener removes them.

use super::{Listen, Message};
use iced::Task;
use listen_dj::{Chip, Steer};

#[derive(Debug, Clone)]
pub enum SteerMsg {
    /// The text box changed.
    Input(String),
    /// Enter in the text box: the typed request.
    Submit,
    /// A chip was clicked (adds it, or removes it when active).
    Chip(Chip),
    /// The × on an active request.
    Remove(usize),
    /// Remove every request.
    Clear,
}

impl Listen {
    pub(super) fn on_steer(&mut self, msg: SteerMsg) -> Task<Message> {
        let changed = match msg {
            SteerMsg::Input(text) => {
                self.dj.request_input = text;
                false
            }
            SteerMsg::Submit => {
                let text = std::mem::take(&mut self.dj.request_input);
                Steer::text(&text).is_some_and(|s| self.dj.requests.add(s))
            }
            SteerMsg::Chip(chip) => {
                self.dj.requests.toggle(chip);
                true
            }
            SteerMsg::Remove(i) => self.dj.requests.remove_at(i),
            SteerMsg::Clear => self.dj.requests.clear(),
        };
        if !changed {
            return Task::none();
        }
        self.steer_save();
        self.dj_replan()
    }

    /// Adds a request from MCP or the CLI (a chip id or free text); `false` when nothing changed.
    pub fn steer_request(&mut self, request: &str) -> (bool, Task<Message>) {
        let Some(steer) = Chip::from_id(request).map(Steer::Chip).or_else(|| Steer::text(request)) else {
            return (false, Task::none());
        };
        if !self.dj.requests.add(steer) {
            return (false, Task::none());
        }
        self.steer_save();
        (true, self.dj_replan())
    }

    fn steer_save(&self) {
        if let Some(file) = self.requests_file()
            && let Err(e) = self.dj.requests.save(&file)
        {
            tracing::warn!("could not save the AI DJ requests: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Args;
    use crate::app::dj::Blocker;
    use crate::demo;

    fn rig() -> Listen {
        let (s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        s
    }

    #[test]
    fn a_request_replaces_the_queued_dj_songs_but_not_the_current_or_own_ones() {
        let mut s = rig();
        let songs = demo::album_songs(0);
        s.queue.set(vec![songs[0].clone(), songs[1].clone()], 0);
        let why = listen_core::queue::Why::new(
            listen_core::queue::Picker::AiDj,
            listen_core::queue::WhyCode::AiDj { note: String::new() },
        );
        s.queue.add_picked(songs[2..5].iter().map(|x| (x.clone(), why.clone())).collect());
        // The DJ only steers while on (demo mode keeps it off): emulate it.
        s.demo = false;
        s.config.settings.ai_dj.enabled = true;
        let _ = s.on_steer(SteerMsg::Chip(Chip::Calmer));
        let left: Vec<String> = s.queue.iter().map(|i| i.song.id.clone()).collect();
        assert_eq!(left, vec![songs[0].id.clone(), songs[1].id.clone()], "current and own song stay");
        assert!(s.dj.requests.has(Chip::Calmer));
        assert!(s.dj.requests_fresh);
        // The same typed request again: nothing changes, no replan.
        s.queue.add_picked(vec![(songs[5].clone(), why)]);
        s.dj.request_input = "calmer".into();
        let _ = s.on_steer(SteerMsg::Submit);
        assert_eq!(s.queue.len(), 3, "an unchanged request keeps the queue");
        let _ = s.on_steer(SteerMsg::Remove(0));
        assert!(s.dj.requests.is_empty());
    }

    #[test]
    fn a_request_drops_the_next_dj_song_whose_mix_has_not_started() {
        let songs = demo::album_songs(0);
        for phase in [crate::app::mix::Phase::Planned, crate::app::mix::Phase::Ready] {
            let mut s = rig();
            s.queue.set(vec![songs[0].clone()], 0);
            s.queue.add_picked(songs[1..4].iter().map(|x| (x.clone(), dj_why())).collect());
            s.demo = false;
            s.config.settings.ai_dj.enabled = true;
            s.dj.key = Some("test-key".into());
            let fake =
                crate::app::substitute::tests::Fake::new(listen_core::model::SourceKind::Subsonic, vec![]);
            s.sources.insert(songs[0].server_id.clone(), std::sync::Arc::new(fake));
            s.mix.phase = phase;
            let _ = s.on_steer(SteerMsg::Chip(Chip::Calmer));
            let left: Vec<String> = s.queue.iter().map(|i| i.song.id.clone()).collect();
            assert_eq!(left, vec![songs[0].id.clone()], "{phase:?}: only the playing song stays");
            assert_eq!(s.mix.phase, crate::app::mix::Phase::Idle, "{phase:?}: the planned mix is cancelled");
            assert_eq!(
                s.dj_plan_blocker(0),
                Some(Blocker::AlreadyPlanning),
                "{phase:?}: the new set is planned to follow the playing song"
            );
        }
    }

    fn dj_why() -> listen_core::queue::Why {
        listen_core::queue::Why::new(
            listen_core::queue::Picker::AiDj,
            listen_core::queue::WhyCode::AiDj { note: String::new() },
        )
    }

    #[test]
    fn a_request_plans_a_new_set_behind_the_dj_song_whose_mix_has_started() {
        let mut s = rig();
        let songs = demo::album_songs(0);
        s.queue.set(vec![songs[0].clone()], 0);
        let why = dj_why();
        s.queue.add_picked(songs[1..4].iter().map(|x| (x.clone(), why.clone())).collect());
        s.demo = false;
        s.config.settings.ai_dj.enabled = true;
        s.dj.key = Some("test-key".into());
        let fake = crate::app::substitute::tests::Fake::new(listen_core::model::SourceKind::Subsonic, vec![]);
        s.sources.insert(songs[0].server_id.clone(), std::sync::Arc::new(fake));
        // The next DJ song's deck is loading (`Phase::decks_busy`): that song stays.
        s.mix.phase = crate::app::mix::Phase::Preloading;
        assert_eq!(s.dj_plan_blocker(0), Some(Blocker::SongsQueued));
        let _ = s.on_steer(SteerMsg::Chip(Chip::Calmer));
        let left: Vec<String> = s.queue.iter().map(|i| i.song.id.clone()).collect();
        assert_eq!(
            left,
            vec![songs[0].id.clone(), songs[1].id.clone()],
            "current and the mixed-into song stay"
        );
        assert_eq!(
            s.dj_plan_blocker(1),
            Some(Blocker::AlreadyPlanning),
            "the new set is being planned at once"
        );
        // Own songs still queued behind it hold the new set back until they have played.
        let mut t = rig();
        t.queue.set(vec![songs[0].clone()], 0);
        t.queue.add_picked(vec![(songs[1].clone(), why.clone())]);
        t.queue.add_last(vec![songs[5].clone()], false);
        t.demo = false;
        t.config.settings.ai_dj.enabled = true;
        t.dj.key = Some("test-key".into());
        t.mix.phase = crate::app::mix::Phase::Running;
        let _ = t.on_steer(SteerMsg::Chip(Chip::MoreEnergy));
        assert_eq!(t.dj_plan_blocker(1), Some(Blocker::SongsQueued));
    }

    #[test]
    fn requests_survive_a_restart_of_the_app() {
        let dir = std::env::temp_dir().join(format!("listen-steer-restart-{}", std::process::id()));
        let paths = listen_core::config::Paths {
            config_dir: dir.join("config"),
            cache_dir: dir.join("cache"),
            data_dir: dir.join("data"),
        };
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let mut s = rig();
        s.paths = Some(paths.clone());
        s.demo = false;
        s.config.settings.ai_dj.enabled = true;
        let _ = s.on_steer(SteerMsg::Chip(Chip::Calmer));
        assert!(paths.data_dir.join("ai-dj-requests.json").exists());
        // A new start reads them back (dj_boot), until they are removed.
        let mut t = rig();
        t.paths = Some(paths.clone());
        t.demo = false;
        t.dj_boot();
        assert!(t.dj.requests.has(Chip::Calmer));
        t.config.settings.ai_dj.enabled = true;
        let _ = t.on_steer(SteerMsg::Remove(0));
        let mut u = rig();
        u.paths = Some(paths);
        u.demo = false;
        u.dj_boot();
        assert!(u.dj.requests.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
