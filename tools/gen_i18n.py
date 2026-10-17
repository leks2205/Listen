#!/usr/bin/env python3
"""Builds app/i18n/<locale>.json (English UI text -> translation) from Feishin's
locale files, for every string literal in Listen's sources that equals (ignoring
case) a Feishin English string. Listen-only strings live in app/i18n/listen/<locale>.json
and win over Feishin's.

usage: gen_i18n.py <feishin>/src/i18n/locales
"""
import json, re, sys, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
loc_dir = pathlib.Path(sys.argv[1])

def flatten(d, prefix=""):
    out = {}
    for k, v in d.items():
        key = f"{prefix}{k}"
        if isinstance(v, dict):
            out.update(flatten(v, key + "."))
        else:
            out[key] = v
    return out

REF = re.compile(r'\$t\(([\w.]+)(?:,\s*(\{.*?\}))?\)')

def resolve(flat, s, depth=0):
    if depth > 6 or not isinstance(s, str):
        return s
    def sub(m):
        key, opts = m.group(1), m.group(2)
        cnt = None
        if opts:
            mc = re.search(r'"count":\s*(\d+)', opts)
            if mc:
                cnt = int(mc.group(1))
        cands = []
        if cnt is not None:
            cands.append(key + ("_one" if cnt == 1 else "_other"))
        cands += [key, key + "_one"]
        for c in cands:
            if c in flat:
                return resolve(flat, flat[c], depth + 1)
        return m.group(0)
    return REF.sub(sub, s)

def load(code):
    flat = flatten(json.load(open(loc_dir / f"{code}.json")))
    return {k: resolve(flat, v) for k, v in flat.items()}

LIT = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
def listen_literals():
    lits = set()
    for p in list((root / "app/src").rglob("*.rs")) + list((root / "crates").rglob("*.rs")):
        if "/tests/" in str(p):
            continue
        for m in LIT.finditer(p.read_text()):
            s = m.group(1)
            try:
                s = json.loads('"' + s + '"')
            except Exception:
                continue
            if len(s) >= 2 and s == s.strip() and re.search(r"[A-Za-z]{2}", s) and "{{" not in s:
                # format strings become templates with `{}` holes
                lits.add(re.sub(r"\{[^{}]*\}", "{}", s))
    return lits

def holes(s):
    """Feishin's i18next `{{var}}` holes -> `{}`."""
    return re.sub(r"\{\{[^{}]*\}\}", "{}", s)

def norm(s):
    return re.sub(r"\s+", " ", s.strip()).lower()

def main():
    en = load("en")
    by_norm = {}
    for k, v in en.items():
        if isinstance(v, str) and "$t(" not in v:
            by_norm.setdefault(norm(holes(v)), []).append(k)
    lits = listen_literals()
    (root / "app/i18n/listen").mkdir(parents=True, exist_ok=True)
    # the UI languages (app/src/i18n.rs LANGUAGES); Feishin's other locales are not shipped
    langs = {"ar", "bn", "de", "es", "fr", "hi", "it", "ko", "nl", "pl", "pt", "ro", "ru", "uk", "ur", "zh-Hans"}
    codes = sorted(p.stem for p in loc_dir.glob("*.json") if p.stem in langs) + sorted(
        langs - {p.stem for p in loc_dir.glob("*.json")})
    matched = {}
    for s in lits:
        keys = by_norm.get(norm(s))
        for key in sorted(keys or [], key=len):
            if s.isupper() or s[0].isupper() == en[key][0].isupper():
                matched[s] = key
                break
    print(f"{len(lits)} literals, {len(matched)} matched Feishin")
    for code in codes:
        tr = load(code) if (loc_dir / f"{code}.json").exists() else {}
        out = {}
        for s, key in matched.items():
            v = tr.get(key)
            if isinstance(v, str) and v and "$t(" not in v and v != en[key]:
                v = holes(v)
                if v.count("{}") > s.count("{}"):
                    continue
                out[s] = v.upper() if s.isupper() and len(s) > 2 else v
        extra = root / "app/i18n/listen" / f"{code}.json"
        if extra.exists():
            out.update(json.load(open(extra)))
        (root / "app/i18n" / f"{code}.json").write_text(
            json.dumps(out, ensure_ascii=False, indent=0, sort_keys=True) + "\n")
    # the list of unmatched literals, for finding what needs a Listen-only translation
    json.dump(sorted(set(lits) - set(matched)), open(root / "app/i18n/.unmatched.json", "w"), indent=0, ensure_ascii=False)

main()
