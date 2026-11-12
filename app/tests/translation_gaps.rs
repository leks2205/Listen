//! Translation coverage: which UI strings exist only in German.
//!
//! Plain file checks over `app/i18n/*.json` (no window, no GPU).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn i18n_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n")
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A language without a string shows it in English; translations are written by people, never by
/// machine. The strings that today exist in German only are listed in
/// `app/i18n/untranslated/german-only.json`, and that list only shrinks:
/// - a new UI string needs its translations in `app/i18n/listen/<code>.json` for the other UI
///   languages too (then run `tools/gen_i18n.py`), not only in `de.json`;
/// - once a listed string is translated into any other language, delete its line from the list.
///
/// `tools/gen_i18n.py --gaps` reports the gaps per language; `tools/gen_i18n.py --german-only`
/// prints the list as it should read now (use it to drop translated strings, never to add new ones).
#[test]
fn no_new_ui_string_is_translated_only_into_german() {
    let dir = i18n_dir();
    let tables: BTreeMap<&str, BTreeMap<String, String>> = listen::i18n::LANGUAGES
        .iter()
        .filter(|(code, _)| *code != "en")
        .map(|(code, _)| (*code, read(&dir.join(format!("{code}.json")))))
        .collect();
    let german_only: BTreeSet<&String> = tables["de"]
        .keys()
        .filter(|key| tables.iter().all(|(code, t)| *code == "de" || !t.contains_key(*key)))
        .collect();
    let listed: Vec<String> = read(&dir.join("untranslated").join("german-only.json"));
    let allowed: BTreeSet<&String> = listed.iter().collect();
    assert_eq!(allowed.len(), listed.len(), "app/i18n/untranslated/german-only.json lists a string twice");
    let new: Vec<&&String> = german_only.difference(&allowed).collect();
    assert!(
        new.is_empty(),
        "these UI strings exist only in German; add their translations to app/i18n/listen/<code>.json \
         for the other UI languages and run tools/gen_i18n.py: {new:#?}"
    );
    let done: Vec<&&String> = allowed.difference(&german_only).collect();
    assert!(
        done.is_empty(),
        "these strings are no longer German-only (translated or removed); delete them from \
         app/i18n/untranslated/german-only.json: {done:#?}"
    );
}
