//! The search page: debounced "search as you type", the tab of the shown
//! list and the images of its rows. The page is `ui::search`; the command
//! palette (`app/palette.rs`) has its own live search.

use super::list::{ListId, SearchTab};
use super::*;

/// Feishin searches this long after the last keystroke (`useDebouncedValue(…, 400)`).
pub const DEBOUNCE: Duration = Duration::from_millis(400);
/// Results per type asked of the source for the page's lists.
pub const PAGE_LIMIT: usize = 100;

/// Hands out a token per keystroke; only the last token's timer acts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Debounce(u64);

impl Debounce {
    /// A keystroke: returns the token the timer of this keystroke carries.
    pub fn bump(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }

    /// True when no later keystroke happened since `token` was handed out.
    pub fn is_current(&self, token: u64) -> bool {
        self.0 == token
    }

    /// A task that sends `msg(token)` after [`DEBOUNCE`].
    pub fn after(token: u64, msg: fn(u64) -> Message) -> Task<Message> {
        Task::perform(
            async move {
                tokio::time::sleep(DEBOUNCE).await;
                token
            },
            msg,
        )
    }
}

#[derive(Debug, Clone)]
pub enum SearchMsg {
    /// The debounce timer of keystroke `token` ran out.
    Debounced(u64),
    Tab(SearchTab),
}

impl Listen {
    /// Text typed in the search page's box: search after a pause.
    pub(super) fn search_input(&mut self, text: String) -> Task<Message> {
        self.search.query = text;
        let token = self.search.debounce.bump();
        if self.search.query.trim().is_empty() {
            // Nothing to wait for: an emptied box clears the results.
            self.search.generation += 1;
            self.search.loading = false;
            self.search.error = None;
            self.search.results = Default::default();
            return Task::none();
        }
        Debounce::after(token, |t| Message::Search(SearchMsg::Debounced(t)))
    }

    /// Searches now (Enter, or the pause after typing); opens the search page.
    pub(super) fn run_search(&mut self) -> Task<Message> {
        let query = self.search.query.trim().to_string();
        let Some(source) = self.source.clone().filter(|_| !query.is_empty()) else { return Task::none() };
        // A pending timer of an earlier keystroke has nothing left to do.
        self.search.debounce.bump();
        self.search.generation += 1;
        self.search.loading = true;
        self.search.error = None;
        let generation = self.search.generation;
        let nav = if self.route != Route::Search { self.navigate(Route::Search) } else { Task::none() };
        Task::batch([
            nav,
            Task::perform(async move { source.search(&query, PAGE_LIMIT).await.map_err(err) }, move |r| {
                Message::SearchLoaded(generation, r)
            }),
        ])
    }

    pub(super) fn on_search(&mut self, msg: SearchMsg) -> Task<Message> {
        match msg {
            SearchMsg::Debounced(token) => {
                if self.search.debounce.is_current(token) {
                    return self.run_search();
                }
            }
            SearchMsg::Tab(tab) => {
                self.search.tab = tab;
                self.lists.reset(&ListId::Search(tab));
                return self.search_visible(tab);
            }
        }
        Task::none()
    }

    /// New results: the lists start at the top, the shown one loads its covers.
    pub(super) fn search_results_changed(&mut self) -> Task<Message> {
        for tab in [SearchTab::Songs, SearchTab::Albums, SearchTab::Artists, SearchTab::Playlists] {
            self.lists.reset(&ListId::Search(tab));
        }
        self.search_visible(self.search.tab)
    }

    /// Covers of the rows of the shown search list.
    pub(super) fn search_visible(&mut self, tab: SearchTab) -> Task<Message> {
        let id = ListId::Search(tab);
        let r = &self.search.results;
        let total = match tab {
            SearchTab::Songs => r.songs.len(),
            SearchTab::Albums => r.albums.len(),
            SearchTab::Artists => r.artists.len(),
            SearchTab::Playlists => r.playlists.len(),
        };
        let (first, last) = self.list_range(&id, total);
        let (first, last) = (first.min(total), last.min(total));
        let r = &self.search.results;
        let refs: Vec<ImageRef> = match tab {
            SearchTab::Songs => r.songs[first..last].iter().filter_map(|x| x.image.clone()).collect(),
            SearchTab::Albums => r.albums[first..last].iter().filter_map(|x| x.image.clone()).collect(),
            SearchTab::Artists => r.artists[first..last].iter().filter_map(|x| x.image.clone()).collect(),
            SearchTab::Playlists => r.playlists[first..last].iter().filter_map(|x| x.image.clone()).collect(),
        };
        self.request_images(refs.iter().map(Some))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;

    fn rig() -> Listen {
        Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() }).0
    }

    #[test]
    fn only_the_last_keystroke_searches() {
        let mut d = Debounce::default();
        let a = d.bump();
        let b = d.bump();
        let c = d.bump();
        assert!(!d.is_current(a) && !d.is_current(b));
        assert!(d.is_current(c));
        // Searching right away (Enter) cancels the pending timer.
        d.bump();
        assert!(!d.is_current(c));
    }

    #[test]
    fn typing_waits_and_an_emptied_box_clears_at_once() {
        let mut s = rig();
        let _ = s.update(Message::SearchInput("a".into()));
        let first = s.search.debounce.clone();
        let _ = s.update(Message::SearchInput("ab".into()));
        assert_ne!(first, s.search.debounce);
        assert_eq!(s.search.query, "ab");
        // Nothing was searched yet: the timer has not fired.
        assert!(!s.search.loading);
        // A stale timer (of "a") does nothing, the current one searches.
        let stale = s.search.debounce.clone();
        let _ = s.update(Message::Search(SearchMsg::Debounced(1)));
        assert!(!s.search.loading);
        s.search.results.songs = crate::demo::album_songs(0);
        let _ = s.update(Message::SearchInput(String::new()));
        assert!(s.search.results.songs.is_empty());
        assert_ne!(stale, s.search.debounce);
    }

    #[test]
    fn the_shown_tab_is_the_active_list_on_the_search_page() {
        let mut s = rig();
        s.route = Route::Search;
        assert_eq!(s.list_for_route(&s.route), Some(ListId::Search(SearchTab::Songs)));
        let _ = s.update(Message::Search(SearchMsg::Tab(SearchTab::Albums)));
        assert_eq!(s.list_for_route(&s.route), Some(ListId::Search(SearchTab::Albums)));
        s.route = Route::Albums;
        assert_eq!(s.list_for_route(&s.route), Some(ListId::Albums));
    }
}
