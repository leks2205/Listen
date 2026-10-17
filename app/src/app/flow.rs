//! Song flow for the DJs: tempo, key and energy from the audio analysis
//! (Navidrome and YouTube Music songs) order the Auto-DJ's picks and the AI
//! DJ's pool.

use super::Listen;
use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;
use std::sync::Arc;

/// Looks up a song's analysed flow features (usable from background tasks).
pub type FlowLookup = Arc<dyn Fn(&Song) -> Option<FlowFeatures> + Send + Sync>;

impl Listen {
    /// The analysis results as a lookup function for tasks.
    pub(super) fn flow_lookup(&self) -> FlowLookup {
        match self.analysis.as_ref() {
            Some(handle) => {
                let handle = handle.clone();
                Arc::new(move |song: &Song| handle.flow(song))
            }
            None => Arc::new(|_: &Song| None),
        }
    }

    /// Orders `candidates` to follow `current` (best flow first) and keeps
    /// `n`; without analysis of the current song, the order is unchanged.
    pub(super) fn flow_order(&self, current: &Song, candidates: Vec<Song>, n: usize) -> Vec<Song> {
        let lookup = self.flow_lookup();
        let here = lookup(current);
        if !self.config.settings.mixing.flow_select || here.is_none() || candidates.len() <= 1 {
            return candidates.into_iter().take(n).collect();
        }
        let ctx = self.flow_ctx(&lookup);
        let annotated: Vec<(Song, Option<FlowFeatures>)> =
            candidates.into_iter().map(|s| (s.clone(), lookup(&s))).collect();
        let order = listen_mix::score::order((current, here.as_ref()), &annotated, n, &ctx);
        order.into_iter().map(|i| annotated[i].0.clone()).collect()
    }

    /// What played before the current song, as context for the scorer
    /// (energy arc, artist repeats).
    pub(super) fn flow_ctx(&self, lookup: &FlowLookup) -> listen_mix::score::FlowCtx {
        let position = self.queue.current_position().unwrap_or(0);
        let before: Vec<&Song> = self.queue.iter().take(position).map(|i| &i.song).collect();
        let recent = &before[before.len().saturating_sub(5)..];
        listen_mix::score::FlowCtx {
            recent: recent.iter().filter_map(|s| lookup(s)).collect(),
            recent_artists: recent.iter().map(|s| s.artist_line().to_lowercase()).collect(),
            ..Default::default()
        }
    }
}
