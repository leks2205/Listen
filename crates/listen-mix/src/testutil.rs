//! Small builders for synthetic songs and features used by the tests.

use listen_core::analysis::{
    BarCurves, BeatGrid, Camelot, Features, FlowFeatures, Mood, Section, SectionKind,
};
use listen_core::model::{ArtistRef, GainInfo, ServerId, Song, SourceKind};
use std::time::Duration;

pub fn song(title: &str, artist: &str, album: Option<&str>, track: Option<u32>) -> Song {
    Song {
        mbz: Default::default(),
        extra: Default::default(),
        id: title.into(),
        server_id: ServerId("s".into()),
        source: SourceKind::Subsonic,
        title: title.into(),
        artists: vec![ArtistRef { id: None, name: artist.into() }],
        album_artists: vec![],
        album: album.map(Into::into),
        album_id: album.map(Into::into),
        image: None,
        duration: Duration::from_secs(200),
        track_number: track,
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

/// Flow features with a sure tempo and key and no timbre.
pub fn flow(bpm: Option<f32>, key: Option<Camelot>, energy: f32) -> FlowFeatures {
    FlowFeatures {
        bpm,
        bpm_confidence: 0.9,
        key: key.map(|k| (k, 1.0)),
        lufs: -9.0,
        mood: Mood { energy, danceability: 0.7, brightness: 0.5 },
        timbre: vec![],
        mixable_in: false,
        mixable_out: false,
    }
}

/// A steady 4/4 song with 8-bar phrases: instrumental intro/outro (vocal 0, energy 120) around a sung
/// body (vocal 255, energy 200).
pub struct FeatBuilder {
    bpm: f32,
    bars: usize,
    intro: usize,
    outro: usize,
    grid: bool,
}

impl FeatBuilder {
    pub fn new(bpm: f32, bars: usize) -> Self {
        FeatBuilder { bpm, bars, intro: 8, outro: 8, grid: true }
    }

    pub fn intro(mut self, bars: usize) -> Self {
        self.intro = bars;
        self
    }

    pub fn outro(mut self, bars: usize) -> Self {
        self.outro = bars;
        self
    }

    pub fn no_grid(mut self) -> Self {
        self.grid = false;
        self
    }

    pub fn build(self) -> Features {
        let period = 60.0 / self.bpm;
        let grid = BeatGrid {
            beats: (0..=self.bars * 4).map(|i| 0.5 + i as f32 * period).collect(),
            downbeat_phase: 0,
            beats_per_bar: 4,
            bpm: self.bpm,
            confidence: 0.9,
            stability: 0.99,
        };
        let audio_end = grid.bar_time(self.bars) + 0.3;
        let instrumental = |b: usize| b < self.intro || b >= self.bars - self.outro;
        let curve = |inst: u8, body: u8| -> Vec<u8> {
            (0..self.bars).map(|b| if instrumental(b) { inst } else { body }).collect()
        };
        let mut sections = Vec::new();
        if self.intro > 0 {
            sections.push(Section { start_bar: 0, bars: self.intro as u16, kind: SectionKind::Intro });
        }
        sections.push(Section {
            start_bar: self.intro as u16,
            bars: (self.bars - self.intro - self.outro) as u16,
            kind: SectionKind::Body,
        });
        if self.outro > 0 {
            sections.push(Section {
                start_bar: (self.bars - self.outro) as u16,
                bars: self.outro as u16,
                kind: SectionKind::Outro,
            });
        }
        Features {
            audio_start: 0.3,
            audio_end,
            grid: self.grid.then_some(grid),
            key: Some((Camelot { number: 8, minor: true }, 0.9)),
            lufs: -9.0,
            lra: 5.0,
            bars: BarCurves {
                energy: curve(120, 200),
                low: curve(100, 200),
                vocal: curve(0, 255),
                centroid: curve(128, 128),
                novelty: vec![0; self.bars],
            },
            phrase_bars: 8,
            phrase_offset: 0,
            sections,
            intro_instrumental_bars: self.intro as u16,
            outro_instrumental_bars: self.outro as u16,
            timbre: vec![0.5; 4],
            mood: Mood { energy: 0.6, danceability: 0.7, brightness: 0.5 },
        }
    }
}
