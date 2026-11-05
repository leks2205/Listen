//! Focus mode (`auto_dj.focus`): automatic picks prefer little lead vocal and calm energy
//! ([`listen_mix::pick::focus_pool`] over the analysis' sung-bar share), low-vocal songs from the
//! listening history join the Auto-DJ's pool, sung YouTube Music picks are swapped for their
//! instrumental version when one exists ([`listen_core::versions::instrumental_of`]), and the AI DJ
//! doesn't talk. Songs started by hand play as they are. Off: the next fill is an ordinary one again.

use super::flow::FlowLookup;
use super::{Listen, Message};
use crate::app::settings_msg::Setting;
use iced::Task;
use listen_core::analysis::FlowFeatures;
use listen_core::history::Picker;
use listen_core::model::Song;
use listen_core::queue::{Why, WhyCode};
use listen_core::versions::{instrumental_of, instrumental_query, song_key};
use listen_mix::pick::{FOCUS_MAX_VOCAL, focus_cost, focus_extras, focus_pool, wants_instrumental};
use std::collections::HashSet;

/// History entries (newest first) searched for low-vocal songs for an Auto-DJ fill.
const HISTORY_LOOK: usize = 5000;
/// The least sung this many history songs join one fill's pool.
const HISTORY_EXTRA: usize = 30;
/// Instrumental-version searches per fill.
const INSTRUMENTAL_LOOKUPS: usize = 3;

#[derive(Debug, Clone)]
pub enum FocusMsg {
    /// The Focus button.
    Toggle,
    Set(bool),
    /// The instrumental version found (or not) for queue item `uid`.
    Instrumental(u64, Option<Box<Song>>),
}

/// The songs of `pool` fit for focus, best first (at least `want`, see [`focus_pool`]).
pub(super) fn focus_songs(pool: Vec<Song>, lookup: &FlowLookup, want: usize) -> Vec<Song> {
    let annotated: Vec<(Song, Option<FlowFeatures>)> = pool
        .into_iter()
        .map(|s| {
            let f = lookup(&s);
            (s, f)
        })
        .collect();
    focus_pool(&annotated, want).into_iter().map(|i| annotated[i].0.clone()).collect()
}

/// The instrumental version of `song` in the demo library (the YouTube Music demo album has one).
fn demo_instrumental(song: &Song, query: &str) -> Option<Song> {
    let library: Vec<Song> = (0..crate::demo::albums().len())
        .flat_map(crate::demo::album_songs)
        .chain(crate::demo::ytm_album().songs)
        .collect();
    let found = instrumental_of(song, &library);
    log_instrumental(song, query, found.as_ref(), library.len());
    found
}

/// One log line per instrumental search, found or not (QA reads these).
fn log_instrumental(song: &Song, query: &str, found: Option<&Song>, results: usize) {
    match found {
        Some(inst) => tracing::info!(
            "Focus: instrumental of “{}” found: “{}” (query “{query}”)",
            song.title,
            inst.title
        ),
        None => tracing::info!(
            "Focus: no instrumental of “{}” among {results} results (query “{query}”)",
            song.title
        ),
    }
}

impl Listen {
    pub fn focus_on(&self) -> bool {
        self.config.settings.auto_dj.focus
    }

    /// The Focus chip beside the play-mode button: offered in Auto-DJ and AI DJ, and while on (so it can be
    /// switched off).
    pub fn focus_chip_shown(&self) -> bool {
        self.focus_on()
            || matches!(
                self.play_mode(),
                super::play_mode::PlayMode::AutoDj | super::play_mode::PlayMode::AiDj
            )
    }

    /// Little lead vocal (or an instrumental version) and calm.
    pub(super) fn focus_friendly(&self, song: &Song) -> bool {
        focus_cost(song, self.flow_lookup()(song).as_ref()) <= FOCUS_MAX_VOCAL
    }

    /// An Auto-DJ fill in focus mode: the fetched songs plus the least sung analysed songs from the history
    /// (not queued, played this session or blocked), narrowed to the focus-friendly ones (at least `n`).
    pub(super) fn focus_fill(
        &self,
        picked: Vec<Song>,
        queued: &HashSet<String>,
        blocked: &HashSet<String>,
        n: usize,
    ) -> Vec<Song> {
        let lookup = self.flow_lookup();
        let mut seen: HashSet<String> = picked.iter().map(song_key).collect();
        let mut pool = picked;
        let mut history: Vec<(Song, Option<FlowFeatures>)> = Vec::new();
        for e in self.history.entries.iter().rev().take(HISTORY_LOOK) {
            let song = e.to_song();
            let key = song_key(&song);
            if seen.contains(&key)
                || queued.contains(&key)
                || blocked.contains(&key)
                || self.session_played.contains(&key)
                || !(self.demo || self.sources.contains_key(&song.server_id))
            {
                continue;
            }
            seen.insert(key);
            let f = lookup(&song);
            history.push((song, f));
        }
        let extra = focus_extras(&history, HISTORY_EXTRA);
        let extra_n = extra.len();
        pool.extend(extra.into_iter().map(|i| history[i].0.clone()));
        let out = focus_songs(pool, &lookup, n);
        tracing::info!(
            "Focus: {} candidates for the fill ({extra_n} from the history), {} focus-friendly",
            out.len(),
            out.iter().filter(|s| focus_cost(s, lookup(s).as_ref()) <= FOCUS_MAX_VOCAL).count()
        );
        out
    }

    /// Looks for instrumental versions of the sung songs among the queue items `uids` (a few per fill).
    pub(super) fn focus_instrumentals(&self, uids: &[u64]) -> Task<Message> {
        self.instrumental_lookups(uids, INSTRUMENTAL_LOOKUPS).1
    }

    /// Searches instrumental versions for up to `cap` sung songs among the queue items `uids`; returns the
    /// searched `(uid, query)` pairs. In demo mode the demo library is searched instead of a server.
    pub(super) fn instrumental_lookups(
        &self,
        uids: &[u64],
        cap: usize,
    ) -> (Vec<(u64, String)>, Task<Message>) {
        let lookup = self.flow_lookup();
        let mut searched = Vec::new();
        let mut tasks = Vec::new();
        for &uid in uids {
            if tasks.len() == cap {
                break;
            }
            let Some(song) = self.queue.iter().find(|i| i.uid == uid).map(|i| i.song.clone()) else {
                continue;
            };
            if !wants_instrumental(&song, lookup(&song).as_ref()) {
                continue;
            }
            let query = instrumental_query(&song);
            if self.demo {
                let found = demo_instrumental(&song, &query);
                searched.push((uid, query));
                tasks.push(Task::done(Message::Focus(FocusMsg::Instrumental(uid, found.map(Box::new)))));
                continue;
            }
            let Some(source) = self.sources.get(&song.server_id).cloned() else { continue };
            searched.push((uid, query.clone()));
            tasks.push(Task::perform(
                async move {
                    let results = match source.search(&query, 10).await {
                        Ok(r) => r.songs,
                        Err(e) => {
                            tracing::info!("Focus: instrumental search “{query}” failed: {e}");
                            return None;
                        }
                    };
                    let found = instrumental_of(&song, &results);
                    log_instrumental(&song, &query, found.as_ref(), results.len());
                    found.map(Box::new)
                },
                move |found| Message::Focus(FocusMsg::Instrumental(uid, found)),
            ));
        }
        (searched, Task::batch(tasks))
    }

    pub(super) fn on_focus(&mut self, msg: FocusMsg) -> Task<Message> {
        match msg {
            FocusMsg::Toggle => self.on_focus(FocusMsg::Set(!self.focus_on())),
            FocusMsg::Set(on) if on == self.focus_on() => Task::none(),
            FocusMsg::Set(on) => self.set_focus(on),
            FocusMsg::Instrumental(uid, found) => self.focus_swap(uid, found),
        }
    }

    fn set_focus(&mut self, on: bool) -> Task<Message> {
        let setting = self.change_setting(Setting::Focus(on));
        if !on {
            self.toast_info("Focus off: the DJs pick as usual again.".into());
            return setting;
        }
        self.toast_success("Focus on: songs with little singing, instrumental versions, no DJ talk.".into());
        // A talk break already planned doesn't air.
        self.drop_break();
        // Upcoming DJ picks that are sung go (not the next one, it may be mixing in already).
        let lookup = self.flow_lookup();
        let current = self.queue.current_position().unwrap_or(0);
        let drop: Vec<u64> = self
            .queue
            .iter()
            .skip(current + 2)
            .filter(|i| i.auto && i.bridge.is_none())
            .filter(|i| focus_cost(&i.song, lookup(&i.song).as_ref()) > FOCUS_MAX_VOCAL)
            .map(|i| i.uid)
            .collect();
        self.queue.remove_many(&drop);
        if !drop.is_empty() {
            tracing::info!("Focus: dropped {} sung DJ picks", drop.len());
        }
        let refill = if self.ai_dj_on() { Task::none() } else { self.maybe_autodj() };
        Task::batch([setting, refill])
    }

    /// Puts the instrumental version in place of queue item `uid` (not while it plays).
    fn focus_swap(&mut self, uid: u64, found: Option<Box<Song>>) -> Task<Message> {
        let Some(inst) = found else { return Task::none() };
        if !self.focus_on() || self.queue.current().is_some_and(|i| i.uid == uid) {
            return Task::none();
        }
        let Some(picker) = self.queue.iter().find(|i| i.uid == uid).map(|i| i.why.as_ref().map(|w| w.picker))
        else {
            return Task::none();
        };
        let was_next = self.queue.peek_next().is_some_and(|i| i.uid == uid);
        tracing::info!("Focus: instrumental version “{}” queued", inst.title);
        self.queue.replace_song(uid, *inst);
        self.queue.set_why(uid, Some(Why::new(picker.unwrap_or(Picker::AutoDj), WhyCode::FocusFriendly)));
        if was_next { self.refresh_next() } else { Task::none() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Args;
    use crate::demo;

    fn rig() -> Listen {
        let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        s.history.entries.clear();
        s
    }

    #[test]
    fn the_button_switches_focus_and_the_setting() {
        let mut s = rig();
        assert!(!s.focus_on());
        let _ = s.update(Message::Focus(FocusMsg::Toggle));
        assert!(s.focus_on() && s.config.settings.auto_dj.focus);
        // No forgotten-favourite slot while focusing.
        assert_eq!(s.config.settings.auto_dj.familiar_slot(), None);
        let _ = s.update(Message::Focus(FocusMsg::Toggle));
        assert!(!s.focus_on());
    }

    #[test]
    fn an_instrumental_takes_the_place_of_a_queued_pick() {
        let mut s = rig();
        let songs = demo::album_songs(0);
        s.queue.set(songs[..1].to_vec(), 0);
        let _ = s.update(Message::Focus(FocusMsg::Set(true)));
        let uids = s.queue.add_picked(
            songs[1..3].iter().map(|x| (x.clone(), Why::new(Picker::AutoDj, WhyCode::NewToYou))).collect(),
        );
        let mut inst = songs[2].clone();
        inst.id = "inst".into();
        inst.title = format!("{} (Inst.)", inst.title);
        let _ = s.update(Message::Focus(FocusMsg::Instrumental(uids[1], Some(Box::new(inst.clone())))));
        let item = s.queue.iter().find(|i| i.uid == uids[1]).unwrap();
        assert_eq!(item.song.id, "inst");
        assert!(item.auto);
        assert_eq!(item.why.as_ref().map(|w| w.code.clone()), Some(WhyCode::FocusFriendly));
        // Not for the playing song, and not once focus is off.
        let current = s.queue.current().unwrap().uid;
        let _ = s.update(Message::Focus(FocusMsg::Instrumental(current, Some(Box::new(inst.clone())))));
        assert_ne!(s.queue.current().unwrap().song.id, "inst");
        let _ = s.update(Message::Focus(FocusMsg::Set(false)));
        let _ = s.update(Message::Focus(FocusMsg::Instrumental(uids[0], Some(Box::new(inst)))));
        assert_ne!(s.queue.iter().find(|i| i.uid == uids[0]).unwrap().song.id, "inst");
    }

    #[test]
    fn the_demo_library_gives_a_deterministic_instrumental_swap() {
        let mut s = rig();
        let ytm = demo::ytm_album().songs;
        s.queue.set(ytm[1..2].to_vec(), 0);
        let _ = s.update(Message::Focus(FocusMsg::Set(true)));
        let uids = s.queue.add_picked(vec![(ytm[0].clone(), Why::new(Picker::AutoDj, WhyCode::NewToYou))]);
        let (searched, _) = s.instrumental_lookups(&uids, 10);
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].0, uids[0]);
        let found = demo_instrumental(&ytm[0], &searched[0].1).expect("Glass Cities (Instrumental)");
        assert_eq!(found.title, "Glass Cities (Instrumental)");
        let _ = s.update(Message::Focus(FocusMsg::Instrumental(uids[0], Some(Box::new(found)))));
        assert_eq!(
            s.queue.iter().find(|i| i.uid == uids[0]).unwrap().song.title,
            "Glass Cities (Instrumental)"
        );
        // An instrumental isn't searched again.
        assert!(s.instrumental_lookups(&uids, 10).0.is_empty());
    }

    #[test]
    fn with_focus_on_the_next_ai_dj_set_is_never_voiced() {
        let mut s = rig();
        s.config.settings.ai_dj.talk_every = 1;
        let before = s.dj_voice_next();
        let _ = s.update(Message::Focus(FocusMsg::Set(true)));
        assert!(!s.dj_voice_next());
        assert!(s.focus_chip_shown());
        let _ = s.update(Message::Focus(FocusMsg::Set(false)));
        assert_eq!(s.dj_voice_next(), before);
    }
}
