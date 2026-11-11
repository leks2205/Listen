//! Source checks for the i18n rules of the view code (see the note above `ui::t_text`): text that iced
//! draws itself, or that is changed before the lookup, never reaches the translation table.

use crate::i18n::rust_sources;

/// Lines of `app/src/ui` (this file excluded) for which `bad` holds, as "path:line: text".
fn offending(bad: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for (path, src) in rust_sources("ui") {
        if path.ends_with("i18n_rules.rs") {
            continue;
        }
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if bad(code) {
                out.push(format!("{path}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    out
}

/// True when `code` has `call` directly followed by a string literal that starts with a letter.
fn literal_after(code: &str, call: &str) -> bool {
    code.match_indices(call).any(|(at, _)| {
        let rest = &code[at + call.len()..];
        // URLs ("https://…") are not text.
        rest.starts_with('"')
            && rest[1..].starts_with(|c: char| c.is_alphabetic())
            && !rest[1..].starts_with("http")
    })
}

#[test]
fn iced_widgets_that_draw_their_own_text_get_translated_literals() {
    // checkbox/toggler `.label("..")`, `text_input("..", ..)` and `.placeholder("..")` render the
    // string as it is: pass `crate::i18n::tr("..")`.
    let bad = offending(|c| {
        literal_after(c, ".label(") || literal_after(c, "text_input(") || literal_after(c, ".placeholder(")
    });
    assert!(bad.is_empty(), "wrap these literals in crate::i18n::tr: {bad:#?}");
}

#[test]
fn ui_text_is_translated_before_it_is_uppercased() {
    // The key is the exact English text: "Mood" has a translation, "MOOD" does not.
    let bad = offending(|c| {
        (c.contains("label(") || c.contains("label_w("))
            && c.contains(".to_uppercase()")
            && !c.contains("tr(")
    });
    assert!(bad.is_empty(), "translate first (crate::i18n::tr(x).to_uppercase()): {bad:#?}");
}

#[test]
fn pick_list_options_show_translated_labels() {
    // iced draws a pick_list option's Display as it is; an English `label()` must go through `tr`
    // (or the option through `ui::Tr`).
    let mut bad =
        offending(|c| c.contains("write_str(self.label())") || c.contains("write_str(self.0.label())"));
    for (path, src) in rust_sources("app") {
        bad.extend(
            src.lines()
                .filter(|l| l.contains("write_str(self.label())"))
                .map(|l| format!("{path}: {}", l.trim())),
        );
    }
    assert!(bad.is_empty(), "use f.write_str(crate::i18n::tr(self.label())): {bad:#?}");
}

#[test]
fn ui_strings_use_the_ellipsis_character() {
    // One spelling per text: "Saving..." and "Saving…" are two catalog keys, and one goes missing.
    let bad = offending(|c| c.split('"').skip(1).step_by(2).any(|lit| lit.ends_with("...")));
    assert!(bad.is_empty(), "write … (U+2026), not ...: {bad:#?}");
}

#[test]
fn ui_strings_have_no_space_runs() {
    // A long literal joined without its `\` continuation keeps the indentation as runs of spaces: the
    // text shows gaps and no longer equals its catalog key.
    let bad = offending(|c| {
        c.split('"').skip(1).step_by(2).any(|lit| {
            let t = lit.trim();
            !t.is_empty() && t.contains("   ")
        })
    });
    assert!(bad.is_empty(), "string literals with runs of spaces: {bad:#?}");
}
