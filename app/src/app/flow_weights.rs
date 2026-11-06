//! Flow weights learnt from early skips (item 16, [`listen_mix::learn`]): refitted from the listening
//! history as it grows, kept in `flow-weights.json` next to it, used by the flow scorer when the setting
//! is on. Every device fits the same synced history, so they arrive at the same weights.

use super::Listen;
use listen_core::history::now_secs;
use listen_mix::learn::{Learnt, learn, transitions};
use std::path::Path;

pub(super) const FILE_NAME: &str = "flow-weights.json";
/// Refit after this many new history entries, or when the last fit is older than this.
const REFIT_EVERY: usize = 10;
const REFIT_AFTER_S: i64 = 6 * 3600;

pub(super) fn load(dir: &Path) -> Option<Learnt> {
    let text = std::fs::read_to_string(dir.join(FILE_NAME)).ok()?;
    serde_json::from_str(&text).ok()
}

impl Listen {
    /// The scorer's weights: learnt when the setting is on and a fit exists, else the hand-set defaults.
    pub(super) fn flow_weights(&self) -> listen_mix::Weights {
        match &self.flow_learnt {
            Some(l) if self.config.settings.auto_dj.learned_weights => l.scorer(),
            _ => listen_mix::Weights::default(),
        }
    }

    /// Fits the weights again when the history grew by [`REFIT_EVERY`] entries or the fit is old
    /// (always with `force`). Needs the analysis store.
    pub(super) fn refit_flow_weights(&mut self, force: bool) {
        let Some(handle) = self.analysis.clone() else { return };
        let len = self.history.entries.len();
        let fresh = self.flow_learnt.as_ref().is_some_and(|l| now_secs() - l.at < REFIT_AFTER_S);
        if !force && fresh && len < self.flow_learnt_len + REFIT_EVERY {
            return;
        }
        let started = std::time::Instant::now();
        let data = transitions(&self.history.entries, |s| handle.flow(s));
        let l = learn(&data, now_secs());
        tracing::info!(
            "flow weights: {} handovers ({} skipped early), trust {:.2}, AUC {:?} (defaults {:?}) in {:?}",
            l.labels,
            l.skips,
            l.trust,
            l.auc_cv,
            l.auc_default,
            started.elapsed()
        );
        if let Some(dir) = self.history.path().and_then(Path::parent)
            && let Ok(text) = serde_json::to_string_pretty(&l)
            && let Err(e) = std::fs::write(dir.join(FILE_NAME), text)
        {
            tracing::warn!("saving {FILE_NAME}: {e}");
        }
        self.flow_learnt = Some(l);
        self.flow_learnt_len = len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_mix::learn::TermWeights;

    #[test]
    fn off_or_without_a_fit_the_scorer_keeps_the_defaults() {
        let (mut s, _) = Listen::boot(&crate::app::Args { demo: true, ..crate::app::Args::default() });
        s.flow_learnt = None;
        assert_eq!(s.flow_weights(), listen_mix::Weights::default());
        let mut l = learn(&[], 0);
        l.weights = TermWeights { key: 0.1, ..l.weights };
        s.flow_learnt = Some(l);
        assert!(s.config.settings.auto_dj.learned_weights, "on by default");
        assert_eq!(s.flow_weights().key, 0.1);
        s.config.settings.auto_dj.learned_weights = false;
        assert_eq!(s.flow_weights(), listen_mix::Weights::default(), "off restores today's weights exactly");
    }

    #[test]
    fn ordering_the_queue_for_smooth_mixing_uses_the_learnt_weights() {
        let (mut s, _) = Listen::boot(&crate::app::Args { demo: true, ..crate::app::Args::default() });
        let mut l = learn(&[], 0);
        l.weights = TermWeights { key: 0.1, ..l.weights };
        s.flow_learnt = Some(l);
        assert_eq!(
            s.smooth_ctx().weights.key,
            0.1,
            "queue menu and playlist action share the learnt weights"
        );
        s.config.settings.auto_dj.learned_weights = false;
        assert_eq!(s.smooth_ctx().weights, listen_mix::Weights::default());
    }
}
