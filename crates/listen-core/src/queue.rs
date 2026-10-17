//! The play queue. It lives in the app; the player only ever holds the
//! current and the next track (Feishin's two-slot model).

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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    items: Vec<QueueItem>,
    /// Play order as indices into `items`; identity unless shuffled.
    order: Vec<usize>,
    /// Position in `order` of the current track.
    position: Option<usize>,
    pub repeat: Repeat,
    shuffle: bool,
    next_uid: u64,
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

    /// Writes atomically (temp file + rename).
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

    /// Number of tracks after the current one (used by Auto-DJ).
    pub fn upcoming(&self) -> usize {
        match self.position {
            Some(p) => self.order.len() - p - 1,
            None => self.order.len(),
        }
    }

    /// Replaces the queue and starts at `start` (an index into `songs`).
    pub fn set(&mut self, songs: Vec<Song>, start: usize) {
        self.items.clear();
        self.order.clear();
        self.position = None;
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
        let at = self.position.map_or(0, |p| p + 1);
        let new = self.push(songs, false);
        self.order.truncate(self.order.len() - new.len());
        self.order.splice(at..at, new);
        if self.position.is_none() && !self.order.is_empty() {
            self.position = Some(0);
        }
    }

    /// "Add to queue": after the current song and the songs queued before
    /// (the end of the manual queue), ahead of the rest of the queue.
    pub fn enqueue(&mut self, songs: Vec<Song>) {
        let at = self.manual_end();
        self.insert_queued(at, songs);
    }

    /// "Play next": right after the current song, at the top of the manual queue.
    pub fn enqueue_next(&mut self, songs: Vec<Song>) {
        let at = self.position.map_or(0, |p| p + 1);
        self.insert_queued(at, songs);
    }

    /// Songs waiting in the manual queue (after the current one).
    pub fn manual_len(&self) -> usize {
        let start = self.position.map_or(0, |p| p + 1);
        self.manual_end() - start
    }

    /// Play-order position just past the manual queue.
    fn manual_end(&self) -> usize {
        let start = self.position.map_or(0, |p| p + 1);
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
        if self.position.is_none() && !self.order.is_empty() {
            self.position = Some(0);
        }
    }

    /// Appends songs at the end of the queue.
    pub fn add_last(&mut self, songs: Vec<Song>, auto: bool) {
        self.push(songs, auto);
        if self.position.is_none() && !self.order.is_empty() {
            self.position = Some(0);
        }
    }

    /// Appends songs the AI DJ picked.
    pub fn add_ai(&mut self, songs: Vec<Song>) {
        let new = self.push(songs, true);
        for i in new {
            self.items[i].ai = true;
        }
        if self.position.is_none() && !self.order.is_empty() {
            self.position = Some(0);
        }
    }

    /// Advances to the next track (the automatic "track ended" path).
    pub fn advance(&mut self) -> Option<&QueueItem> {
        self.position = self.next_position();
        self.current()
    }

    /// User pressed "next": repeat-one does not pin the track here (like
    /// Feishin it advances, but stays on the last track instead of wrapping).
    pub fn skip_next(&mut self) -> Option<&QueueItem> {
        let p = self.position?;
        self.position = if p + 1 < self.order.len() {
            Some(p + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else if self.repeat == Repeat::One {
            Some(p)
        } else {
            None
        };
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
        self.current()
    }

    pub fn remove(&mut self, uid: u64) {
        let Some(idx) = self.items.iter().position(|it| it.uid == uid) else {
            return;
        };
        let pos = self.order.iter().position(|&i| i == idx).expect("order covers items");
        self.items.remove(idx);
        self.order.remove(pos);
        for i in &mut self.order {
            if *i > idx {
                *i -= 1;
            }
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
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        let current = self.position.map(|p| self.order[p]);
        let start = self.position.map_or(0, |p| p + 1);
        let manual: Vec<usize> = self.order[start..self.manual_end()].to_vec();
        if self.shuffle {
            self.reshuffle_keeping(current);
        } else {
            self.order = (0..self.items.len()).collect();
        }
        // The manual queue still plays next, in the order it was queued.
        self.order.retain(|i| !manual.contains(i));
        let at = current.and_then(|c| self.order.iter().position(|&i| i == c)).map_or(0, |p| p + 1);
        self.order.splice(at..at, manual);
        self.position = current.and_then(|c| self.order.iter().position(|&i| i == c));
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
        let cur = self.position.unwrap_or(0);
        let (before, after) = if self.position.is_some() { (cur, cur + 1) } else { (0, 0) };
        let manual_end = self.manual_end().max(after);
        let mut rng = rand::rng();
        self.order[..before].shuffle(&mut rng);
        self.order[after..manual_end].shuffle(&mut rng);
        self.order[manual_end..].shuffle(&mut rng);
        self.follow(current);
    }

    fn next_position(&self) -> Option<usize> {
        let p = self.position?;
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

    /// Cheap fingerprint of everything the queue panel shows (order, flags, position, favourites, ratings,
    /// durations); the app rebuilds its row cache when it changes.
    pub fn signature(&self) -> u64 {
        let mix = |h: u64, v: u64| (h ^ v).wrapping_mul(0x100_0000_01b3).rotate_left(23);
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
                    | u64::from(it.ai) << 3,
            );
            h = mix(h, u64::from(it.song.rating.unwrap_or(0)));
            h = mix(h, it.song.duration.as_millis() as u64);
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
        let start = self.position.map_or(0, |p| p + 1).min(self.order.len());
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
        let start = self.position.map_or(0, |p| p + 1);
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
        if self.position.is_none() && !self.order.is_empty() {
            self.position = Some(0);
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
            self.items.push(QueueItem { uid: self.next_uid, song, auto, ai: false, queued: false });
        }
        let new: Vec<usize> = (start..self.items.len()).collect();
        self.order.extend(&new);
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GainInfo, ServerId, SourceKind};
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
            mbz: Default::default(),
            extra: Default::default(),
            id: id.into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            title: id.into(),
            artists: vec![],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::from_secs(60),
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: GainInfo::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
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
}
