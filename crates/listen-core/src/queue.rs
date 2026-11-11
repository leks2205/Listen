//! The play queue. It lives in the app; the player only ever holds the
//! current and the next track (Feishin's two-slot model).

pub use crate::history::Picker;
use crate::model::Song;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    /// Off → One → All → Off, like Feishin's repeat button
    /// (`PlayerRepeat.NONE → ONE → ALL → NONE`).
    pub fn cycle(self) -> Self {
        match self {
            Repeat::Off => Repeat::One,
            Repeat::One => Repeat::All,
            Repeat::All => Repeat::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueItem {
    /// Unique within the queue, so the same song can appear twice.
    pub uid: u64,
    pub song: Song,
    /// Added by Auto-DJ rather than by the user.
    pub auto: bool,
    /// Picked by the AI DJ (also `auto`).
    #[serde(default)]
    pub ai: bool,
    /// Added with "Play next" / "Add to queue": part of the manual queue that
    /// plays right after the current song, before the rest.
    #[serde(default)]
    pub queued: bool,
    /// A DJ bridge: only this stretch of the song plays, mixed in and out (see `listen_mix::bridge`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge: Option<BridgeCut>,
    /// Why the player picked this song ("Why this song?"); `None` for songs the user added.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::history::lenient")]
    pub why: Option<Why>,
}

/// The reason for an automatic pick: who picked it and a code rendered through i18n (never prose,
/// except the AI DJ's own note and the listener's request text). Serialised as
/// `{"picker":"auto_dj","code":"similar_to","args":{"seed":"…"}}`. A code this version does not know
/// reads as no reason (the item itself always loads).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Why {
    #[serde(default)]
    pub picker: Picker,
    #[serde(flatten)]
    pub code: WhyCode,
}

impl Why {
    pub fn new(picker: Picker, code: WhyCode) -> Self {
        Self { picker, code }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", content = "args", rename_all = "snake_case")]
pub enum WhyCode {
    /// Sounds like `seed` (a song title).
    SimilarTo { seed: String },
    /// Played a lot, not for a long time; `last_played` in Unix seconds.
    ForgottenFavourite { last_played: i64 },
    /// Never played before.
    NewToYou,
    /// Its tempo fits the tempo target (heart rate, cadence).
    MatchesTempo { bpm: f32 },
    /// Little lead vocal, steady energy (focus mode).
    FocusFriendly,
    /// A DJ bridge between the songs titled `a` and `b`.
    BridgeBetween { a: String, b: String },
    /// The AI DJ's own short note (≤ 80 characters).
    AiDj { note: String },
    /// Picked for the listener's request `text`.
    Request { text: String },
    /// A new release by `artist`.
    NewRelease { artist: String },
    /// A random song from the library (the Auto-DJ's random fill).
    Random,
}

impl Why {
    /// The reason as an English i18n pattern with `{}` placeholders and its arguments, for both apps to
    /// translate the pattern and fill it in (`now` in Unix seconds, for "not played for 5 months").
    pub fn template(&self, now: i64) -> (&'static str, Vec<String>) {
        match &self.code {
            WhyCode::SimilarTo { seed } => ("Similar to “{}”", vec![seed.clone()]),
            WhyCode::ForgottenFavourite { last_played } => {
                let months = (now - last_played).max(0) / (30 * 86_400);
                if months >= 2 {
                    ("A favourite, not played for {} months", vec![months.to_string()])
                } else {
                    ("A favourite you haven't played for a while", vec![])
                }
            }
            WhyCode::NewToYou => ("New to you", vec![]),
            WhyCode::MatchesTempo { bpm } => ("Matches your tempo: {} BPM", vec![format!("{bpm:.0}")]),
            WhyCode::FocusFriendly => ("Good for focus", vec![]),
            WhyCode::BridgeBetween { a, b } => ("Bridge between “{}” and “{}”", vec![a.clone(), b.clone()]),
            WhyCode::AiDj { note } if !note.trim().is_empty() => ("{}", vec![note.trim().to_string()]),
            WhyCode::AiDj { .. } => ("Picked by the AI DJ", vec![]),
            WhyCode::Request { text } => ("For your request “{}”", vec![text.clone()]),
            WhyCode::NewRelease { artist } => ("New release by {}", vec![artist.clone()]),
            WhyCode::Random => ("Random pick from your library", vec![]),
        }
    }

    /// [`Self::template`] filled in, untranslated (logs, MCP, tests).
    pub fn english(&self, now: i64) -> String {
        let (pattern, args) = self.template(now);
        let mut out = String::new();
        let mut rest = pattern;
        let mut args = args.iter();
        while let Some(i) = rest.find("{}") {
            out.push_str(&rest[..i]);
            out.push_str(args.next().map_or("", String::as_str));
            rest = &rest[i + 2..];
        }
        out.push_str(rest);
        out
    }
}

/// The stretch of a song played as a DJ bridge between two songs that don't mix well directly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BridgeCut {
    /// File seconds where the stretch starts (a phrase downbeat) and ends.
    pub start_s: f64,
    pub end_s: f64,
    /// The same as bars on the song's beat grid.
    pub start_bar: u32,
    pub bars: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    items: Vec<QueueItem>,
    /// Play order as indices into `items`: the order songs were added in, changed by inserts, moves
    /// and shuffle. It is the only record of where the user and the DJs put each song.
    order: Vec<usize>,
    /// Position in `order` of the current track. `None`: nothing started yet, or (`ended_after` set)
    /// playback ran past the last track.
    position: Option<usize>,
    pub repeat: Repeat,
    shuffle: bool,
    next_uid: u64,
    /// Playback ran past the end of the queue (`position` is `None`): the uid of the song that played
    /// last, or 0 once that one is removed and nothing is before it. Only songs after it in play order
    /// are still to come, so songs added later play next instead of the finished queue from the top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ended_after: Option<u64>,
    /// While shuffled: the play order from before shuffle was turned on, as uids, so turning it off
    /// restores the user's own order (empty: the order songs were added in).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unshuffled: Vec<u64>,
}

/// The queue as saved between runs, with how far the current song had played (seconds).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedQueue {
    pub queue: Queue,
    pub position_secs: f64,
}

impl SavedQueue {
    /// A missing or unreadable file gives `None` (a fresh start, never an error).
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let saved: Self = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        saved.queue.is_consistent().then_some(saved)
    }

    /// Writes atomically (temp file + rename). One writer at a time per `path`: concurrent calls share the
    /// temp file and can rename each other's half-written data (the app serialises its writes).
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `order` is a permutation of `items` and `position` points into it
    /// (guards against a damaged save file).
    fn is_consistent(&self) -> bool {
        let mut seen = vec![false; self.items.len()];
        self.order.len() == self.items.len()
            && self.order.iter().all(|&i| i < seen.len() && !std::mem::replace(&mut seen[i], true))
            && self.position.is_none_or(|p| p < self.order.len())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Items in play order.
    pub fn iter(&self) -> impl Iterator<Item = &QueueItem> {
        self.order.iter().map(|&i| &self.items[i])
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.position.map(|p| &self.items[self.order[p]])
    }

    /// Changes every song in the queue (e.g. its like status).
    pub fn for_each_song_mut(&mut self, mut f: impl FnMut(&mut Song)) {
        self.items.iter_mut().for_each(|i| f(&mut i.song));
    }

    pub fn current_mut(&mut self) -> Option<&mut QueueItem> {
        let index = self.order[self.position?];
        self.items.get_mut(index)
    }

    pub fn current_position(&self) -> Option<usize> {
        self.position
    }

    /// The track that follows the current one, honouring repeat.
    pub fn peek_next(&self) -> Option<&QueueItem> {
        self.next_position().map(|p| &self.items[self.order[p]])
    }

    /// Number of tracks still to come (used by Auto-DJ and the AI DJ): after the current one, none in a
    /// queue that ran past its end (only songs added since), all of a queue that has not started.
    pub fn upcoming(&self) -> usize {
        self.order.len().saturating_sub(self.next_start())
    }

    /// Play-order position of the first track still to come: after the current one, after the song that
    /// played last when the queue ran past its end, else 0.
    fn next_start(&self) -> usize {
        self.position.or_else(|| self.ended_pos()).map_or(0, |p| p + 1)
    }

    /// Play-order position of the song that played last, while the queue has run past its end.
    fn ended_pos(&self) -> Option<usize> {
        let uid = self.ended_after.filter(|_| self.position.is_none())?;
        self.order.iter().position(|&i| self.items[i].uid == uid)
    }

    /// Ran past the last track: nothing is current and only songs added from now on are still to come.
    fn end_after(&mut self, p: usize) {
        self.ended_after = Some(self.items[self.order[p]].uid);
        self.position = None;
    }

    /// After songs were added: a queue that never started starts at its first song. A queue that ran
    /// past its end stays stopped; its next song is the first one added (see `next_position`).
    fn start_if_fresh(&mut self) {
        if self.position.is_none() && self.ended_after.is_none() && !self.order.is_empty() {
            self.position = Some(0);
        }
    }

    /// Replaces the queue and starts at `start` (an index into `songs`).
    pub fn set(&mut self, songs: Vec<Song>, start: usize) {
        self.items.clear();
        self.order.clear();
        self.position = None;
        self.ended_after = None;
        self.unshuffled.clear();
        self.push(songs, false);
        if self.items.is_empty() {
            return;
        }
        let start = start.min(self.items.len() - 1);
        if self.shuffle {
            self.reshuffle_keeping(Some(start));
        }
        self.position = self.order.iter().position(|&i| i == start);
    }

    /// Inserts songs right after the current track.
    pub fn add_next(&mut self, songs: Vec<Song>) {
        let at = self.next_start();
        let new = self.push(songs, false);
        self.order.truncate(self.order.len() - new.len());
        self.order.splice(at..at, new);
        self.start_if_fresh();
    }

    /// "Add to queue": after the current song and the songs queued before
    /// (the end of the manual queue), ahead of the rest of the queue.
    pub fn enqueue(&mut self, songs: Vec<Song>) {
        let at = self.manual_end();
        self.insert_queued(at, songs);
    }

    /// "Play next": right after the current song, at the top of the manual queue.
    pub fn enqueue_next(&mut self, songs: Vec<Song>) {
        let at = self.next_start();
        self.insert_queued(at, songs);
    }

    /// Songs waiting in the manual queue (after the current one).
    pub fn manual_len(&self) -> usize {
        self.manual_end() - self.next_start()
    }

    /// Play-order position just past the manual queue.
    fn manual_end(&self) -> usize {
        let start = self.next_start();
        start
            + self.order[start.min(self.order.len())..].iter().take_while(|&&i| self.items[i].queued).count()
    }

    fn insert_queued(&mut self, at: usize, songs: Vec<Song>) {
        let new = self.push(songs, false);
        for &i in &new {
            self.items[i].queued = true;
        }
        self.order.truncate(self.order.len() - new.len());
        let at = at.min(self.order.len());
        self.order.splice(at..at, new);
        self.start_if_fresh();
    }

    /// Appends songs at the end of the queue.
    pub fn add_last(&mut self, songs: Vec<Song>, auto: bool) {
        self.push(songs, auto);
        self.start_if_fresh();
    }

    /// Appends songs the AI DJ picked.
    pub fn add_ai(&mut self, songs: Vec<Song>) {
        let new = self.push(songs, true);
        for i in new {
            self.items[i].ai = true;
        }
        self.start_if_fresh();
    }

    /// Appends songs a DJ picked, each with its reason (`ai` when the AI DJ picked it, also for a request). Returns their uids.
    pub fn add_picked(&mut self, picks: Vec<(Song, Why)>) -> Vec<u64> {
        let (songs, whys): (Vec<Song>, Vec<Why>) = picks.into_iter().unzip();
        let new = self.push(songs, true);
        let mut uids = Vec::with_capacity(new.len());
        for (i, why) in new.into_iter().zip(whys) {
            self.items[i].ai = matches!(why.picker, Picker::AiDj | Picker::Request);
            self.items[i].why = Some(why);
            uids.push(self.items[i].uid);
        }
        self.start_if_fresh();
        uids
    }

    /// Inserts a DJ bridge right after the current song (picked by the DJ: `auto`, and `ai` when the AI DJ's
    /// pick follows it). Returns its uid.
    pub fn insert_bridge(&mut self, song: Song, cut: BridgeCut, ai: bool) -> Option<u64> {
        let at = self.position? + 1;
        let new = self.push(vec![song], true);
        let i = *new.first()?;
        self.items[i].ai = ai;
        self.items[i].bridge = Some(cut);
        self.order.truncate(self.order.len() - new.len());
        self.order.insert(at.min(self.order.len()), i);
        Some(self.items[i].uid)
    }

    /// Swaps the song of item `uid` for another version of it (focus mode's instrumental), keeping its
    /// place and flags. False when the item is gone. Changes [`Queue::signature`]: the song's id, title and
    /// artists are part of it, so views keyed on it see the swap.
    pub fn replace_song(&mut self, uid: u64, song: Song) -> bool {
        match self.items.iter_mut().find(|i| i.uid == uid) {
            Some(item) => {
                item.song = song;
                true
            }
            None => false,
        }
    }

    /// Sets the reason of the item `uid` ("Why this song?"); false when it is not in the queue.
    pub fn set_why(&mut self, uid: u64, why: Option<Why>) -> bool {
        match self.items.iter_mut().find(|it| it.uid == uid) {
            Some(it) => {
                it.why = why;
                true
            }
            None => false,
        }
    }

    /// Advances to the next track (the automatic "track ended" path). Past the last track the queue has
    /// ended; after that it advances to the first song added since, if any.
    pub fn advance(&mut self) -> Option<&QueueItem> {
        match (self.position, self.next_position()) {
            (_, Some(next)) => {
                self.position = Some(next);
                self.ended_after = None;
            }
            (Some(p), None) => self.end_after(p),
            (None, None) => {}
        }
        self.current()
    }

    /// User pressed "next": repeat-one does not pin the track here (like
    /// Feishin it advances, but stays on the last track instead of wrapping).
    /// After the queue ran past its end it moves to the first song added since.
    pub fn skip_next(&mut self) -> Option<&QueueItem> {
        let Some(p) = self.position else {
            if let Some(next) = self.next_position() {
                self.position = Some(next);
                self.ended_after = None;
            }
            return self.current();
        };
        if p + 1 < self.order.len() {
            self.position = Some(p + 1);
        } else if self.repeat == Repeat::All {
            self.position = Some(0);
        } else if self.repeat == Repeat::One {
            self.position = Some(p);
        } else {
            self.end_after(p);
        }
        self.current()
    }

    /// User pressed "previous" (the app handles "restart if > 10 s in").
    /// On the first track only repeat-all wraps to the last one; otherwise it
    /// stays on track 0, like Feishin.
    pub fn skip_previous(&mut self) -> Option<&QueueItem> {
        let p = self.position?;
        self.position = Some(self.previous_position(p));
        self.current()
    }

    fn previous_position(&self, p: usize) -> usize {
        if p > 0 {
            p - 1
        } else if self.repeat == Repeat::All {
            self.order.len() - 1
        } else {
            0
        }
    }

    /// Whether two queue entries belong to the same album (by album id, else
    /// by album name; entries with neither never match).
    fn same_album(a: &Song, b: &Song) -> bool {
        match (&a.album_id, &b.album_id) {
            (Some(x), Some(y)) => x == y,
            (None, None) => matches!((&a.album, &b.album), (Some(x), Some(y)) if x == y),
            _ => false,
        }
    }

    fn song_at(&self, pos: usize) -> &Song {
        &self.items[self.order[pos]].song
    }

    /// Play-order index of the first track of the next album (Feishin's
    /// `mediaNext(toNextAlbum)`). On the last album: repeat-all wraps to the
    /// first track of a different album, otherwise `None` (the queue ends).
    pub fn next_album_index(&self) -> Option<usize> {
        let p = self.position?;
        let cur = self.song_at(p);
        if let Some(i) = (p + 1..self.order.len()).find(|&i| !Self::same_album(cur, self.song_at(i))) {
            return Some(i);
        }
        if self.repeat == Repeat::All {
            return Some((0..p).find(|&i| !Self::same_album(cur, self.song_at(i))).unwrap_or(0));
        }
        None
    }

    /// Play-order index of the first track of the previous album (Feishin's
    /// `mediaPrevious(toPreviousAlbum)`); the first track keeps the plain
    /// previous rules (repeat-all wraps, otherwise it stays).
    pub fn prev_album_index(&self) -> Option<usize> {
        let p = self.position?;
        if p == 0 {
            return Some(self.previous_position(0));
        }
        let cur = self.song_at(p);
        let Some(last) = (0..p).rev().find(|&i| !Self::same_album(cur, self.song_at(i))) else {
            return Some(0);
        };
        let prev = self.song_at(last);
        let mut start = last;
        while start > 0 && Self::same_album(prev, self.song_at(start - 1)) {
            start -= 1;
        }
        Some(start)
    }

    /// Alt+Next: jumps to the next album; at the end of the queue behaves
    /// like `skip_next`.
    pub fn skip_next_album(&mut self) -> Option<&QueueItem> {
        match self.next_album_index() {
            Some(i) => {
                self.position = Some(i);
                self.current()
            }
            None => self.skip_next(),
        }
    }

    /// Alt+Previous: jumps to the previous album.
    pub fn skip_previous_album(&mut self) -> Option<&QueueItem> {
        self.position = self.prev_album_index().or(self.position);
        self.current()
    }

    /// Jumps to the item with `uid`.
    pub fn jump_to(&mut self, uid: u64) -> Option<&QueueItem> {
        let p = self.order.iter().position(|&i| self.items[i].uid == uid)?;
        self.position = Some(p);
        self.ended_after = None;
        self.current()
    }

    /// Removes an item. Removing the current one makes the next item in play order current, or the
    /// previous one when it was the last; an empty queue has no current item.
    pub fn remove(&mut self, uid: u64) {
        let Some(idx) = self.items.iter().position(|it| it.uid == uid) else {
            return;
        };
        let pos = self.order.iter().position(|&i| i == idx).expect("order covers items");
        if self.ended_after == Some(uid) {
            // The end mark moves to the song before it, so the songs after it stay the ones to come.
            self.ended_after = Some(pos.checked_sub(1).map_or(0, |p| self.items[self.order[p]].uid));
        }
        self.items.remove(idx);
        self.order.remove(pos);
        for i in &mut self.order {
            if *i > idx {
                *i -= 1;
            }
        }
        if self.order.is_empty() {
            self.ended_after = None;
        }
        self.position = match self.position {
            _ if self.order.is_empty() => None,
            Some(p) if pos < p => Some(p - 1),
            Some(p) => Some(p.min(self.order.len() - 1)),
            None => None,
        };
    }

    /// Moves the item at play-order position `from` to `to`.
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.order.len() || to >= self.order.len() || from == to {
            return;
        }
        let current = self.position.map(|p| self.order[p]);
        let entry = self.order.remove(from);
        self.order.insert(to, entry);
        self.position = current.and_then(|c| self.order.iter().position(|&i| i == c));
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.position = None;
        self.ended_after = None;
        self.unshuffled.clear();
    }

    /// Shuffle on: the songs after the playing one are shuffled. Shuffle off: the order from before
    /// shuffling comes back, without the songs removed since and with the songs added since after it.
    /// Either way the manual queue still plays next, and a pending DJ bridge stays right before the song
    /// it leads into.
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        let current = self.position.map(|p| self.order[p]);
        // The playing song, or the one that played last in a queue that ran past its end.
        let anchor = self.position.or_else(|| self.ended_pos()).map(|p| self.order[p]);
        let start = self.next_start();
        let kept: Vec<usize> = self.order[start..self.kept_end(anchor, start)].to_vec();
        if self.shuffle {
            self.unshuffled = self.order.iter().map(|&i| self.items[i].uid).collect();
        }
        if self.shuffle && current.is_none() && anchor.is_some() {
            // A finished queue stays finished: only the songs added since are shuffled.
            self.order[start..].shuffle(&mut rand::rng());
        } else if self.shuffle {
            self.reshuffle_keeping(anchor);
        } else {
            self.order = self.unshuffled_order();
            self.unshuffled.clear();
        }
        // The manual queue still plays next, in the order it was queued.
        self.order.retain(|i| !kept.contains(i));
        let at = anchor.and_then(|c| self.order.iter().position(|&i| i == c)).map_or(0, |p| p + 1);
        self.order.splice(at..at, kept);
        self.position = current.and_then(|c| self.order.iter().position(|&i| i == c));
    }

    /// Play-order end of the songs that keep their place right after `anchor` (an item index) when shuffle
    /// is toggled: the manual queue, and a DJ bridge together with the song it leads into.
    fn kept_end(&self, anchor: Option<usize>, start: usize) -> usize {
        let mut leads_in = anchor.is_some_and(|a| self.items[a].bridge.is_some());
        let mut end = start;
        while let Some(&i) = self.order.get(end) {
            let it = &self.items[i];
            if !(it.queued || it.bridge.is_some() || leads_in) {
                break;
            }
            leads_in = it.bridge.is_some();
            end += 1;
        }
        end
    }

    /// The play order saved when shuffle was turned on (item indices), without the songs removed since,
    /// followed by the songs added since in their current order. Without a saved order (a queue saved by
    /// an older version): the order the songs were added in.
    fn unshuffled_order(&self) -> Vec<usize> {
        if self.unshuffled.is_empty() {
            return (0..self.items.len()).collect();
        }
        let index: std::collections::HashMap<u64, usize> =
            self.items.iter().enumerate().map(|(i, it)| (it.uid, i)).collect();
        let mut seen = vec![false; self.items.len()];
        let mut order: Vec<usize> = Vec::with_capacity(self.items.len());
        for i in self.unshuffled.iter().filter_map(|uid| index.get(uid).copied()) {
            if !std::mem::replace(&mut seen[i], true) {
                order.push(i);
            }
        }
        order.extend(self.order.iter().copied().filter(|&i| !seen[i]));
        order
    }

    /// Where each of `uids` sits in the play order, ascending.
    fn positions_of(&self, uids: &[u64]) -> Vec<usize> {
        (0..self.order.len()).filter(|&p| uids.contains(&self.items[self.order[p]].uid)).collect()
    }

    /// Takes the items at `picked` (ascending play-order positions) out of the
    /// order and returns them in order; `position` follows the current track.
    fn take_picked(&mut self, picked: &[usize]) -> Vec<usize> {
        let current = self.position.map(|p| self.order[p]);
        let taken: Vec<usize> = picked.iter().map(|&p| self.order[p]).collect();
        self.order = self.order.iter().copied().filter(|i| !taken.contains(i)).collect();
        self.position = current.and_then(|c| self.order.iter().position(|&i| i == c));
        taken
    }

    /// Restores `position` to the track `current` (an item index).
    fn follow(&mut self, current: Option<usize>) {
        self.position = current.and_then(|c| self.order.iter().position(|&i| i == c));
    }

    /// "Move to top": the items go to the front of the queue (in their play
    /// order) and leave the manual queue. The current track keeps playing.
    pub fn move_to_top(&mut self, uids: &[u64]) {
        let current = self.position.map(|p| self.order[p]);
        let picked = self.positions_of(uids);
        let taken = self.take_picked(&picked);
        for &i in &taken {
            self.items[i].queued = false;
        }
        self.order.splice(0..0, taken);
        self.follow(current);
    }

    /// "Move to bottom": the items go to the end of the queue and leave the
    /// manual queue. Auto-DJ songs keep their `auto` flag.
    pub fn move_to_bottom(&mut self, uids: &[u64]) {
        let current = self.position.map(|p| self.order[p]);
        let picked = self.positions_of(uids);
        let taken = self.take_picked(&picked);
        for &i in &taken {
            self.items[i].queued = false;
        }
        self.order.extend(taken);
        self.follow(current);
    }

    /// "Move to next": the items join the top of the manual queue, right after
    /// the current track, like "Play next". The current track itself stays.
    pub fn move_to_next(&mut self, uids: &[u64]) {
        let current = self.position.map(|p| self.order[p]);
        let picked: Vec<usize> =
            self.positions_of(uids).into_iter().filter(|&p| Some(self.order[p]) != current).collect();
        let taken = self.take_picked(&picked);
        for &i in &taken {
            self.items[i].queued = true;
        }
        let at = self.position.map_or(0, |p| p + 1);
        self.order.splice(at..at, taken);
        self.follow(current);
    }

    /// "Shuffle selected": the selected items trade places among themselves.
    /// Manual-queue items stay among manual-queue places, the rest among the
    /// rest, and the current track stays put.
    pub fn shuffle_selected(&mut self, uids: &[u64]) {
        let current = self.position.map(|p| self.order[p]);
        let picked: Vec<usize> =
            self.positions_of(uids).into_iter().filter(|&p| Some(self.order[p]) != current).collect();
        for queued in [true, false] {
            let places: Vec<usize> =
                picked.iter().copied().filter(|&p| self.items[self.order[p]].queued == queued).collect();
            let mut items: Vec<usize> = places.iter().map(|&p| self.order[p]).collect();
            items.shuffle(&mut rand::rng());
            for (p, i) in places.into_iter().zip(items) {
                self.order[p] = i;
            }
        }
        self.follow(current);
    }

    /// "Shuffle all": the songs before the current one, the manual queue and
    /// the rest are each shuffled, so the current track and the manual queue
    /// keep their places.
    pub fn shuffle_all(&mut self) {
        let current = self.position.map(|p| self.order[p]);
        // A queue that ran past its end keeps the song that played last as the dividing line.
        let anchor = self.position.or_else(|| self.ended_pos());
        let (before, after) = anchor.map_or((0, 0), |p| (p, p + 1));
        let manual_end = self.manual_end().max(after);
        let mut rng = rand::rng();
        self.order[..before].shuffle(&mut rng);
        self.order[after..manual_end].shuffle(&mut rng);
        self.order[manual_end..].shuffle(&mut rng);
        self.follow(current);
    }

    fn next_position(&self) -> Option<usize> {
        let Some(p) = self.position else {
            // A finished queue goes on with the first song added since; it never wraps around.
            let start = self.next_start();
            return (self.ended_after.is_some() && start < self.order.len()).then_some(start);
        };
        match self.repeat {
            Repeat::One => Some(p),
            _ if p + 1 < self.order.len() => Some(p + 1),
            Repeat::All => Some(0),
            Repeat::Off => None,
        }
    }

    /// Shuffles the play order and puts `first` (an item index) at the front.
    fn reshuffle_keeping(&mut self, first: Option<usize>) {
        let mut rest: Vec<usize> = (0..self.items.len()).filter(|&i| Some(i) != first).collect();
        rest.shuffle(&mut rand::rng());
        self.order = first.into_iter().chain(rest).collect();
    }

    // ---- the queue panel: change detection, rows, drag and drop ---------------

    /// Cheap fingerprint of everything the queue panel, the remote and handoff show (order, flags, position,
    /// song ids, titles and artists, favourites, ratings, durations, bridges); the app rebuilds its row cache
    /// and republishes the queue when it changes. A song swapped under the same uid changes it too.
    pub fn signature(&self) -> u64 {
        let mix = |h: u64, v: u64| (h ^ v).wrapping_mul(0x100_0000_01b3).rotate_left(23);
        // FNV-1a over the bytes, ended by a separator so "ab"+"c" and "a"+"bc" differ.
        let text = |h: u64, s: &str| {
            s.bytes()
                .chain(std::iter::once(0xff))
                .fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
        };
        let mut h = mix(0xcbf2_9ce4_8422_2325, self.items.len() as u64);
        h = mix(h, self.position.map_or(u64::MAX, |p| p as u64));
        h = mix(h, u64::from(self.shuffle));
        for &i in &self.order {
            let it = &self.items[i];
            h = mix(h, it.uid);
            h = mix(
                h,
                u64::from(it.auto)
                    | u64::from(it.queued) << 1
                    | u64::from(it.song.favorite) << 2
                    | u64::from(it.ai) << 3
                    | u64::from(it.bridge.is_some()) << 4,
            );
            h = mix(h, u64::from(it.song.rating.unwrap_or(0)));
            h = mix(h, it.song.duration.as_millis() as u64);
            h = text(h, &it.song.id);
            h = text(h, &it.song.title);
            for a in &it.song.artists {
                h = text(h, &a.name);
            }
        }
        h
    }

    /// The item at play-order position `at`.
    pub fn item_at(&self, at: usize) -> Option<&QueueItem> {
        self.order.get(at).map(|&i| &self.items[i])
    }

    /// Play-order range `start..end` of the "Up Next" block: the songs added
    /// with "Play next" / "Add to queue" that wait right after the current one.
    pub fn up_next_range(&self) -> std::ops::Range<usize> {
        let start = self.next_start().min(self.order.len());
        start..self.manual_end().max(start)
    }

    /// Takes the items with `uids` out (the playing one too if it is among them).
    pub fn remove_many(&mut self, uids: &[u64]) {
        for &uid in uids {
            self.remove(uid);
        }
    }

    /// Drag and drop: the items with `uids` (in play order) land before play-order position `at` (`len()` =
    /// the end). Dropped inside the Up Next block they join it, elsewhere they leave it. The current track
    /// keeps playing; `auto` flags stay.
    pub fn move_uids(&mut self, uids: &[u64], at: usize) {
        let picked = self.positions_of(uids);
        if picked.is_empty() {
            return;
        }
        let at = at.min(self.order.len());
        let before = picked.iter().filter(|&&p| p < at).count();
        let current = self.position.map(|p| self.order[p]);
        let taken = self.take_picked(&picked);
        let at = (at - before).min(self.order.len());
        for &i in &taken {
            self.items[i].queued = false;
        }
        self.order.splice(at..at, taken.iter().copied());
        self.follow(current);
        // Up Next is the run right after the current song: dropped into it
        // (or on its first row) the items become part of it.
        if self.joins_run(at, &taken) {
            for &i in &taken {
                self.items[i].queued = true;
            }
        }
    }

    /// Whether items just inserted at `at` (item indices `taken`, already in `order`) land inside the other
    /// items' Up Next run: on one of its rows, not after its end.
    fn joins_run(&self, at: usize, taken: &[usize]) -> bool {
        let start = self.next_start();
        if at < start {
            return false;
        }
        let mut run = 0;
        for &i in self.order[start.min(self.order.len())..].iter().filter(|i| !taken.contains(i)) {
            if !self.items[i].queued {
                break;
            }
            run += 1;
        }
        at - start < run
    }

    /// Inserts songs before the row at play-order position `at` (dropping
    /// albums, playlists, songs from elsewhere into the queue). Same Up Next
    /// rule as [`Self::move_uids`]. Returns the uids of the new items.
    pub fn insert_at(&mut self, at: usize, songs: Vec<Song>) -> Vec<u64> {
        let at = at.min(self.order.len());
        let new = self.push(songs, false);
        self.order.truncate(self.order.len() - new.len());
        self.order.splice(at..at, new.iter().copied());
        if self.position.is_none() {
            self.start_if_fresh();
        } else if let Some(p) = self.position
            && at <= p
        {
            self.position = Some(p + new.len());
        }
        if self.joins_run(at, &new) {
            for &i in &new {
                self.items[i].queued = true;
            }
        }
        new.iter().map(|&i| self.items[i].uid).collect()
    }

    /// Appends items and their order entries; returns the new order entries.
    fn push(&mut self, songs: Vec<Song>, auto: bool) -> Vec<usize> {
        let start = self.items.len();
        for song in songs {
            self.next_uid += 1;
            self.items.push(QueueItem {
                uid: self.next_uid,
                song,
                auto,
                ai: false,
                queued: false,
                bridge: None,
                why: None,
            });
        }
        let new: Vec<usize> = (start..self.items.len()).collect();
        self.order.extend(&new);
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ServerId, SourceKind};
    use std::time::Duration;

    #[test]
    fn ai_picks_are_auto_and_flagged() {
        let mut q = Queue::default();
        q.add_last(vec![song("a")], true);
        q.add_ai(vec![song("b"), song("c")]);
        let flags: Vec<_> = q.iter().map(|i| (i.auto, i.ai)).collect();
        assert_eq!(flags, [(true, false), (true, true), (true, true)]);
        // Old saved queues have no flag.
        let item: QueueItem = serde_json::from_str(
            &serde_json::to_string(q.iter().next().unwrap()).unwrap().replace("\"ai\":false,", ""),
        )
        .unwrap();
        assert!(!item.ai);
    }

    fn song(id: &str) -> Song {
        Song {
            duration: Duration::from_secs(60),
            ..Song::new(id, ServerId("s".into()), SourceKind::Subsonic, id)
        }
    }

    fn ids(q: &Queue) -> Vec<String> {
        q.iter().map(|i| i.song.id.clone()).collect()
    }

    fn cur(q: &Queue) -> Option<&str> {
        q.current().map(|i| i.song.id.as_str())
    }

    fn uid_at(q: &Queue, n: usize) -> u64 {
        q.iter().nth(n).unwrap().uid
    }

    fn abc() -> Queue {
        let mut q = Queue::new();
        q.set(vec![song("a"), song("b"), song("c")], 0);
        q
    }

    #[test]
    fn advances_and_stops_without_repeat() {
        let mut q = abc();
        assert_eq!(q.peek_next().unwrap().song.id, "b");
        q.advance();
        q.advance();
        assert_eq!(cur(&q), Some("c"));
        assert!(q.peek_next().is_none());
        assert!(q.advance().is_none());
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_pins() {
        let mut q = abc();
        q.repeat = Repeat::All;
        q.jump_to(3);
        assert_eq!(q.peek_next().unwrap().song.id, "a");
        q.repeat = Repeat::One;
        assert_eq!(q.advance().unwrap().song.id, "c");
        // An explicit skip on the last track stays (Feishin clamps), earlier ones move on.
        assert_eq!(q.skip_next().unwrap().song.id, "c");
        q.jump_to(1);
        assert_eq!(q.skip_next().unwrap().song.id, "b");
    }

    #[test]
    fn add_next_inserts_after_current() {
        let mut q = abc();
        q.advance();
        q.add_next(vec![song("x"), song("y")]);
        assert_eq!(ids(&q), ["a", "b", "x", "y", "c"]);
        assert_eq!(cur(&q), Some("b"));
        assert_eq!(q.upcoming(), 3);
    }

    #[test]
    fn manual_queue_plays_first_in_order_added() {
        let mut q = abc();
        q.enqueue(vec![song("x")]);
        q.enqueue(vec![song("y")]);
        assert_eq!(ids(&q), ["a", "x", "y", "b", "c"]);
        assert_eq!(q.manual_len(), 2);
        // Play next goes to the top of the manual queue.
        q.enqueue_next(vec![song("n")]);
        assert_eq!(ids(&q), ["a", "n", "x", "y", "b", "c"]);
        // Once x plays, a newly queued song goes after y, still before b.
        q.advance();
        q.advance();
        assert_eq!(cur(&q), Some("x"));
        q.enqueue(vec![song("z")]);
        assert_eq!(ids(&q), ["a", "n", "x", "y", "z", "b", "c"]);
        q.advance();
        q.advance();
        q.advance();
        assert_eq!(cur(&q), Some("b"));
        assert_eq!(q.manual_len(), 0);
        // Auto-DJ songs appended later stay behind the manual queue.
        q.enqueue(vec![song("m")]);
        q.add_last(vec![song("auto")], true);
        assert_eq!(ids(&q)[6..], ["m", "c", "auto"]);
    }

    #[test]
    fn adding_after_the_queue_ran_out_starts_the_new_song() {
        let mut q = abc();
        q.jump_to(3);
        assert!(q.skip_next().is_none());
        assert_eq!(q.upcoming(), 0, "a finished queue has nothing to come");
        assert!(q.peek_next().is_none());
        let uids = q.add_picked(vec![(song("d"), Why::new(Picker::AutoDj, WhyCode::Random))]);
        assert_eq!(cur(&q), None, "adding does not start playback by itself");
        assert_eq!(q.upcoming(), 1);
        assert_eq!(q.peek_next().map(|i| i.uid), uids.first().copied());
        assert_eq!(q.advance().map(|i| i.song.id.as_str()), Some("d"));
        // Next after the end: the same, and the end of a played-out queue by `advance` too.
        let mut q = abc();
        q.advance();
        q.advance();
        assert!(q.advance().is_none());
        assert_eq!(q.upcoming(), 0);
        q.enqueue(vec![song("x")]);
        q.add_last(vec![song("y")], true);
        assert_eq!(ids(&q), ["a", "b", "c", "x", "y"]);
        assert_eq!(q.skip_next().map(|i| i.song.id.as_str()), Some("x"));
        assert!(q.is_consistent());
    }

    #[test]
    fn inserting_at_the_end_after_the_queue_ran_out_starts_the_drop() {
        let mut q = abc();
        q.jump_to(3);
        q.skip_next();
        q.insert_at(q.len(), vec![song("d")]);
        assert_eq!(q.upcoming(), 1);
        assert_eq!(q.advance().map(|i| i.song.id.as_str()), Some("d"));
        // Removing the song that played last keeps the end where it was.
        let mut q = abc();
        q.jump_to(3);
        q.skip_next();
        q.remove(3);
        assert_eq!(q.upcoming(), 0);
        q.add_next(vec![song("n")]);
        assert_eq!(q.skip_next().map(|i| i.song.id.as_str()), Some("n"));
        // Shuffle on and off in between keeps the end.
        let mut q = abc();
        q.jump_to(3);
        q.skip_next();
        q.toggle_shuffle();
        assert_eq!(q.upcoming(), 0);
        q.toggle_shuffle();
        assert_eq!(q.upcoming(), 0);
        // Starting the queue again clears the end.
        q.jump_to(1);
        assert_eq!(q.upcoming(), 2);
        assert!(q.is_consistent());
    }

    #[test]
    fn enqueue_into_empty_queue_starts_it() {
        let mut q = Queue::new();
        q.enqueue(vec![song("x")]);
        assert_eq!(cur(&q), Some("x"));
    }

    #[test]
    fn shuffle_keeps_manual_queue_next() {
        let mut q = Queue::new();
        q.set((0..20).map(|i| song(&i.to_string())).collect(), 0);
        q.enqueue(vec![song("x"), song("y")]);
        q.toggle_shuffle();
        assert_eq!(ids(&q)[..3], ["0", "x", "y"]);
        q.toggle_shuffle();
        assert_eq!(ids(&q)[..3], ["0", "x", "y"]);
        assert_eq!(q.len(), 22);
    }

    #[test]
    fn remove_keeps_current_track() {
        let mut q = abc();
        q.advance(); // b
        q.remove(1); // a
        assert_eq!(cur(&q), Some("b"));
        q.remove(2); // b (current) -> c becomes current
        assert_eq!(cur(&q), Some("c"));
        q.remove(3);
        assert!(q.current().is_none());
        assert!(q.is_empty());
    }

    #[test]
    fn shuffle_keeps_current_first_and_unshuffle_restores() {
        let mut q = Queue::new();
        q.set((0..20).map(|i| song(&i.to_string())).collect(), 5);
        q.toggle_shuffle();
        assert_eq!(cur(&q), Some("5"));
        assert_eq!(q.current_position(), Some(0));
        assert_eq!(q.len(), 20);
        q.toggle_shuffle();
        assert_eq!(cur(&q), Some("5"));
        assert_eq!(ids(&q)[0], "0");
    }

    #[test]
    fn unshuffle_restores_the_order_from_before_shuffle_including_drags() {
        let mut q = abc();
        let c = uid_of(&q, "c");
        q.move_uids(&[c], 1);
        assert_eq!(ids(&q), ["a", "c", "b"]);
        q.toggle_shuffle();
        q.toggle_shuffle();
        assert_eq!(ids(&q), ["a", "c", "b"], "the drag survives a shuffle on and off");
        // Songs removed while shuffled stay removed, songs added while shuffled follow at the end.
        q.add_last(vec![song("d"), song("e")], true);
        q.toggle_shuffle();
        q.add_last(vec![song("f")], true);
        q.remove(uid_of(&q, "b"));
        q.toggle_shuffle();
        assert_eq!(ids(&q), ["a", "c", "d", "e", "f"]);
        assert_eq!(cur(&q), Some("a"));
        // The saved order survives a restart.
        q.toggle_shuffle();
        let json = serde_json::to_string(&q).unwrap();
        let mut back: Queue = serde_json::from_str(&json).unwrap();
        back.toggle_shuffle();
        assert_eq!(ids(&back), ["a", "c", "d", "e", "f"]);
        assert!(back.is_consistent());
    }

    #[test]
    fn a_pending_bridge_stays_right_after_the_current_song_across_shuffle() {
        let cut = BridgeCut { start_s: 10.0, end_s: 40.0, start_bar: 4, bars: 16 };
        for _ in 0..10 {
            let mut q = Queue::new();
            q.set((0..12).map(|i| song(&i.to_string())).collect(), 0);
            q.insert_bridge(song("x"), cut, false).unwrap();
            assert_eq!(ids(&q)[..3], ["0", "x", "1"]);
            q.toggle_shuffle();
            assert_eq!(ids(&q)[..3], ["0", "x", "1"], "shuffle on keeps the bridge before its song");
            q.toggle_shuffle();
            assert_eq!(ids(&q)[..3], ["0", "x", "1"], "shuffle off too");
            // While the bridge plays, the song it leads into stays next.
            q.advance();
            assert_eq!(cur(&q), Some("x"));
            q.toggle_shuffle();
            assert_eq!(q.peek_next().map(|i| i.song.id.as_str()), Some("1"));
            assert!(q.is_consistent());
        }
    }

    #[test]
    fn move_item_tracks_current() {
        let mut q = abc();
        q.move_item(0, 2);
        assert_eq!(ids(&q), ["b", "c", "a"]);
        assert_eq!(cur(&q), Some("a"));
    }

    #[test]
    fn repeat_cycles_like_feishin() {
        assert_eq!(Repeat::Off.cycle(), Repeat::One);
        assert_eq!(Repeat::One.cycle(), Repeat::All);
        assert_eq!(Repeat::All.cycle(), Repeat::Off);
    }

    #[test]
    fn saved_queue_roundtrips_with_position_and_shuffle() {
        let dir = std::env::temp_dir().join(format!("listen-queue-{}", std::process::id()));
        let path = dir.join("queue.json");
        assert!(SavedQueue::load(&path).is_none());

        let mut q = abc();
        q.toggle_shuffle();
        q.repeat = Repeat::All;
        q.advance();
        let want = ids(&q);
        SavedQueue { queue: q, position_secs: 83.5 }.save(&path).unwrap();

        let back = SavedQueue::load(&path).unwrap();
        assert_eq!(ids(&back.queue), want);
        assert_eq!(back.queue.current_position(), Some(1));
        assert_eq!(back.queue.repeat, Repeat::All);
        assert!(back.queue.shuffle());
        assert_eq!(back.position_secs, 83.5);

        std::fs::write(&path, b"{ not json").unwrap();
        assert!(SavedQueue::load(&path).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn damaged_queue_is_rejected() {
        let mut q = abc();
        q.order[1] = 0; // duplicate entry
        assert!(!q.is_consistent());
        let mut q = abc();
        q.position = Some(9);
        assert!(!q.is_consistent());
    }

    fn album_song(id: &str, album: Option<&str>, name: Option<&str>) -> Song {
        let mut s = song(id);
        s.album_id = album.map(Into::into);
        s.album = name.map(Into::into);
        s
    }

    /// a1 a2 | b1 b2 b3 | c1
    fn albums() -> Queue {
        let mut q = Queue::new();
        q.set(
            vec![
                album_song("a1", Some("A"), None),
                album_song("a2", Some("A"), None),
                album_song("b1", Some("B"), None),
                album_song("b2", Some("B"), None),
                album_song("b3", Some("B"), None),
                album_song("c1", Some("C"), None),
            ],
            0,
        );
        q
    }

    #[test]
    fn skip_matrix_next_and_previous() {
        for shuffle in [false, true] {
            for repeat in [Repeat::Off, Repeat::One, Repeat::All] {
                for manual in [false, true] {
                    let mut q = abc();
                    if shuffle {
                        q.toggle_shuffle();
                    }
                    if manual {
                        q.enqueue(vec![song("m")]);
                    }
                    q.repeat = repeat;
                    let order = ids(&q);
                    let n = order.len();
                    let ctx = format!("{repeat:?} shuffle={shuffle} manual={manual}");

                    // First track: previous.
                    q.jump_to(uid_at(&q, 0));
                    let first = cur(&q).unwrap().to_string();
                    let prev = q.skip_previous().map(|i| i.song.id.clone()).unwrap();
                    match repeat {
                        Repeat::All => assert_eq!(prev, order[n - 1], "{ctx}: wraps"),
                        _ => assert_eq!(prev, first, "{ctx}: stays on track 0"),
                    }
                    // Middle: previous goes back one.
                    q.jump_to(uid_at(&q, 1));
                    assert_eq!(q.skip_previous().unwrap().song.id, order[0], "{ctx}");

                    // Last track: next.
                    q.jump_to(uid_at(&q, n - 1));
                    let next = q.skip_next().map(|i| i.song.id.clone());
                    match repeat {
                        Repeat::Off => assert_eq!(next, None, "{ctx}: ends"),
                        Repeat::One => assert_eq!(next.as_deref(), Some(order[n - 1].as_str()), "{ctx}"),
                        Repeat::All => assert_eq!(next.as_deref(), Some(order[0].as_str()), "{ctx}"),
                    }
                    // Middle: next advances regardless of repeat.
                    q.jump_to(uid_at(&q, 0));
                    assert_eq!(q.skip_next().unwrap().song.id, order[1], "{ctx}");
                }
            }
        }
    }

    #[test]
    fn manual_queue_and_shuffle_untouched_by_previous() {
        let mut q = abc();
        q.enqueue(vec![song("m1"), song("m2")]);
        q.skip_previous();
        assert_eq!(cur(&q), Some("a"));
        assert_eq!(ids(&q), ["a", "m1", "m2", "b", "c"]);
        assert_eq!(q.manual_len(), 2);
        q.toggle_shuffle();
        let before = ids(&q);
        q.skip_previous();
        assert_eq!(ids(&q), before, "skipping never reorders");
        assert_eq!(q.manual_len(), 2);
    }

    #[test]
    fn next_album_jumps_to_first_track_of_next_album() {
        let mut q = albums();
        q.jump_to(2); // a2
        assert_eq!(q.next_album_index(), Some(2));
        assert_eq!(q.skip_next_album().unwrap().song.id, "b1");
        q.jump_to(4); // b2
        assert_eq!(q.skip_next_album().unwrap().song.id, "c1");
        // Last album: Off ends the queue, All wraps to the first different album.
        assert_eq!(q.next_album_index(), None);
        assert!(q.skip_next_album().is_none());
        let mut q = albums();
        q.repeat = Repeat::All;
        q.jump_to(6);
        assert_eq!(q.next_album_index(), Some(0));
    }

    #[test]
    fn previous_album_jumps_to_start_of_previous_album() {
        let mut q = albums();
        q.jump_to(4); // b2
        assert_eq!(q.prev_album_index(), Some(0));
        assert_eq!(q.skip_previous_album().unwrap().song.id, "a1");
        q.jump_to(6); // c1
        assert_eq!(q.skip_previous_album().unwrap().song.id, "b1");
        // On the first track: Off stays, All wraps.
        q.jump_to(2);
        assert_eq!(q.skip_previous_album().unwrap().song.id, "a1");
        q.repeat = Repeat::All;
        assert_eq!(q.skip_previous_album().unwrap().song.id, "c1");
    }

    #[test]
    fn album_jump_falls_back_to_album_name_and_ignores_unknown() {
        let mut q = Queue::new();
        q.set(
            vec![
                album_song("x1", None, Some("X")),
                album_song("x2", None, Some("X")),
                album_song("y1", None, Some("Y")),
                album_song("n1", None, None),
                album_song("n2", None, None),
            ],
            0,
        );
        assert_eq!(q.skip_next_album().unwrap().song.id, "y1");
        // Songs without album info are their own "album".
        assert_eq!(q.skip_next_album().unwrap().song.id, "n1");
        assert_eq!(q.skip_next_album().unwrap().song.id, "n2");
        // YouTube Music style ids (MPREb_...) work like any other id.
        let mut q = Queue::new();
        q.set(
            vec![
                album_song("v1", Some("MPREb_aaa"), Some("One")),
                album_song("v2", Some("MPREb_bbb"), Some("One")),
            ],
            0,
        );
        assert_eq!(q.skip_next_album().unwrap().song.id, "v2", "the id decides, not the name");
    }

    #[test]
    fn album_jump_keeps_manual_queue_and_auto_items() {
        let mut q = albums();
        q.enqueue(vec![album_song("m", Some("M"), None)]);
        q.add_last(vec![album_song("auto", Some("Z"), None)], true);
        let before = ids(&q);
        assert_eq!(q.manual_len(), 1);
        q.skip_next_album();
        assert_eq!(ids(&q), before, "a jump only moves the position");
        assert_eq!(cur(&q), Some("m"));
        assert!(q.iter().last().unwrap().auto);
    }

    #[test]
    fn auto_dj_append_and_upcoming_unaffected_by_repeat_order() {
        let mut q = abc();
        q.add_last(vec![song("d")], true);
        q.add_last(vec![song("e")], true);
        assert_eq!(q.upcoming(), 4);
        assert_eq!(ids(&q), ["a", "b", "c", "d", "e"]);
        q.toggle_shuffle();
        q.add_last(vec![song("f")], true);
        assert_eq!(q.iter().last().unwrap().song.id, "f", "auto songs still append at the end when shuffled");
        assert_eq!(q.upcoming(), 5);
        q.repeat = Repeat::One;
        assert_eq!(q.upcoming(), 5, "repeat does not change what Auto-DJ counts");
    }

    /// a b c d e with `x`, `y` queued after a (a x y b c d e), an Auto-DJ `z` at the end.
    fn manual_queue() -> Queue {
        let mut q = Queue::new();
        q.set(vec![song("a"), song("b"), song("c"), song("d"), song("e")], 0);
        q.enqueue(vec![song("x"), song("y")]);
        q.add_last(vec![song("z")], true);
        q
    }

    fn uid_of(q: &Queue, id: &str) -> u64 {
        q.iter().find(|i| i.song.id == id).unwrap().uid
    }

    #[test]
    fn move_to_next_joins_the_top_of_the_manual_queue() {
        let mut q = manual_queue();
        let (d, e) = (uid_of(&q, "d"), uid_of(&q, "e"));
        q.move_to_next(&[e, d]);
        assert_eq!(ids(&q), ["a", "d", "e", "x", "y", "b", "c", "z"]);
        assert_eq!(cur(&q), Some("a"));
        assert_eq!(q.manual_len(), 4, "the moved songs are part of the manual queue now");
        // Moving the current song to next does nothing to it.
        let a = uid_of(&q, "a");
        q.move_to_next(&[a]);
        assert_eq!(ids(&q), ["a", "d", "e", "x", "y", "b", "c", "z"]);
        // Moved from before the current song: the current song keeps playing.
        q.advance();
        q.advance();
        assert_eq!(cur(&q), Some("e"));
        q.move_to_next(&[a]);
        assert_eq!(ids(&q), ["d", "e", "a", "x", "y", "b", "c", "z"]);
        assert_eq!(cur(&q), Some("e"));
    }

    #[test]
    fn move_to_top_and_bottom_leave_the_manual_queue() {
        let mut q = manual_queue();
        let (x, c) = (uid_of(&q, "x"), uid_of(&q, "c"));
        q.move_to_bottom(&[x]);
        assert_eq!(ids(&q), ["a", "y", "b", "c", "d", "e", "z", "x"]);
        assert_eq!(q.manual_len(), 1);
        assert!(!q.iter().last().unwrap().queued);
        q.move_to_top(&[c]);
        assert_eq!(ids(&q), ["c", "a", "y", "b", "d", "e", "z", "x"]);
        assert_eq!(cur(&q), Some("a"), "the playing song is unchanged");
        assert_eq!(q.current_position(), Some(1));
        // Auto-DJ songs keep their flag when moved.
        let z = uid_of(&q, "z");
        q.move_to_top(&[z]);
        assert!(q.iter().next().unwrap().auto);
        assert_eq!(q.iter().filter(|i| i.auto).count(), 1);
        assert!(q.is_consistent());
    }

    #[test]
    fn shuffle_selected_only_trades_places_among_the_selection() {
        for _ in 0..20 {
            let mut q = manual_queue();
            let sel: Vec<u64> = ["x", "y", "c", "d", "a"].iter().map(|i| uid_of(&q, i)).collect();
            q.shuffle_selected(&sel);
            let got = ids(&q);
            assert_eq!(got[0], "a", "the current song stays");
            assert!(got[1..3].contains(&"x".to_string()) && got[1..3].contains(&"y".to_string()));
            assert_eq!(got[3], "b");
            assert!(got[4..6].contains(&"c".to_string()) && got[4..6].contains(&"d".to_string()));
            assert_eq!((got[6].as_str(), got[7].as_str()), ("e", "z"));
            assert_eq!(q.manual_len(), 2);
        }
    }

    #[test]
    fn shuffle_all_keeps_the_current_song_and_the_manual_queue_in_front() {
        for _ in 0..10 {
            let mut q = manual_queue();
            q.advance();
            q.advance(); // y
            q.shuffle_all();
            assert_eq!(cur(&q), Some("y"));
            let got = ids(&q);
            assert!(got[..2].contains(&"a".to_string()) && got[..2].contains(&"x".to_string()));
            assert_eq!(got[2], "y");
            assert_eq!(got.len(), 8);
            assert!(q.is_consistent());
        }
    }

    // ---- drag and drop in the queue panel --------------------------------------

    #[test]
    fn up_next_range_is_the_manual_run_after_the_current_song() {
        let mut q = manual_queue();
        assert_eq!(q.up_next_range(), 1..3);
        q.advance();
        assert_eq!(q.up_next_range(), 2..3, "x is playing: y is left");
        q.advance();
        assert!(q.up_next_range().is_empty());
        assert!(Queue::new().up_next_range().is_empty());
    }

    #[test]
    fn dragging_rows_reorders_and_keeps_the_playing_song() {
        let mut q = manual_queue();
        let (d, e) = (uid_of(&q, "d"), uid_of(&q, "e"));
        // d and e go before c (position 4).
        q.move_uids(&[d, e], 4);
        assert_eq!(ids(&q), ["a", "x", "y", "b", "d", "e", "c", "z"]);
        assert_eq!(cur(&q), Some("a"));
        // Dropped on the end.
        let b = uid_of(&q, "b");
        q.move_uids(&[b], q.len());
        assert_eq!(ids(&q), ["a", "x", "y", "d", "e", "c", "z", "b"]);
        // Moving the playing song itself: playback follows it.
        let a = uid_of(&q, "a");
        q.move_uids(&[a], 3);
        assert_eq!(ids(&q), ["x", "y", "a", "d", "e", "c", "z", "b"]);
        assert_eq!(cur(&q), Some("a"));
        assert!(q.is_consistent());
    }

    #[test]
    fn dropping_into_up_next_joins_it_and_dropping_out_leaves_it() {
        let mut q = manual_queue(); // a [x y] b c d e z
        let d = uid_of(&q, "d");
        // Between x and y.
        q.move_uids(&[d], 2);
        assert_eq!(ids(&q), ["a", "x", "d", "y", "b", "c", "e", "z"]);
        assert_eq!(q.manual_len(), 3);
        // On the first Up Next row: top of the block.
        let e = uid_of(&q, "e");
        q.move_uids(&[e], 1);
        assert_eq!(q.manual_len(), 4);
        assert_eq!(&ids(&q)[..5], ["a", "e", "x", "d", "y"]);
        // Right after the block: not part of it.
        let c = uid_of(&q, "c");
        q.move_uids(&[c], 5);
        assert_eq!(q.manual_len(), 4);
        assert_eq!(ids(&q)[5], "c");
        // A queued song dragged out of the block leaves it.
        let x = uid_of(&q, "x");
        q.move_uids(&[x], q.len());
        assert_eq!(q.manual_len(), 3);
        assert!(!q.iter().find(|i| i.song.id == "x").unwrap().queued);
        // The invariant holds: queued songs sit right after the current one.
        assert_eq!(q.up_next_range(), 1..4);
        assert!(q.is_consistent());
    }

    #[test]
    fn auto_dj_flags_survive_a_move() {
        let mut q = manual_queue();
        let z = uid_of(&q, "z");
        q.move_uids(&[z], 4);
        let item = q.iter().find(|i| i.uid == z).unwrap();
        assert!(item.auto && !item.queued);
        assert_eq!(q.upcoming(), 7);
    }

    #[test]
    fn inserting_songs_at_a_row_follows_the_same_rules() {
        let mut q = manual_queue(); // a [x y] b c d e z
        let uids = q.insert_at(2, vec![song("n1"), song("n2")]);
        assert_eq!(uids.len(), 2);
        assert_eq!(ids(&q), ["a", "x", "n1", "n2", "y", "b", "c", "d", "e", "z"]);
        assert_eq!(q.manual_len(), 4, "dropped inside Up Next");
        q.insert_at(q.len(), vec![song("end")]);
        assert_eq!(ids(&q).last().unwrap(), "end");
        // Before the playing song: it keeps playing.
        q.advance();
        assert_eq!(cur(&q), Some("x"));
        q.insert_at(0, vec![song("first")]);
        assert_eq!(cur(&q), Some("x"));
        assert_eq!(ids(&q)[0], "first");
        // An empty queue starts with the first song.
        let mut e = Queue::new();
        e.insert_at(7, vec![song("p"), song("q")]);
        assert_eq!(cur(&e), Some("p"));
        assert!(q.is_consistent() && e.is_consistent());
    }

    #[test]
    fn the_signature_changes_with_whatever_the_panel_shows() {
        let mut q = manual_queue();
        let before = q.signature();
        assert_eq!(before, q.signature());
        q.advance();
        let moved = q.signature();
        assert_ne!(before, moved);
        let d = uid_of(&q, "d");
        q.move_uids(&[d], 1);
        assert_ne!(moved, q.signature());
        let at = q.signature();
        q.for_each_song_mut(|s| s.favorite = !s.favorite);
        assert_ne!(at, q.signature());
        // Another version of the same length under the same uid (focus mode's instrumental) changes it too.
        let at = q.signature();
        let b = uid_of(&q, "b");
        let old = q.items.iter().find(|i| i.uid == b).unwrap().song.clone();
        q.replace_song(b, Song { duration: old.duration, ..song("b-inst") });
        assert_ne!(at, q.signature(), "a swapped song changes the signature");
    }

    // ---- player filters ---------------------------------------------------------

    #[test]
    fn player_filters_keep_matching_songs_out() {
        use crate::settings::{FilterField, FilterOp, PlayerFilter, QueueSettings};
        let mut loud = song("loud one");
        loud.favorite = true;
        loud.rating = Some(2);
        loud.duration = Duration::from_secs(400);
        let songs = vec![song("calm"), loud, song("other")];
        let mut s = QueueSettings::default();
        assert_eq!(s.filtered(songs.clone()).len(), 3, "no rules: nothing is filtered");
        let rule = |field, op, value: &str| PlayerFilter { field, op, value: value.into(), enabled: true };
        s.filters.push(rule(FilterField::Name, FilterOp::Contains, "LOUD"));
        assert_eq!(
            s.filtered(songs.clone()).iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["calm", "other"]
        );
        s.filters[0] = rule(FilterField::Duration, FilterOp::Gt, "300");
        assert_eq!(s.filtered(songs.clone()).len(), 2);
        s.filters[0] = rule(FilterField::Favorite, FilterOp::Is, "true");
        assert_eq!(s.filtered(songs.clone()).len(), 2);
        s.filters[0] = rule(FilterField::Rating, FilterOp::Lt, "3");
        // Unrated songs count as 0 stars, so every song matches.
        assert_eq!(s.filtered(songs.clone()).len(), 0);
        // A switched-off rule or one without a value does nothing.
        s.filters[0].enabled = false;
        assert_eq!(s.filtered(songs.clone()).len(), 3);
        // A regex rule is case-insensitive and matches anywhere; a broken one matches nothing.
        s.filters[0] = rule(FilterField::Name, FilterOp::Regex, "^(loud|quiet)\\b");
        s.filters[0].value = "^(LOUD|quiet)\\b".into();
        assert_eq!(
            s.filtered(songs.clone()).iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["calm", "other"],
            "the regex keeps its matches out, ignoring case"
        );
        assert!(s.filters[0].regex().is_some() && !s.filters[0].invalid_regex());
        s.filters[0] = rule(FilterField::Name, FilterOp::Regex, "([");
        assert!(s.filters[0].invalid_regex());
        assert_eq!(s.filtered(songs.clone()).len(), 3, "an invalid pattern matches nothing");
        s.filters[0] = rule(FilterField::Name, FilterOp::Contains, "  ");
        assert_eq!(s.filtered(songs).len(), 3);
    }

    #[test]
    fn release_year_filter_uses_the_release_date_like_the_column() {
        use crate::settings::{FilterField, FilterOp, PlayerFilter};
        let mut remaster = song("remaster");
        remaster.year = Some(1975);
        remaster.extra.release_date = Some("2011-05-01".into());
        assert_eq!(remaster.release_year(), Some(2011));
        assert_eq!(crate::model::year_of("1999-05-01"), Some(1999));
        assert_eq!(crate::model::year_of("x"), None);
        let rule = |field| PlayerFilter { field, op: FilterOp::Gt, value: "2000".into(), enabled: true };
        assert!(rule(FilterField::ReleaseYear).matches(&remaster));
        assert!(!rule(FilterField::Year).matches(&remaster));
        // Without a release date the song's own year counts.
        remaster.extra.release_date = None;
        assert!(!rule(FilterField::ReleaseYear).matches(&remaster));
    }

    #[test]
    fn number_is_compares_numerically() {
        use crate::settings::{FilterField, FilterOp, PlayerFilter};
        let mut rated = song("rated");
        rated.rating = Some(3);
        let rule = |op, value: &str| PlayerFilter {
            field: FilterField::Rating,
            op,
            value: value.into(),
            enabled: true,
        };
        assert!(rule(FilterOp::Is, "3.0").matches(&rated));
        assert!(rule(FilterOp::Is, " 3 ").matches(&rated));
        assert!(!rule(FilterOp::IsNot, "3.0").matches(&rated));
        assert!(rule(FilterOp::IsNot, "4").matches(&rated));
        // Text fields still compare as text.
        let title =
            PlayerFilter { field: FilterField::Name, op: FilterOp::Is, value: "1.0".into(), enabled: true };
        assert!(!title.matches(&song("1")));
    }

    #[test]
    fn reasons_serialise_as_codes_and_old_queues_load() {
        let mut q = Queue::default();
        q.set(vec![song("a")], 0);
        // A queue item written by 0.40: no `why`, written back unchanged.
        let old = serde_json::to_string(&q.items[0]).unwrap();
        assert!(!old.contains("why"));
        let back: QueueItem = serde_json::from_str(&old).unwrap();
        assert_eq!(back, q.items[0]);

        let uid = q.items[0].uid;
        let why = Why::new(Picker::AutoDj, WhyCode::SimilarTo { seed: "Song".into() });
        assert!(q.set_why(uid, Some(why.clone())));
        assert!(!q.set_why(uid + 99, None));
        let line = serde_json::to_string(&q.items[0]).unwrap();
        assert!(
            line.contains(r#""why":{"picker":"auto_dj","code":"similar_to","args":{"seed":"Song"}}"#),
            "{line}"
        );
        assert_eq!(serde_json::from_str::<QueueItem>(&line).unwrap().why, Some(why));

        let unit = Why::new(Picker::Rediscover, WhyCode::NewToYou);
        let j = serde_json::to_string(&unit).unwrap();
        assert_eq!(serde_json::from_str::<Why>(&j).unwrap(), unit);
        // A newer version's code or a broken reason never drops the item.
        let newer = line.replace("similar_to", "moon_phase");
        assert_eq!(serde_json::from_str::<QueueItem>(&newer).unwrap().why, None);
        let broken = line.replace(r#""seed":"Song""#, r#""seed":3"#);
        assert_eq!(serde_json::from_str::<QueueItem>(&broken).unwrap().why, None);
    }

    #[test]
    fn picked_songs_carry_their_reason() {
        let mut q = Queue::new();
        q.set(vec![song("a")], 0);
        let ai = Why::new(Picker::AiDj, WhyCode::AiDj { note: "Same key, a bit faster".into() });
        let auto = Why::new(Picker::AutoDj, WhyCode::Random);
        let uids = q.add_picked(vec![(song("b"), ai.clone()), (song("c"), auto.clone())]);
        assert_eq!(uids.len(), 2);
        let items: Vec<&QueueItem> = q.iter().collect();
        assert!(items[0].why.is_none() && !items[0].auto);
        assert!(items[1].auto && items[1].ai && items[1].why == Some(ai));
        assert!(items[2].auto && !items[2].ai && items[2].why == Some(auto));
        assert_eq!(items[1].uid, uids[0]);
    }

    #[test]
    fn every_reason_reads_as_a_sentence() {
        let now = 1_800_000_000;
        let w = |c| Why::new(Picker::AutoDj, c);
        assert_eq!(w(WhyCode::SimilarTo { seed: "Roads".into() }).english(now), "Similar to “Roads”");
        let old = now - 150 * 86_400;
        assert_eq!(
            w(WhyCode::ForgottenFavourite { last_played: old }).english(now),
            "A favourite, not played for 5 months"
        );
        assert_eq!(w(WhyCode::AiDj { note: " Warm up ".into() }).english(now), "Warm up");
        assert_eq!(w(WhyCode::AiDj { note: "".into() }).english(now), "Picked by the AI DJ");
        assert_eq!(
            w(WhyCode::BridgeBetween { a: "A".into(), b: "B".into() }).english(now),
            "Bridge between “A” and “B”"
        );
        assert_eq!(w(WhyCode::MatchesTempo { bpm: 123.6 }).english(now), "Matches your tempo: 124 BPM");
        let j = serde_json::to_string(&w(WhyCode::Random)).unwrap();
        assert_eq!(j, r#"{"picker":"auto_dj","code":"random"}"#);
        assert_eq!(serde_json::from_str::<Why>(&j).unwrap().code, WhyCode::Random);
    }
}
