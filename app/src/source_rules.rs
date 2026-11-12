//! Source-scanning checks for rules no type can enforce: each job has one shared helper, and a copy
//! of it fails here with the name of the helper to use. The list of shared helpers is "Where shared
//! helpers live" in `docs/book/src/development/contributing.md`.

use std::path::Path;

/// This file names the patterns it looks for.
const THIS_FILE: &str = "app/src/source_rules.rs";

/// Every `.rs` file under `app/` and `crates/` as (path from the workspace root with `/`, text).
fn workspace_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "target" {
                    walk(&path, root, out);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                if rel != THIS_FILE {
                    out.push((rel, std::fs::read_to_string(&path).unwrap_or_default()));
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = Vec::new();
    for dir in ["app", "crates"] {
        walk(&root.join(dir), &root, &mut out);
    }
    assert!(
        out.iter().any(|(p, _)| p == "crates/listen-core/src/model.rs"),
        "no sources under {}",
        root.display()
    );
    out
}

/// `path:line` of byte offset `at` in `src`.
fn place(path: &str, src: &str, at: usize) -> String {
    format!("{path}:{}", src[..at].matches('\n').count() + 1)
}

/// The text directly inside the bracket at `open` (nested brackets and their contents left out).
fn top_level(src: &str, open: usize) -> String {
    let mut depth = 0usize;
    let mut top = String::new();
    for c in src[open..].chars() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ if depth == 1 => top.push(c),
            _ => {}
        }
    }
    top
}

/// Songs and albums start from `Song::new` / `Album::new` with the known fields set on top. Only the
/// source mappers write every field, so a new field is a compile error exactly where a source has to
/// decide what fills it.
#[test]
fn only_source_mappers_spell_out_whole_songs_and_albums() {
    const MAPPERS: [&str; 6] = [
        "crates/listen-core/src/model.rs",
        "crates/listen-jellyfin/src/lib.rs",
        "crates/listen-local/src/lib.rs",
        "crates/listen-subsonic/src/native.rs",
        "crates/listen-subsonic/src/normalize.rs",
        "crates/listen-ytmusic/src/lib.rs",
    ];
    let mut found = Vec::new();
    for (path, src) in workspace_sources() {
        if MAPPERS.contains(&path.as_str()) {
            continue;
        }
        for ty in ["Song {", "Album {"] {
            for (at, _) in src.match_indices(ty) {
                let before = src[..at].trim_end();
                let joined = src[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == ':');
                if joined || ["struct", "impl", "enum", "for", "->"].iter().any(|k| before.ends_with(k)) {
                    continue;
                }
                let top = top_level(&src, at + ty.len() - 1);
                let names_server =
                    top.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == "server_id");
                if names_server && !top.contains("..") {
                    found.push(place(&path, &src, at));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "build these with listen_core::model::Song::new / Album::new and set the rest on top \
         (`Song {{ artists, ..Song::new(..) }}`): {found:?}"
    );
}
