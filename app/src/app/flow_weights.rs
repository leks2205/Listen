//! Flow weights learnt from early skips (item 16, [`listen_mix::learn`]): refitted from the listening
//! history as it grows, kept in `flow-weights.json` next to it, used by the flow scorer when the setting
//! is on. The fit also goes through listen-server (`/v1/flow-weights`, with the history sync): the
//! newest fit of any device wins ([`Learnt::replaces`]), so every device mixes with the same weights.

use super::history::HistoryMsg;
use super::{Listen, Message};
use iced::Task;
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

/// POSTs `offer` (or GETs without one) and returns the fit the server holds.
async fn exchange(
    http: reqwest::Client,
    url: String,
    token: String,
    offer: Option<Learnt>,
) -> Result<Option<Learnt>, String> {
    let err = |e: reqwest::Error| e.without_url().to_string();
    let request = match &offer {
        Some(l) => http.post(url).json(l),
        None => http.get(url),
    };
    let response =
        request.timeout(super::history_sync::SYNC_TIMEOUT).bearer_auth(token).send().await.map_err(err)?;
    let response = response.error_for_status().map_err(err)?;
    response.json::<Option<Learnt>>().await.map_err(err)
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
        let fresh = self.flow_learnt.is_some() && now_secs() - self.flow_fit_at < REFIT_AFTER_S;
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
        self.flow_fit_at = l.at;
        self.flow_learnt_len = len;
        if l.replaces(self.flow_learnt.as_ref()) {
            self.flow_learnt_keep(l);
        }
    }

    /// Uses `l` from now on and writes it to `flow-weights.json`.
    fn flow_learnt_keep(&mut self, l: Learnt) {
        if let Some(dir) = self.history.path().and_then(Path::parent)
            && let Ok(text) = serde_json::to_string_pretty(&l)
            && let Err(e) = std::fs::write(dir.join(FILE_NAME), text)
        {
            tracing::warn!("saving {FILE_NAME}: {e}");
        }
        self.flow_learnt = Some(l);
    }

    /// Offers this computer's fit to listen-server (when it learnt something) or asks for the one there,
    /// with the history sync (same address and token). A server without `/v1/flow-weights` only logs.
    pub(super) fn flow_weights_sync(&mut self) -> Task<Message> {
        if !self.config.settings.history.sync
            || !self.config.settings.auto_dj.learned_weights
            || self.history.path().is_none()
        {
            return Task::none();
        }
        let Some(server) = self.listen_server() else { return Task::none() };
        let url = server.url("v1/flow-weights");
        let offer = self.flow_learnt.clone().filter(Learnt::worth_sharing);
        Task::perform(exchange(self.http.clone(), url, server.token, offer), |r| {
            Message::History(HistoryMsg::FlowWeightsSynced(r))
        })
    }

    pub(super) fn flow_weights_synced(&mut self, result: Result<Option<Learnt>, String>) {
        match result {
            Ok(Some(theirs)) if theirs.replaces(self.flow_learnt.as_ref()) => {
                tracing::info!(
                    "flow weights from another device: {} handovers, trust {:.2}, fitted at {}",
                    theirs.labels,
                    theirs.trust,
                    theirs.at
                );
                self.flow_learnt_keep(theirs);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("syncing the flow weights: {e}"),
        }
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

    #[test]
    fn a_newer_fit_from_another_device_replaces_this_computers_and_an_older_one_does_not() {
        let (mut s, _) = Listen::boot(&crate::app::Args { demo: true, ..crate::app::Args::default() });
        let mine = Learnt { at: 100, trust: 0.4, ..learn(&[], 100) };
        s.flow_learnt = Some(mine.clone());
        let mut theirs = Learnt { at: 200, trust: 0.6, ..learn(&[], 200) };
        theirs.weights.key = 2.4;
        s.flow_weights_synced(Ok(Some(theirs.clone())));
        assert_eq!(s.flow_weights().key, 2.4, "the newest fit wins");
        s.flow_weights_synced(Ok(Some(mine)));
        assert_eq!(s.flow_weights().key, 2.4, "an older one does not");
        s.flow_weights_synced(Err("404".into()));
        s.flow_weights_synced(Ok(None));
        assert_eq!(s.flow_learnt.as_ref(), Some(&theirs));
    }

    /// Against a throwaway listen-server (temp state dir): `LISTEN_TEST_SERVER=http://… LISTEN_TEST_TOKEN=…
    /// cargo test -p listen --lib flow_weights_live -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a running listen-server"]
    async fn flow_weights_live_round_trip() {
        let base = std::env::var("LISTEN_TEST_SERVER").unwrap();
        let url = format!("{}/v1/flow-weights", base.trim_end_matches('/'));
        let token = std::env::var("LISTEN_TEST_TOKEN").unwrap();
        let at = now_secs();
        let mut mine = Learnt { at, trust: 0.5, labels: 150, ..learn(&[], at) };
        mine.weights.key = 1.7;
        let held =
            exchange(reqwest::Client::new(), url.clone(), token.clone(), Some(mine.clone())).await.unwrap();
        assert_eq!(held.as_ref(), Some(&mine), "the newest fit is kept");
        let older = Learnt { at: at - 10, ..mine.clone() };
        assert_eq!(
            exchange(reqwest::Client::new(), url.clone(), token.clone(), Some(older)).await.unwrap().as_ref(),
            Some(&mine)
        );
        assert_eq!(
            exchange(reqwest::Client::new(), url, token, None).await.unwrap(),
            Some(mine),
            "a device without a fit gets it"
        );
    }
}
