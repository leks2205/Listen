//! Finding the `.milk` presets in a folder and choosing the next one
//! (Feishin's cycling: random without repeating the current one, or the next
//! in the list; all presets or only the selected ones, minus the ignored).

use listen_core::settings::MilkdropSettings;
use std::path::{Path, PathBuf};

/// One preset file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    /// The path inside the folder without `.milk` (`/` separated); what the
    /// settings store and the UI shows.
    pub name: String,
    pub path: PathBuf,
}

/// Deepest folder level searched and the most presets read (packs have 10 000+).
const MAX_DEPTH: usize = 8;
const MAX_PRESETS: usize = 200_000;

/// `<data dir>/milkdrop/presets`.
pub fn default_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("milkdrop").join("presets")
}

/// The folder in use: the setting, else the default one.
pub fn folder(setting: &str, data_dir: Option<&Path>) -> Option<PathBuf> {
    let t = setting.trim();
    if t.is_empty() { data_dir.map(default_dir) } else { Some(PathBuf::from(t)) }
}

/// Every `.milk` file under `dir`, ordered by name (case-insensitive, then exact).
pub fn scan(dir: &Path) -> Vec<Preset> {
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Preset>) {
    if depth > MAX_DEPTH || out.len() >= MAX_PRESETS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        // `metadata` follows links, so a linked pack folder works.
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            walk(root, &path, depth + 1, out);
        } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("milk"))
            && let Ok(rel) = path.strip_prefix(root)
        {
            let name = rel
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push(Preset { name, path });
        }
    }
}

/// The names the cycle picks from: all presets (or the selected ones that
/// exist, when "include all" is off and some are selected), minus the ignored.
pub fn candidates(all: &[Preset], s: &MilkdropSettings) -> Vec<String> {
    let known = |n: &String| all.iter().any(|p| &p.name == n);
    let mut list: Vec<String> = if s.include_all || s.selected.is_empty() {
        all.iter().map(|p| p.name.clone()).collect()
    } else {
        let chosen: Vec<String> = s.selected.iter().filter(|n| known(n)).cloned().collect();
        if chosen.is_empty() { all.iter().map(|p| p.name.clone()).collect() } else { chosen }
    };
    list.retain(|n| !s.ignored.contains(n));
    list
}

/// The preset after `current`. `random` returns an index below its argument;
/// without `randomize` the next in the list (wrapping; the first when
/// `current` isn't in it).
pub fn pick_next<'a>(
    list: &'a [String],
    current: Option<&str>,
    randomize: bool,
    mut random: impl FnMut(usize) -> usize,
) -> Option<&'a String> {
    match list.len() {
        0 => None,
        1 => list.first(),
        n if randomize => {
            let others: Vec<&String> = list.iter().filter(|p| Some(p.as_str()) != current).collect();
            others.get(random(others.len()).min(n - 1)).copied()
        }
        _ => {
            let next =
                current.and_then(|c| list.iter().position(|p| p == c)).map_or(0, |i| (i + 1) % list.len());
            list.get(next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("listen-milk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(l: &[Preset]) -> Vec<&str> {
        l.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn scan_is_recursive_case_insensitive_and_ordered() {
        let d = tmp("scan");
        std::fs::create_dir_all(d.join("Pack/Sub")).unwrap();
        for f in ["b.milk", "A.MILK", "Pack/z.milk", "Pack/Sub/c.milk", "readme.txt", "Pack/notes.md"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let found = scan(&d);
        assert_eq!(names(&found), ["A", "b", "Pack/Sub/c", "Pack/z"]);
        assert_eq!(found[2].path, d.join("Pack/Sub/c.milk"));
        assert!(scan(&d.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn folder_setting_or_default() {
        let data = Path::new("/data");
        assert_eq!(folder("", Some(data)), Some(PathBuf::from("/data/milkdrop/presets")));
        assert_eq!(folder("  /mnt/p ", Some(data)), Some(PathBuf::from("/mnt/p")));
        assert_eq!(folder("", None), None);
    }

    fn presets(n: &[&str]) -> Vec<Preset> {
        n.iter().map(|n| Preset { name: n.to_string(), path: PathBuf::from(format!("{n}.milk")) }).collect()
    }

    #[test]
    fn candidates_follow_feishin() {
        let all = presets(&["a", "b", "c", "d"]);
        let mut s = MilkdropSettings::default();
        assert_eq!(candidates(&all, &s), ["a", "b", "c", "d"]);
        s.ignored = vec!["b".into()];
        assert_eq!(candidates(&all, &s), ["a", "c", "d"]);
        // include all off: only the selected ones that exist
        s.include_all = false;
        s.selected = vec!["d".into(), "a".into(), "gone".into(), "b".into()];
        assert_eq!(candidates(&all, &s), ["d", "a"]);
        // nothing (valid) selected: all of them
        s.selected = vec!["gone".into()];
        assert_eq!(candidates(&all, &s), ["a", "c", "d"]);
        s.selected.clear();
        assert_eq!(candidates(&all, &s), ["a", "c", "d"]);
    }

    #[test]
    fn next_preset_sequential_wraps() {
        let l: Vec<String> = ["a", "b", "c"].map(String::from).into();
        let n = |cur| pick_next(&l, cur, false, |_| 0).map(String::as_str);
        assert_eq!(n(None), Some("a"));
        assert_eq!(n(Some("a")), Some("b"));
        assert_eq!(n(Some("c")), Some("a"));
        assert_eq!(n(Some("gone")), Some("a"));
        assert_eq!(pick_next(&[], None, false, |_| 0), None);
    }

    #[test]
    fn next_preset_random_never_repeats_the_current() {
        let l: Vec<String> = ["a", "b", "c"].map(String::from).into();
        for r in 0..2 {
            let got = pick_next(&l, Some("b"), true, |n| r % n).unwrap();
            assert_ne!(got, "b");
        }
        // a single preset is repeated
        let one = vec!["x".to_string()];
        assert_eq!(pick_next(&one, Some("x"), true, |_| 0).map(String::as_str), Some("x"));
        // a random index past the end is clamped
        assert!(pick_next(&l, None, true, |_| 99).is_some());
    }
}
