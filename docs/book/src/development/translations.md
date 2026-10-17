# Translations

Listen's interface is translated into 16 languages besides English. This page explains how the translation tables work, how to improve one, and how to add a language.

## How lookup works

Listen uses a gettext-style scheme: **the English text is the key**. Code writes the English string and wraps it with `tr()` (or `trf()` for texts with values); the active table maps the English text to the translation. If the table has no entry, the English text is shown, so a missing translation never breaks the interface. English itself has no table.

| Function | Use |
|---|---|
| `tr(s)` | Look up a fixed text. |
| `tr_cow(s)` | The same for a `Cow<str>`. |
| `trf(pattern, args)` | Look up a pattern with `{}` holes and fill the holes with `args` in order. |

Because the English text is the key, changing an English string in the code also changes its key. After editing a string, update every table that has the old text, or the translations stop matching.

## Files

| Path | Content |
|---|---|
| `app/i18n/<code>.json` | The merged table for one language: a flat JSON object, English text to translation. This is what the app embeds with `include_str!`. |
| `app/i18n/listen/<code>.json` | Strings that exist only in Listen (not in Feishin). These are the files to edit by hand. |
| `tools/gen_i18n.py` | Builds the merged tables. |
| `app/src/i18n.rs` | The `LANGUAGES` list, the lookup code and the locale detection. |

The merged files are generated. Do not edit `app/i18n/<code>.json` directly; your change would be overwritten the next time the script runs. Put Listen-only strings into `app/i18n/listen/<code>.json`.

## Generating the tables

Many of Listen's labels are the same as Feishin's, so their translations are reused. `tools/gen_i18n.py` takes the folder with Feishin's locale files:

```bash
python3 tools/gen_i18n.py /path/to/feishin/src/i18n/locales
```

For each language the script:

1. Collects every string literal in the Rust sources of `app/` and `crates/` (test files excluded) that looks like UI text.
2. Matches each literal against Feishin's English strings, ignoring case and whitespace, and takes Feishin's translation for the matching key. Feishin's `{{name}}` holes become `{}`.
3. Adds the entries of `app/i18n/listen/<code>.json`. These win over Feishin's.
4. Writes `app/i18n/<code>.json`, sorted by key.

It also writes `app/i18n/.unmatched.json`, the literals that matched nothing in Feishin. That list shows what still needs a Listen-only translation.

## Finding what is missing

Run the app in a language and let it record the texts without a translation:

```bash
LISTEN_LANG=de LISTEN_I18N_COLLECT=/tmp/missing.txt cargo run --release -p listen -- --demo
```

`LISTEN_LANG` forces the language. `LISTEN_I18N_COLLECT` appends every untranslated text to the file, once each, as JSON strings. Click through the pages you want to check, then add those texts to `app/i18n/listen/<code>.json` and run `gen_i18n.py` again.

## Placeholder rules

Texts that are built from a pattern use `{}` for each value, for example `"Added {} songs"`.

- The translation must contain no more `{}` than the English text, and should contain the same number. A translation with more holes is dropped by the generator and ignored by the app.
- Holes are filled in order. A translation may leave out trailing holes, but it cannot reorder them.
- Keep the `{}` exactly as written, with no spaces inside.
- A very short pattern such as `"{} songs"` only matches when the filled-in value is a number, so it does not catch user text like "Love songs". Write short patterns with this in mind.

## Adding a language

1. Add the language to `LANGUAGES` in `app/src/i18n.rs`. The first item is the code, the second the language's own name:

   ```rust
   ("sv", "Svenska"),
   ```

2. Add the code to `english_name()` in the same file. The AI DJ uses it to name the language in its prompt.
3. Add a line to `source()` that embeds the table: `"sv" => Some(include_str!("../i18n/sv.json")),`.
4. Add the code to the `langs` set in `tools/gen_i18n.py`.
5. Create `app/i18n/listen/sv.json` with at least `{}`, then fill it with translations, and run `tools/gen_i18n.py` against Feishin's locale files. If Feishin has no locale of that code, the table is built from your Listen-only file alone.
6. Update the language count in the `every_language_has_a_table_that_parses` test in `app/src/i18n.rs`. That test also requires each table to hold more than 1000 entries, so a new language needs a substantial first translation.
7. Run `cargo test -p listen --test i18n` and `cargo test -p listen --lib i18n`, and look at the result with `LISTEN_LANG=sv cargo run --release -p listen -- --demo`.

Right-to-left languages (Arabic and Urdu are present) are drawn right to left automatically by the text layout; check the layout of the player bar and the settings pages when you add one.

The language picker in **Settings -> General -> Application -> Language** lists `LANGUAGES` plus an entry that follows the system language. The system language comes from `LC_ALL`, `LC_MESSAGES` or `LANG`; `pt_BR` and `pt_PT` map to `pt`, any `zh_*` maps to `zh-Hans`, and unknown languages fall back to English.

## License note

The files `app/i18n/<code>.json` contain translations taken from Feishin, which is licensed under GPL-3.0. They stay under GPL-3.0 and are combined with Listen under section 13 of the GPLv3 and the AGPLv3; see `NOTICE.md`. The Listen-only files in `app/i18n/listen/` are AGPL-3.0-or-later. If you contribute a translation, you agree to this split. Do not copy translations from sources with an incompatible license.
