//! Discs Listen has ripped (`rips.json` in the data dir): a disc that is
//! already in the drive at start-up is only auto-ripped if it isn't here,
//! and rips that need fixing (unknown to MusicBrainz) are listed.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub disc_id: String,
    /// Unix seconds.
    pub when: u64,
    pub artist: String,
    pub album: String,
    pub folder: PathBuf,
    #[serde(default)]
    pub needs_fix: bool,
    /// "12/12 verified" etc.
    #[serde(default)]
    pub accuraterip: String,
    /// Where an unknown disc can be added to MusicBrainz.
    #[serde(default)]
    pub submit_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub rips: Vec<Entry>,
}

impl History {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    pub fn contains(&self, disc_id: &str) -> bool {
        self.rips.iter().any(|e| e.disc_id == disc_id)
    }

    pub fn to_fix(&self) -> impl Iterator<Item = &Entry> {
        self.rips.iter().filter(|e| e.needs_fix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let path = std::env::temp_dir().join(format!("listen-rips-{}.json", std::process::id()));
        let mut h = History::load(&path);
        assert!(!h.contains("x"));
        h.rips.push(Entry {
            disc_id: "x".into(),
            when: 1,
            artist: "Unknown Artist".into(),
            album: "Unknown Album".into(),
            folder: "/m/a".into(),
            needs_fix: true,
            accuraterip: String::new(),
            submit_url: String::new(),
        });
        h.save(&path).unwrap();
        let back = History::load(&path);
        assert!(back.contains("x"));
        assert_eq!(back.to_fix().count(), 1);
        std::fs::remove_file(path).unwrap();
    }
}
