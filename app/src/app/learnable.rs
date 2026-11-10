//! Known words in Korean lyrics and the "Learnable songs" mix (`listen_lyrics::known`).
//!
//! Every Korean song whose lyrics were seen (played with lyrics, or looked at by the mix) has
//! its words in `<cache>/lyrics-words.json`; the share of known words is recomputed from there
//! whenever Lute's word list (vocab-hub) changed. The mix page loads the liked songs, looks up
//! the lyrics of those not seen yet (a few at a time) and lists the ones at 90–98 % known words.

use super::{Listen, Message, Route};
use iced::Task;
use listen_core::model::Song;
use listen_lyrics::known::{self, Coverage, Store};
use listen_lyrics::vocab::{Knowledge, Words};
use listen_source::SongQuery;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Liked songs whose lyrics are looked up at once.
const BATCH: usize = 6;
/// The word list is asked for again after this long (Lute changes show up).
const VOCAB_FRESH: Duration = Duration::from_secs(300);

/// A looked-up song: key, lyric lines (`None`: no lyrics), dictionary forms by word.
pub type Looked = (String, Option<Vec<String>>, HashMap<String, String>);

#[derive(Debug, Clone)]
pub enum LearnMsg {
    Liked(Result<Vec<Song>, String>),
    Vocab(Option<Words>),
    /// Lyric lines (`None`: no lyrics) and dictionary forms per song key.
    Lyrics(Vec<Looked>),
    Play(usize),
    /// Hub counts for stored songs: word-list version, key → coverage.
    HubCounted(String, Vec<(String, Result<Coverage, String>)>),
    /// Hub classes of the shown song's words (song key, word → count).
    WordsCounted(String, Vec<(String, Coverage)>),
}

/// Songs counted by the hub at once.
const HUB_BATCH: usize = 8;

#[derive(Debug, Default)]
pub struct LearnState {
    store: Option<Store>,
    /// When the word list was last loaded (`lyrics_ui.vocab` holds it).
    pub vocab_at: Option<Instant>,
    vocab_loading: bool,
    /// The liked songs (loaded when the page opens).
    liked: Vec<Song>,
    /// The mix: songs and their coverage, easiest first.
    pub songs: Vec<Song>,
    pub cov: Vec<Coverage>,
    pub loading: bool,
    /// Liked songs looked at so far / to look at in this pass.
    pub checked: usize,
    pub to_check: usize,
    /// Hub counts (`/api/analyze`) are on their way; `hub_off`: the hub could not count.
    hub_busy: bool,
    hub_off: bool,
    /// The shown song's words as the hub classes them (song key, word → class).
    pub shown_key: String,
    pub shown: HashMap<String, Knowledge>,
    shown_busy: bool,
    pub error: Option<String>,
}

impl Listen {
    fn learn_store(&mut self) -> &mut Store {
        let dir = self.paths.as_ref().map(|p| p.cache_dir.clone());
        self.learn.store.get_or_insert_with(|| dir.map(|d| Store::load(&d)).unwrap_or_default())
    }

    fn learn_save(&self) {
        if let (Some(s), Some(p)) = (&self.learn.store, &self.paths)
            && let Err(e) = s.save(&p.cache_dir)
        {
            tracing::warn!("lyric words not saved: {e}");
        }
    }

    pub(crate) fn vocab_hub(&self) -> Option<String> {
        let base = self.config.settings.lyrics.vocab_hub_url.trim();
        (!base.is_empty()).then(|| base.to_string())
    }

    /// Whether the known-words features are on (a vocab-hub address is set).
    pub fn learn_available(&self) -> bool {
        self.vocab_hub().is_some()
    }

    /// Loads Lute's word list unless a fresh one is there.
    pub(crate) fn learn_vocab(&mut self) -> Task<Message> {
        let Some(base) = self.vocab_hub() else { return Task::none() };
        let fresh =
            self.lyrics_ui.vocab.is_some() && self.learn.vocab_at.is_some_and(|t| t.elapsed() < VOCAB_FRESH);
        if fresh || self.learn.vocab_loading {
            return Task::none();
        }
        self.learn.vocab_loading = true;
        let http = self.http.clone();
        Task::perform(
            async move {
                listen_lyrics::vocab::words(&http, &base)
                    .await
                    .map_err(|e| tracing::warn!("vocab-hub word list: {e}"))
                    .ok()
            },
            |w| Message::Learn(LearnMsg::Vocab(w)),
        )
    }

    /// The shown song's words go into the store (with the gloss dictionary forms, if any).
    pub(crate) fn learn_record_shown(&mut self) -> Task<Message> {
        if !self.learn_available() {
            return Task::none();
        }
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        let Some(lyrics) = &self.lyrics else { return Task::none() };
        let lines: Vec<String> = lyrics.lines.iter().map(|l| l.text.clone()).collect();
        if !listen_lyrics::gemini::has_hangul(&lines) {
            return Task::none();
        }
        let lemmas = known::gloss_lemmas(&self.lyrics_ui.glosses);
        let key = Self::server_song_key(&song);
        if self.learn_store().record(&key, &lines, &lemmas, listen_core::history::now_secs()) {
            self.learn_recount();
            self.learn_save();
        }
        Task::batch([self.learn_hub_batch(), self.learn_shown_words()])
    }

    /// Asks the hub how it classes the shown song's words that the word list alone calls new
    /// (conjugated forms: the hub finds their dictionary form, as Lute does). Words mode only.
    pub(crate) fn learn_shown_words(&mut self) -> Task<Message> {
        let (Some(base), Some(words), Some(lyrics)) = (self.vocab_hub(), &self.lyrics_ui.vocab, &self.lyrics)
        else {
            return Task::none();
        };
        if !self.lyrics_ui.words_mode || self.learn.shown_busy || self.learn.hub_off {
            return Task::none();
        }
        let key = self.lyrics_ui.key.clone();
        if self.learn.shown_key == key && !self.learn.shown.is_empty() {
            return Task::none();
        }
        let lines: Vec<String> = lyrics.lines.iter().map(|l| l.text.clone()).collect();
        let lemmas = known::gloss_lemmas(&self.lyrics_ui.glosses);
        let ask: Vec<String> = known::tokens(&lines, &lemmas)
            .into_iter()
            .filter(|t| known::status_of(words, &t.form, t.lemma.as_deref()).is_none())
            .map(|t| t.form)
            .collect();
        if ask.is_empty() {
            return Task::none();
        }
        self.learn.shown_busy = true;
        let http = self.http.clone();
        Task::perform(
            async move {
                use iced::futures::StreamExt;
                iced::futures::stream::iter(ask)
                    .map(|w| {
                        let (http, base) = (http.clone(), base.clone());
                        async move {
                            known::hub_coverage(&http, &base, &w, listen_lyrics::vocab::DEFAULT_KNOWN_MIN)
                                .await
                                .ok()
                                .map(|c| (w, c))
                        }
                    })
                    .buffer_unordered(HUB_BATCH)
                    .filter_map(|x| async move { x })
                    .collect::<Vec<_>>()
                    .await
            },
            move |r| Message::Learn(LearnMsg::WordsCounted(key.clone(), r)),
        )
    }

    /// The class of a shown word: the hub's when it was asked, else from the word list.
    pub fn learn_word_class(&self, words: &Words, form: &str, lemma: Option<&str>) -> Knowledge {
        if self.learn.shown_key == self.lyrics_ui.key
            && let Some(k) = self.learn.shown.get(form)
        {
            return *k;
        }
        Knowledge::from_status(known::status_of(words, form, lemma), listen_lyrics::vocab::DEFAULT_KNOWN_MIN)
    }

    /// Lets the hub count the stored songs still counted locally (a few at a time).
    fn learn_hub_batch(&mut self) -> Task<Message> {
        let (Some(base), Some(words)) = (self.vocab_hub(), self.lyrics_ui.vocab.clone()) else {
            return Task::none();
        };
        if self.learn.hub_busy || self.learn.hub_off {
            return Task::none();
        }
        let version = Store::version(&words, listen_lyrics::vocab::DEFAULT_KNOWN_MIN);
        let pending = self.learn_store().hub_pending(HUB_BATCH);
        if pending.is_empty() {
            return Task::none();
        }
        self.learn.hub_busy = true;
        let http = self.http.clone();
        Task::perform(
            async move {
                let jobs = pending.into_iter().map(|(key, text)| {
                    let (http, base) = (http.clone(), base.clone());
                    async move {
                        (
                            key,
                            known::hub_coverage(&http, &base, &text, listen_lyrics::vocab::DEFAULT_KNOWN_MIN)
                                .await,
                        )
                    }
                });
                iced::futures::future::join_all(jobs).await
            },
            move |r| Message::Learn(LearnMsg::HubCounted(version.clone(), r)),
        )
    }

    /// Coverage of the playing song's lyrics (words mode shows it): the hub's count when there
    /// is one, else the estimate from the word list.
    pub fn learn_shown_coverage(&self) -> Option<Coverage> {
        if let Some(s) = self.learn.store.as_ref().and_then(|s| s.songs.get(&self.lyrics_ui.key))
            && s.hub
            && s.lines_key == listen_lyrics::gemini::lines_key(&self.shown_lyric_lines())
        {
            return s.cov;
        }
        let words = self.lyrics_ui.vocab.as_ref()?;
        let lines = self.shown_lyric_lines();
        let lemmas = known::gloss_lemmas(&self.lyrics_ui.glosses);
        let c =
            known::coverage(words, &known::tokens(&lines, &lemmas), listen_lyrics::vocab::DEFAULT_KNOWN_MIN);
        (c.words > 0).then_some(c)
    }

    fn shown_lyric_lines(&self) -> Vec<String> {
        self.lyrics.as_ref().map(|l| l.lines.iter().map(|l| l.text.clone()).collect()).unwrap_or_default()
    }

    /// Recomputes coverage (only where the word list changed) and the mix.
    fn learn_recount(&mut self) {
        let Some(words) = self.lyrics_ui.vocab.clone() else { return };
        let changed = self.learn_store().refresh(&words, listen_lyrics::vocab::DEFAULT_KNOWN_MIN);
        if changed {
            self.learn_save();
        }
        let liked = std::mem::take(&mut self.learn.liked);
        let keys: Vec<String> = liked.iter().map(Self::server_song_key).collect();
        let picks = self.learn_store().learnable(keys.iter().map(String::as_str));
        let by_key: HashMap<&str, &Song> = keys.iter().map(String::as_str).zip(liked.iter()).collect();
        let blocked = self.auto_blocked();
        let (songs, cov): (Vec<Song>, Vec<Coverage>) = picks
            .into_iter()
            .filter_map(|(k, c)| Some(((*by_key.get(k)?).clone(), c)))
            .filter(|(s, _)| !blocked.contains(&listen_core::versions::song_key(s)))
            .unzip();
        self.learn.songs = songs;
        self.learn.cov = cov;
        self.learn.liked = liked;
    }

    /// The page opened: liked songs, word list, then the lyrics not seen yet.
    pub(super) fn learn_enter(&mut self) -> Task<Message> {
        if !self.learn_available() {
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        self.learn.loading = true;
        self.learn.error = None;
        let q = SongQuery { favorites_only: true, limit: 5000, ..Default::default() };
        Task::batch([
            self.learn_vocab(),
            Task::perform(
                async move { source.songs(&q).await.map(|p| p.items).map_err(|e| e.to_string()) },
                |r| Message::Learn(LearnMsg::Liked(r)),
            ),
        ])
    }

    /// Looks up the lyrics of the next liked songs not seen yet.
    fn learn_next_batch(&mut self) -> Task<Message> {
        if self.route != Route::Learnable || self.lyrics_ui.vocab.is_none() {
            self.learn.loading = false;
            return Task::none();
        }
        let now = listen_core::history::now_secs();
        let liked = self.learn.liked.clone();
        let store = self.learn_store();
        let batch: Vec<Song> =
            liked.into_iter().filter(|s| store.needs(&Self::server_song_key(s), now)).take(BATCH).collect();
        if batch.is_empty() {
            self.learn.loading = false;
            self.learn_recount();
            return Task::none();
        }
        let s = &self.config.settings.lyrics;
        let opts = listen_lyrics::fetch::Options {
            sources: listen_lyrics::providers::Source::parse_list(&s.sources),
            netease_translation: false,
        };
        let fetch_on = s.fetch && !self.demo;
        let http = self.http.clone();
        let sources = self.sources.clone();
        let ai_dir = self.paths.as_ref().map(|p| p.cache_dir.join("lyrics-ai"));
        let lang = if crate::i18n::language() == "de" { "de" } else { "en" };
        Task::perform(
            async move {
                let jobs = batch.into_iter().map(|song| {
                    let (http, opts, ai_dir) = (http.clone(), opts.clone(), ai_dir.clone());
                    let source = sources.get(&song.server_id).cloned();
                    async move {
                        let key = Listen::server_song_key(&song);
                        let server = match source {
                            Some(s) => s.lyrics(&song).await.ok().flatten().filter(|l| !l.lines.is_empty()),
                            None => None,
                        };
                        let lyrics = match server {
                            Some(l) => Some(l),
                            None if fetch_on && !opts.sources.is_empty() => {
                                listen_lyrics::fetch::auto(&http, &song, &opts).await
                            }
                            None => None,
                        };
                        let lines: Option<Vec<String>> =
                            lyrics.map(|l| l.lines.into_iter().map(|l| l.text).collect());
                        let mut lemmas = HashMap::new();
                        if let Some(lines) = lines.as_ref().filter(|l| listen_lyrics::gemini::has_hangul(l))
                            && let Some(dir) = &ai_dir
                            && let Some(g) =
                                listen_lyrics::gemini::SongCache::load(dir, &key).glossary(lang, lines)
                        {
                            lemmas = known::gloss_lemmas(g);
                        }
                        (key, lines, lemmas)
                    }
                });
                iced::futures::future::join_all(jobs).await
            },
            |r| Message::Learn(LearnMsg::Lyrics(r)),
        )
    }

    pub(super) fn on_learn(&mut self, msg: LearnMsg) -> Task<Message> {
        match msg {
            LearnMsg::Liked(Ok(songs)) => {
                let now = listen_core::history::now_secs();
                self.learn.liked = songs;
                let keys: Vec<String> = self.learn.liked.iter().map(Self::server_song_key).collect();
                let store = self.learn_store();
                let to_check = keys.iter().filter(|k| store.needs(k, now)).count();
                self.learn.to_check = to_check;
                self.learn.checked = 0;
                self.learn_recount();
                return self.learn_next_batch();
            }
            LearnMsg::Liked(Err(e)) => {
                self.learn.loading = false;
                self.learn.error = Some(e);
            }
            LearnMsg::Vocab(words) => {
                self.learn.vocab_loading = false;
                match words {
                    Some(w) => {
                        self.learn.vocab_at = Some(Instant::now());
                        self.lyrics_ui.vocab = Some(w);
                        self.learn.shown.clear();
                        self.learn_recount();
                        let mut tasks = vec![self.learn_hub_batch(), self.learn_shown_words()];
                        if self.route == Route::Learnable && !self.learn.liked.is_empty() {
                            tasks.push(self.learn_next_batch());
                        }
                        return Task::batch(tasks);
                    }
                    None if self.route == Route::Learnable => {
                        self.learn.loading = false;
                        self.learn.error = Some(crate::i18n::tr("The vocab-hub did not answer.").into());
                    }
                    None => {}
                }
            }
            LearnMsg::Lyrics(found) => {
                let now = listen_core::history::now_secs();
                self.learn.checked += found.len();
                for (key, lines, lemmas) in found {
                    let lines = lines.unwrap_or_default();
                    self.learn_store().record(&key, &lines, &lemmas, now);
                }
                self.learn_save();
                self.learn_recount();
                return Task::batch([self.learn_next_batch(), self.learn_hub_batch()]);
            }
            LearnMsg::Play(i) => return self.play_picked(self.learn.songs.clone(), i),
            LearnMsg::HubCounted(version, counted) => {
                self.learn.hub_busy = false;
                if counted.iter().all(|(_, r)| r.is_err()) {
                    if let Some((_, Err(e))) = counted.first() {
                        tracing::warn!("vocab-hub cannot count lyrics: {e}");
                    }
                    self.learn.hub_off = true;
                    return Task::none();
                }
                let store = self.learn_store();
                for (key, r) in counted {
                    if let Ok(c) = r {
                        store.set_hub(&key, &version, c);
                    }
                }
                self.learn_save();
                self.learn_recount();
                return self.learn_hub_batch();
            }
            LearnMsg::WordsCounted(key, counted) => {
                self.learn.shown_busy = false;
                if key == self.lyrics_ui.key {
                    self.learn.shown_key = key;
                    self.learn.shown = counted.into_iter().map(|(w, c)| (w, known::class_of(&c))).collect();
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Args;

    #[test]
    fn mix_lists_learnable_liked_songs_and_follows_lute() {
        let (mut app, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        let mut two = crate::demo::album_songs(0).into_iter();
        let (a, b) = (two.next().unwrap(), two.next().unwrap());
        let a_id = a.id.clone();
        let mut lines: Vec<String> = vec!["사랑".into(); 19];
        lines.push("별".into());
        let ka = Listen::server_song_key(&a);
        app.learn_store().record(&ka, &lines, &HashMap::new(), 0);
        app.learn_store().record(&Listen::server_song_key(&b), &["hello".to_string()], &HashMap::new(), 0);
        app.learn.liked = vec![a, b];
        app.lyrics_ui.vocab = Some(Words { version: "1".into(), terms: [("사랑".to_string(), 99)].into() });
        app.learn_recount();
        assert_eq!(app.learn.songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [a_id.as_str()]);
        assert_eq!(app.learn.cov[0].percent(), Some(95));
        app.lyrics_ui.vocab = Some(Words {
            version: "2".into(),
            terms: [("사랑".to_string(), 99), ("별".to_string(), 5)].into(),
        });
        app.learn_recount();
        assert!(app.learn.songs.is_empty(), "all words known after Lute changed");
    }

    #[test]
    fn hub_classes_override_the_word_list_for_the_shown_song_only() {
        let (mut app, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        let words = Words { version: "1".into(), terms: [("고르다".to_string(), 99)].into() };
        app.lyrics_ui.key = "s:1".into();
        assert_eq!(app.learn_word_class(&words, "골랐다는", None), Knowledge::New);
        let known = Coverage { words: 1, known: 1, ..Default::default() };
        let _ = app.on_learn(LearnMsg::WordsCounted("s:1".into(), vec![("골랐다는".into(), known)]));
        assert_eq!(app.learn_word_class(&words, "골랐다는", None), Knowledge::Known);
        let _ = app.on_learn(LearnMsg::WordsCounted("s:0".into(), vec![]));
        assert_eq!(
            app.learn_word_class(&words, "골랐다는", None),
            Knowledge::Known,
            "a late answer for another song is dropped"
        );
        app.lyrics_ui.key = "s:2".into();
        assert_eq!(app.learn_word_class(&words, "골랐다는", None), Knowledge::New);
    }
}
