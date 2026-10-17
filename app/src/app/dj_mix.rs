//! What the AI DJ knows about mixing: a preview of how each candidate would be mixed in after the set's
//! previous song, and how each played song actually came in (for "Played since your last break").

use super::Listen;
use listen_core::analysis::{Features, TrackAnalysis};
use listen_core::model::Song;
use listen_core::settings::Mixing;
use listen_dj::BreakMode;
use listen_mix::plan::{Kind, PlanCtx, TransitionPlan};
use std::collections::HashMap;
use std::sync::Arc;

/// Looks up a song's full analysis record (usable from background tasks).
pub type AnalysisLookup = Arc<dyn Fn(&Song) -> Option<Arc<TrackAnalysis>> + Send + Sync>;

/// Tempo moves smaller than this (1.5 %) aren't worth mentioning.
const AUDIBLE_TEMPO_LR: f32 = 0.015;

/// Everything the planning task needs to preview the transitions into the
/// candidates.
pub struct MixPreview {
    /// The last song queued: the new set follows it.
    from: Song,
    lookup: AnalysisLookup,
    settings: Mixing,
}

impl MixPreview {
    /// Appends "mix from now: …" to the note of every analysed candidate. Planned with a neutral context
    /// (song start, no voice break) so the phrase depends only on the two songs.
    pub fn annotate(&self, pool: &[Song], notes: &mut HashMap<String, String>) {
        // Instrumental intro length: the DJ keeps a talk-over shorter than that.
        for song in pool {
            let Some(intro) =
                (self.lookup)(song).and_then(|r| r.features().and_then(instrumental_intro_secs))
            else {
                continue;
            };
            let phrase = format!("intro {intro:.0} s");
            notes
                .entry(song.id.clone())
                .and_modify(|n| {
                    n.push_str("; ");
                    n.push_str(&phrase);
                })
                .or_insert(phrase);
        }
        let Some(out_rec) = (self.lookup)(&self.from) else { return };
        let Some(out) = out_rec.features() else { return };
        for song in pool {
            let Some(in_rec) = (self.lookup)(song) else { continue };
            let Some(inn) = in_rec.features() else { continue };
            let ctx = PlanCtx {
                settings: &self.settings,
                out_position_s: 0.0,
                voice_break: None,
                out_genres: &self.from.genres,
                in_genres: &song.genres,
            };
            let plan = listen_mix::plan::plan(out, inn, &ctx);
            let phrase = format!(
                "{} {}",
                listen_dj::prompt::MIX_PREVIEW,
                describe(&plan, Some(out), Some(inn), &self.settings)
            );
            notes
                .entry(song.id.clone())
                .and_modify(|n| {
                    n.push_str("; ");
                    n.push_str(&phrase);
                })
                .or_insert(phrase);
        }
    }
}

/// A short, plain phrase for a transition ("16-bar beat blend + bass swap,
/// tempo eases down 3 %", "fade (tempos 9 % apart)"), cheap in tokens.
pub fn describe(plan: &TransitionPlan, out: Option<&Features>, inn: Option<&Features>, s: &Mixing) -> String {
    let bpm = |f: Option<&Features>| f.and_then(|f| f.grid.as_ref()).map(|g| g.bpm).filter(|b| *b > 0.0);
    // ln(incoming / outgoing tempo), half/double time folded in.
    let gap = bpm(out).zip(bpm(inn)).map(|(o, i)| listen_mix::score::octave_match(i, o).1);
    let tempo = |p: String| match gap {
        Some(lr) if lr <= -AUDIBLE_TEMPO_LR => format!("{p}, tempo eases down {:.0} %", pct(lr)),
        Some(lr) if lr >= AUDIBLE_TEMPO_LR => format!("{p}, tempo lifts {:.0} %", pct(lr)),
        _ => p,
    };
    match plan.kind {
        Kind::BeatBlend { bars, bass_swap_bar } => tempo(format!(
            "{bars}-bar beat blend{}",
            if bass_swap_bar.is_some() { " + bass swap" } else { "" }
        )),
        Kind::ShortBlend { bars } => tempo(format!("short {bars}-bar blend")),
        Kind::FilterOut { .. } => tempo("filter-out".into()),
        Kind::Fade { .. } => match gap {
            Some(lr) if pct(lr) > s.max_tempo_pct => format!("fade (tempos {:.0} % apart)", pct(lr)),
            None => "fade (no steady beat)".into(),
            Some(_) => "fade".into(),
        },
        Kind::Gapless => "straight cut".into(),
    }
}

fn pct(lr: f32) -> f32 {
    lr.abs().exp_m1() * 100.0
}

impl Listen {
    /// The analysis records as a lookup function for tasks.
    fn analysis_lookup(&self) -> AnalysisLookup {
        match self.analysis.as_ref() {
            Some(handle) => {
                let handle = handle.clone();
                Arc::new(move |song: &Song| handle.lookup(song))
            }
            None => Arc::new(|_: &Song| None),
        }
    }

    /// Inputs for the transition previews, or `None` when the set's first song won't be mixed in (mixing
    /// off, or a spoken break plays between the songs).
    pub(super) fn dj_mix_preview(&self, mode: BreakMode, voiced: bool) -> Option<MixPreview> {
        let settings = &self.config.settings.mixing;
        if !settings.enabled || self.analysis.is_none() || (mode == BreakMode::Between && voiced) {
            return None;
        }
        let from = self.queue.iter().last()?.song.clone();
        Some(MixPreview { from, lookup: self.analysis_lookup(), settings: settings.clone() })
    }

    /// How the current song was mixed in ("16-bar beat blend + bass swap from Teardrop"), if it was.
    fn dj_mixed_in(&self, uid: u64) -> Option<String> {
        let (out_uid, in_uid) = self.mix.pair()?;
        let plan = self.mix.plan()?;
        if in_uid != uid || plan.kind == Kind::Gapless {
            return None;
        }
        let out = self.queue.iter().find(|i| i.uid == out_uid)?.song.clone();
        let inn = self.queue.iter().find(|i| i.uid == in_uid)?.song.clone();
        let lookup = self.analysis_lookup();
        let (a, b) = (lookup(&out), lookup(&inn));
        let how = describe(
            plan,
            a.as_deref().and_then(TrackAnalysis::features),
            b.as_deref().and_then(TrackAnalysis::features),
            &self.config.settings.mixing,
        );
        Some(format!("{how} from {}", out.title))
    }

    /// Records how the current song came in, once per queue item.
    pub(super) fn dj_note_entered(&mut self) {
        let Some(uid) = self.queue.current().map(|i| i.uid) else { return };
        if self.dj.entered.as_ref().is_some_and(|(u, _)| *u == uid) {
            return;
        }
        self.dj.entered = Some((uid, self.dj_mixed_in(uid)));
    }
}

/// Seconds from the start of the audio until the lead vocal comes in, when
/// the song has a beat grid and an instrumental intro of at least two bars.
pub fn instrumental_intro_secs(f: &listen_core::analysis::Features) -> Option<f32> {
    let grid = f.grid.as_ref()?;
    let bars = usize::from(f.intro_instrumental_bars);
    if bars < 2 {
        return None;
    }
    Some((grid.bar_time(bars) - f.audio_start).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::analysis::*;
    use listen_core::model::{ArtistRef, GainInfo, ServerId, SourceKind};
    use std::time::Duration;

    fn song(id: &str) -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: id.into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            title: id.into(),
            artists: vec![ArtistRef { id: None, name: "A".into() }],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::from_secs(240),
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

    /// A steady-beat record with instrumental intro/outro (as in
    /// `app/tests/autodj.rs`); `bpm == 0` means no beat grid.
    fn analysed(song: &Song, bpm: f32) -> Arc<TrackAnalysis> {
        let grid = (bpm > 0.0).then(|| {
            let period = 60.0 / bpm;
            BeatGrid {
                beats: (0..600).map(|i| 0.1 + i as f32 * period).collect(),
                downbeat_phase: 0,
                beats_per_bar: 4,
                bpm,
                confidence: 0.9,
                stability: 0.99,
            }
        });
        Arc::new(TrackAnalysis {
            version: ANALYZER_VERSION,
            analyzed_at: 1,
            machine: "test".into(),
            decoder: "test".into(),
            song_id: song.id.clone(),
            server_hint: String::new(),
            file: FileId::of(song),
            title_hint: song.title.clone(),
            outcome: Outcome::Ok(Box::new(Features {
                audio_start: 0.1,
                audio_end: song.duration.as_secs_f32(),
                grid,
                key: Some((Camelot { number: 8, minor: true }, 0.9)),
                lufs: -9.0,
                lra: 5.0,
                bars: BarCurves::default(),
                phrase_bars: 8,
                phrase_offset: 0,
                sections: vec![],
                intro_instrumental_bars: 16,
                outro_instrumental_bars: 16,
                timbre: vec![],
                mood: Mood { energy: 0.6, danceability: 0.7, brightness: 0.5 },
            })),
        })
    }

    #[test]
    fn previews_name_the_transition_per_candidate() {
        let from = song("now");
        let pool = vec![song("close"), song("slower"), song("far"), song("beatless"), song("unknown")];
        let records: HashMap<String, Arc<TrackAnalysis>> = [
            analysed(&from, 120.0),
            analysed(&pool[0], 120.0),
            analysed(&pool[1], 116.0),
            analysed(&pool[2], 132.0),
            analysed(&pool[3], 0.0),
        ]
        .into_iter()
        .map(|r| (r.song_id.clone(), r))
        .collect();
        let preview = MixPreview {
            from,
            lookup: Arc::new(move |s: &Song| records.get(&s.id).cloned()),
            settings: Mixing::default(),
        };
        let mut notes = HashMap::from([("close".to_string(), "120 bpm, 8A".to_string())]);
        preview.annotate(&pool, &mut notes);
        // The instrumental intro (32 s at 120 bpm) comes first, then the mix preview.
        assert_eq!(notes["close"], "120 bpm, 8A; intro 32 s; mix from now: 16-bar beat blend + bass swap");
        assert!(notes["slower"].contains("beat blend") && notes["slower"].contains("tempo eases down 3 %"));
        assert!(notes["far"].ends_with("mix from now: fade (tempos 10 % apart)"));
        // No beat grid: no intro length either.
        assert_eq!(notes["beatless"], "mix from now: fade (no steady beat)");
        assert!(!notes.contains_key("unknown"), "unanalysed songs get no preview");
    }

    #[test]
    fn phrases_per_kind() {
        let s = Mixing::default();
        let mut plan = TransitionPlan::gapless(200.0, "x");
        assert_eq!(describe(&plan, None, None, &s), "straight cut");
        plan.kind = Kind::ShortBlend { bars: 4 };
        assert_eq!(describe(&plan, None, None, &s), "short 4-bar blend");
        plan.kind = Kind::FilterOut { bars: 2 };
        assert_eq!(describe(&plan, None, None, &s), "filter-out");
        plan.kind = Kind::BeatBlend { bars: 8, bass_swap_bar: None };
        assert_eq!(describe(&plan, None, None, &s), "8-bar beat blend");
    }
}
