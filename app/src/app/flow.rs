//! Song flow for the DJs: tempo, key and energy from the audio analysis
//! (Navidrome and YouTube Music songs) order the Auto-DJ's picks and the AI
//! DJ's pool.

use super::{Listen, Message};
use iced::Task;
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
            weights: self.flow_weights(),
            ..Default::default()
        }
    }
}

/// Result of ordering a list for smooth mixing.
pub struct SmoothOrder {
    /// A permutation of the input indices.
    pub order: Vec<usize>,
    /// Songs with analysis (only these move).
    pub analysed: usize,
}

impl Listen {
    /// "Order for smooth mixing": the playing order of `songs` (first kept when `keep_first`), analysed
    /// songs in flow order, unanalysed ones after them in their order. Logs the mean link cost between analysed songs before/after.
    pub(super) fn smooth_order(&self, songs: &[Song], keep_first: bool) -> SmoothOrder {
        let lookup = self.flow_lookup();
        let annotated: Vec<(Song, Option<FlowFeatures>)> =
            songs.iter().map(|s| (s.clone(), lookup(s))).collect();
        let analysed = annotated.iter().filter(|a| a.1.is_some()).count();
        let ctx = self.smooth_ctx();
        let started = std::time::Instant::now();
        let order = listen_mix::order_all(&annotated, keep_first.then_some(0), &ctx);
        let took = started.elapsed();
        let mean = |o: &[usize]| {
            let (sum, n) = listen_mix::path_cost(&annotated, o, &ctx);
            if n == 0 { 0.0 } else { sum / n as f32 }
        };
        let identity: Vec<usize> = (0..songs.len()).collect();
        tracing::info!(
            "smooth order: {} songs ({analysed} analysed), mean link cost between analysed songs {:.3} → {:.3}, {:.0} ms",
            songs.len(),
            mean(&identity),
            mean(&order),
            took.as_secs_f64() * 1000.0
        );
        SmoothOrder { order, analysed }
    }

    /// Scorer context for "Order for smooth mixing" (playlist and queue): no history, the learnt
    /// weights when the setting is on.
    pub(super) fn smooth_ctx(&self) -> listen_mix::score::FlowCtx {
        listen_mix::score::FlowCtx { weights: self.flow_weights(), ..Default::default() }
    }

    /// Reorders the not-yet-played part of the queue for smooth mixing ([`listen_mix::order_queue`]).
    pub(super) fn smooth_queue(&mut self) -> Task<Message> {
        let lookup = self.flow_lookup();
        let started = std::time::Instant::now();
        let ctx = self.smooth_ctx();
        match listen_mix::order_queue(&mut self.queue, |s| lookup(s), &ctx) {
            listen_mix::QueueOrder::Nothing => {
                self.toast_info("Nothing to reorder in the queue.".into());
                Task::none()
            }
            listen_mix::QueueOrder::TooFewAnalysed => {
                self.toast_info("Too few analysed songs in the queue to order them.".into());
                Task::none()
            }
            listen_mix::QueueOrder::Ordered { analysed } => {
                tracing::info!(
                    "smooth order: queue reordered ({analysed} analysed upcoming songs, {:.0} ms)",
                    started.elapsed().as_secs_f64() * 1000.0
                );
                self.toast_success("Queue ordered for smooth mixing.".into());
                self.refresh_next()
            }
        }
    }
}
