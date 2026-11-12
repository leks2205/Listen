#!/usr/bin/env python3
"""listen-embed: embeddings for "Describe it" search (natural-language library search).

Runs on a PC with a GPU. For every song of the analysis store that has no line yet, it writes one line to
`<out>/v1/clamp3/<machine>.jsonl` (format: `listen_core::describe::EmbedRecord`):

* `audio`: CLaMP 3 (SAAS) embedding of a 30-s clip from 35 % into the song (MERT-v1-95M features),
* `tags_en` / `tags_ko`: search tags Gemini writes from artist and title (genre, decade, country, singer,
  instruments, mood, scene labels such as "1st-generation K-pop" or "city pop"),
* `tags`: CLaMP 3 text embedding of those tags.

Vectors are little-endian float16, base64. listen-server encodes queries with CLaMP 3's text tower, which
`export-text` writes to `<out>/v1/clamp3/text/` (safetensors + tokenizer.json + config.json + check.json).

Audio: `ytm:` songs through yt-dlp, Navidrome songs through the Subsonic API (`LISTEN_ND_URL`,
`LISTEN_ND_USER`, `LISTEN_ND_PASS`). Gemini key: `GEMINI_API_KEY` (or `LISTEN_GEMINI_API_KEY`).
CLaMP 3: a checkout of https://github.com/sanderwood/clamp3 (`--clamp3`), see README.md.

    python listen_embed.py run --limit 500
    python listen_embed.py export-text
    python listen_embed.py status
"""

import argparse
import base64
import glob
import hashlib
import json
import os
import random
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request

HOME = os.path.expanduser("~")


def listen_folder():
    """The synced `Listen` folder, found like the desktop app finds the analysis store: the first Nextcloud
    sync folder from nextcloud.cfg, else ~/Nextcloud."""
    try:
        with open(os.path.join(HOME, ".config/Nextcloud/nextcloud.cfg"), encoding="utf-8") as f:
            for line in f:
                key, _, value = line.partition("=")
                if key.strip().endswith("localPath") and value.strip():
                    return os.path.join(value.strip(), "Listen")
    except OSError:
        pass
    return os.path.join(HOME, "Nextcloud", "Listen")


DEFAULT_STORE = os.environ.get("LISTEN_ANALYSIS_DIR") or os.path.join(listen_folder(), "analysis", "v1")
DEFAULT_OUT = os.environ.get("LISTEN_EMBED_DIR") or os.path.join(listen_folder(), "embeddings")
DEFAULT_HISTORY = os.path.join(HOME, ".local/share/listen/history.jsonl")
MODEL = "clamp3"
CLIP_S = 30
CLIP_AT = 0.35
GEMINI = "https://generativelanguage.googleapis.com/v1beta/interactions"
GEMINI_MODEL = os.environ.get("LISTEN_GEMINI_MODEL", "gemini-3.5-flash-lite")
TAG_BATCH = 40

TAG_SYSTEM = (
    "You tag songs for a music search engine. For each numbered song (artist and title) give what is known "
    "about the recording: genre and style, decade of release, country and language, who sings (male solo, "
    "female solo, boy group, girl group, band, duet, instrumental), notable instruments, mood, tempo feel, and "
    "labels fans use (for example 1st-generation K-pop, city pop, trot, bossa nova, Motown, live recording). "
    "Write 'en': one line of comma-separated English tags, and 'ko': the same tags in Korean. "
    "If you do not know the song, infer from the artist; never invent a different song."
)
TAG_SCHEMA = {
    "type": "array",
    "items": {
        "type": "object",
        "properties": {"n": {"type": "integer"}, "en": {"type": "string"}, "ko": {"type": "string"}},
        "required": ["n", "en", "ko"],
    },
}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


# ---- the analysis store and the embeddings store ------------------------------------------------------


def store_songs(store_dir):
    """song_id -> {title_hint, duration_ms, server_hint} for every song with an Ok analysis record."""
    songs = {}
    for path in sorted(glob.glob(os.path.join(store_dir, "*", "*.jsonl"))):
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(r.get("outcome"), dict) or "Ok" not in r["outcome"]:
                    continue
                songs[r["song_id"]] = {
                    "title_hint": r.get("title_hint", ""),
                    "duration_ms": (r.get("file") or {}).get("duration_ms", 0),
                    "server_hint": r.get("server_hint", ""),
                }
    return songs


def out_dir(out):
    return os.path.join(out, "v1", MODEL)


def done_records(out):
    """song_id -> record of every line written so far (all machines)."""
    done = {}
    for path in sorted(glob.glob(os.path.join(out_dir(out), "*.jsonl"))):
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if r.get("song_id"):
                    done[r["song_id"]] = r
    return done


def end_torn_line(path):
    """Ends a last line a killed run left without its newline, so the next record starts on its own line."""
    if not os.path.exists(path) or os.path.getsize(path) == 0:
        return
    with open(path, "rb+") as f:
        f.seek(-1, os.SEEK_END)
        if f.read(1) != b"\n":
            f.write(b"\n")


def play_counts(history):
    """song id (store form) -> plays, to embed the songs that are played first."""
    counts = {}
    if not os.path.exists(history):
        return counts
    with open(history, encoding="utf-8", errors="replace") as f:
        for line in f:
            try:
                e = json.loads(line)
            except ValueError:
                continue
            sid = e.get("song_id") or ""
            if str(e.get("source", "")).lower() == "youtubemusic" or str(e.get("server_id", "")).startswith("ytm"):
                sid = "ytm:" + sid
            counts[sid] = counts.get(sid, 0) + 1
    return counts


def vec_b64(v):
    import numpy as np

    v = np.asarray(v, dtype=np.float32).reshape(-1)
    return base64.b64encode(v.astype("<f2").tobytes()).decode()


# ---- Gemini tags -------------------------------------------------------------------------------------


def gemini_key():
    return os.environ.get("GEMINI_API_KEY") or os.environ.get("LISTEN_GEMINI_API_KEY") or ""


def split_hint(hint):
    for sep in (" – ", " - "):
        if sep in hint:
            a, t = hint.split(sep, 1)
            return a.strip(), t.strip()
    return "", hint.strip()


def tag_batch(hints):
    """Tags for a list of title hints: [(en, ko) or None]."""
    lines = []
    for k, h in enumerate(hints):
        artist, title = split_hint(h)
        lines.append(f"{k + 1}. {artist} — {title}" if artist else f"{k + 1}. {title}")
    body = {
        "model": GEMINI_MODEL,
        "system_instruction": TAG_SYSTEM,
        "input": "\n".join(lines),
        "response_format": {"type": "text", "mime_type": "application/json", "schema": TAG_SCHEMA},
        "generation_config": {"thinking_level": "minimal"},
        "store": False,
    }
    req = urllib.request.Request(
        GEMINI,
        data=json.dumps(body).encode(),
        headers={"x-goog-api-key": gemini_key(), "content-type": "application/json"},
    )
    reply = json.load(urllib.request.urlopen(req, timeout=180))
    text = "".join(
        c.get("text", "")
        for s in reply.get("steps", [])
        if s.get("type", "model_output") == "model_output"
        for c in s.get("content", [])
        if c.get("type") == "text"
    )
    out = [None] * len(hints)
    for e in json.loads(text):
        n = e.get("n", 0)
        if 1 <= n <= len(hints):
            out[n - 1] = (e.get("en", "").strip(), e.get("ko", "").strip())
    return out


# ---- audio clips -------------------------------------------------------------------------------------


def clip_ytm(vid, start, wav, tmp):
    base = os.path.join(tmp, "dl")
    for f in glob.glob(base + ".*"):
        os.remove(f)
    subprocess.run(
        [
            "yt-dlp", "-q", "--no-warnings", "-f", "251/140/bestaudio",
            "--download-sections", f"*{start}-{start + CLIP_S}",
            "-o", base + ".%(ext)s", f"https://music.youtube.com/watch?v={vid}",
        ],
        check=True, timeout=180, stdin=subprocess.DEVNULL, capture_output=True,
    )
    got = [f for f in glob.glob(base + ".*") if not f.endswith(".part")]
    if not got:
        raise RuntimeError("yt-dlp wrote nothing")
    to_wav(got[0], wav)


def subsonic_url(song_id):
    url, user, pw = (os.environ.get(k, "") for k in ("LISTEN_ND_URL", "LISTEN_ND_USER", "LISTEN_ND_PASS"))
    if not (url and user and pw):
        return None
    salt = secrets.token_hex(6)
    q = {
        "id": song_id, "u": user, "t": hashlib.md5((pw + salt).encode()).hexdigest(), "s": salt,
        "v": "1.16.1", "c": "listen-embed", "format": "raw",
    }
    return url.rstrip("/") + "/rest/stream?" + urllib.parse.urlencode(q)


def clip_subsonic(song_id, start, wav):
    url = subsonic_url(song_id)
    if not url:
        raise RuntimeError("no Navidrome credentials (LISTEN_ND_URL/USER/PASS)")
    # The URL carries the login (u/t/s) and every local user can read a process's arguments, so it never goes
    # into ffmpeg's command line: fetch it here and pipe the audio in.
    with urllib.request.urlopen(url, timeout=180) as r:
        data = r.read()
    to_wav("pipe:0", wav, start=start, stdin=data)


def to_wav(src, wav, start=None, stdin=None):
    """`src` is a local file, a public URL or `pipe:0` with the audio in `stdin` (bytes)."""
    cmd = ["ffmpeg", "-loglevel", "error", "-y"]
    if start is not None:
        cmd += ["-ss", str(start)]
    cmd += ["-i", src, "-t", str(CLIP_S), "-ac", "1", "-ar", "24000", wav]
    if stdin is None:
        subprocess.run(cmd, check=True, timeout=180, stdin=subprocess.DEVNULL, capture_output=True)
    else:
        subprocess.run(cmd, check=True, timeout=180, input=stdin, capture_output=True)
    if os.path.getsize(wav) < 24000 * 2 * 5:
        raise RuntimeError("clip shorter than 5 s")


def nd_host():
    url = os.environ.get("LISTEN_ND_URL", "")
    return urllib.parse.urlparse(url).hostname or ""


def hint_host(hint):
    """A stored server_hint as listen-core's describe::server_hint_of writes it (lower case, no user/port)."""
    if not hint:
        return ""
    return urllib.parse.urlparse("//" + hint).hostname or ""


# ---- CLaMP 3 -----------------------------------------------------------------------------------------


class Clamp3:
    """CLaMP 3 (SAAS) text and audio towers plus MERT-v1-95M, from a checkout of the CLaMP 3 repository."""

    def __init__(self, repo, audio=True):
        import torch

        sys.path.insert(0, os.path.join(repo, "code"))
        sys.path.insert(0, os.path.join(repo, "preprocessing", "audio"))
        import config as C
        from transformers import AutoTokenizer, BertConfig
        from utils import CLaMP3Model

        self.torch, self.C = torch, C
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        audio_config = BertConfig(
            vocab_size=1, hidden_size=C.AUDIO_HIDDEN_SIZE, num_hidden_layers=C.AUDIO_NUM_LAYERS,
            num_attention_heads=C.AUDIO_HIDDEN_SIZE // 64, intermediate_size=C.AUDIO_HIDDEN_SIZE * 4,
            max_position_embeddings=C.MAX_AUDIO_LENGTH,
        )
        symbolic_config = BertConfig(
            vocab_size=1, hidden_size=C.M3_HIDDEN_SIZE, num_hidden_layers=C.PATCH_NUM_LAYERS,
            num_attention_heads=C.M3_HIDDEN_SIZE // 64, intermediate_size=C.M3_HIDDEN_SIZE * 4,
            max_position_embeddings=C.PATCH_LENGTH,
        )
        model = CLaMP3Model(
            audio_config=audio_config, symbolic_config=symbolic_config, text_model_name=C.TEXT_MODEL_NAME,
            hidden_size=C.CLAMP3_HIDDEN_SIZE, load_m3=C.CLAMP3_LOAD_M3,
        )
        weights = os.path.join(repo, "code", os.path.basename(C.CLAMP3_WEIGHTS_PATH))
        if not os.path.exists(weights):
            raise SystemExit(f"CLaMP 3 weights missing: {weights} (see README.md)")
        ckpt = torch.load(weights, map_location="cpu", weights_only=True)
        model.load_state_dict(ckpt["model"])
        self.model = model.to(self.device).eval()
        self.tokenizer = AutoTokenizer.from_pretrained(C.TEXT_MODEL_NAME)
        self.mert = None
        if audio:
            from hf_pretrains import HuBERTFeature

            self.mert = HuBERTFeature("m-a-p/MERT-v1-95M", 24000, force_half=False, processor_normalize=True)
            self.mert.to(self.device).eval()

    def text(self, lines):
        """Global text embedding of lines (joined with the separator token, as CLaMP 3 does)."""
        torch, C = self.torch, self.C
        text = self.tokenizer.sep_token.join([l for l in lines if l])
        ids = self.tokenizer(text, return_tensors="pt")["input_ids"].squeeze(0)
        return self._global(ids, C.MAX_TEXT_LENGTH, self.tokenizer.pad_token_id, "text")

    def audio(self, wav):
        torch, C = self.torch, self.C
        from MERT_utils import load_audio

        with torch.no_grad():
            w = load_audio(wav, target_sr=24000, is_mono=True, is_normalize=False, device=self.device)
            w = self.mert.process_wav(w).to(self.device)
            step = 24000 * 5
            chunks = [w[:, i : i + step] for i in range(0, w.shape[-1], step)]
            if chunks and chunks[-1].shape[-1] < 24000:
                chunks = chunks[:-1]
            if not chunks:
                raise RuntimeError("clip too short")
            feats = torch.cat([self.mert(c, layer=None, reduction="mean") for c in chunks], dim=1)
            feats = feats.mean(dim=0, keepdim=True).reshape(-1, C.AUDIO_HIDDEN_SIZE).float().cpu()
        zero = torch.zeros((1, feats.size(-1)))
        data = torch.cat((zero, feats, zero), 0)
        return self._global(data, C.MAX_AUDIO_LENGTH, 0.0, "audio")

    def _global(self, data, max_len, pad, kind):
        torch = self.torch
        segs = [data[i : i + max_len] for i in range(0, len(data), max_len)]
        segs[-1] = data[-max_len:]
        outs = []
        with torch.no_grad():
            for seg in segs:
                n = seg.size(0)
                mask = torch.cat((torch.ones(n), torch.zeros(max_len - n)), 0).unsqueeze(0)
                if kind == "text":
                    seg = torch.cat((seg, torch.ones(max_len - n).long() * pad), 0).unsqueeze(0)
                    o = self.model.get_text_features(text_inputs=seg.to(self.device), text_masks=mask, get_global=True)
                else:
                    seg = torch.cat((seg, torch.zeros((max_len - n, seg.size(-1)))), 0).unsqueeze(0)
                    o = self.model.get_audio_features(audio_inputs=seg.to(self.device), audio_masks=mask, get_global=True)
                outs.append(o)
        full, rem = divmod(len(data), max_len)
        w = [max_len] * full + ([rem] if rem else [])
        w = torch.tensor(w, device=self.device, dtype=torch.float32).view(-1, 1)
        v = (torch.cat(outs, 0) * w).sum(0) / w.sum()
        return v.float().cpu().numpy()


# ---- commands ----------------------------------------------------------------------------------------


def cmd_run(a):
    if not gemini_key():
        raise SystemExit("set GEMINI_API_KEY")
    songs = store_songs(a.store)
    done = done_records(a.out)
    retry = a.retry_audio
    todo = [
        sid for sid in songs
        if sid not in done or (retry and not done[sid].get("audio") and done[sid].get("tags"))
    ]
    host = nd_host()
    if not a.navidrome:
        todo = [s for s in todo if s.startswith("ytm:")]
    elif host:
        # An empty hint names no server; describe::server_for maps it to the only Navidrome server, which is
        # LISTEN_ND_URL here.
        todo = [s for s in todo if s.startswith("ytm:") or hint_host(songs[s]["server_hint"]) in (host, "")]
    if a.only:
        only = set(json.load(open(a.only)))
        todo = [s for s in todo if s in only]
    plays = play_counts(a.history)
    todo.sort(key=lambda s: (-plays.get(s, 0), s))
    if a.limit:
        todo = todo[: a.limit]
    log(f"store {len(songs)} songs, embedded {len(done)}, to do now {len(todo)}")
    if not todo:
        return
    clamp = Clamp3(a.clamp3, audio=True)
    os.makedirs(out_dir(a.out), exist_ok=True)
    path = os.path.join(out_dir(a.out), f"{a.machine}.jsonl")
    end_torn_line(path)
    stop = []  # SIGTERM / SIGINT: finish the song in hand, then stop (the next run resumes)
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, lambda *_: stop.append(True))
    tmp = tempfile.mkdtemp(prefix="listen-embed-")
    wav = os.path.join(tmp, "clip.wav")
    n_ok = n_fail = 0
    try:
        for b in range(0, len(todo), TAG_BATCH):
            if stop:
                break
            batch = todo[b : b + TAG_BATCH]
            tags = [None] * len(batch)
            have = [done.get(s) for s in batch]
            need = [i for i, h in enumerate(have) if not (h and h.get("tags_en"))]
            for i, h in enumerate(have):
                if h and h.get("tags_en"):
                    tags[i] = (h["tags_en"], h.get("tags_ko", ""))
            if need:
                for attempt in range(3):
                    try:
                        got = tag_batch([songs[batch[i]]["title_hint"] for i in need])
                        for i, t in zip(need, got):
                            tags[i] = t
                        break
                    except Exception as e:  # noqa: BLE001 - network, quota: wait and retry
                        log(f"gemini: {e}")
                        time.sleep(20 * (attempt + 1))
            with open(path, "a", encoding="utf-8") as out:
                for sid, t in zip(batch, tags):
                    if stop:
                        break
                    s = songs[sid]
                    rec = {
                        "song_id": sid, "title_hint": s["title_hint"], "duration_ms": s["duration_ms"],
                        "server_hint": s["server_hint"], "at": int(time.time()),
                    }
                    if t:
                        rec["tags_en"], rec["tags_ko"] = t
                        rec["tags"] = vec_b64(clamp.text([t[0], t[1]]))
                    start = int(s["duration_ms"] / 1000 * CLIP_AT) if s["duration_ms"] else 30
                    try:
                        if sid.startswith("ytm:"):
                            clip_ytm(sid[4:], start, wav, tmp)
                        else:
                            clip_subsonic(sid, start, wav)
                        rec["audio"] = vec_b64(clamp.audio(wav))
                        n_ok += 1
                    except Exception as e:  # noqa: BLE001 - one song failing must not stop the job
                        msg = getattr(e, "stderr", b"") or b""
                        msg = msg.decode(errors="replace") if isinstance(msg, bytes) else str(msg)
                        rec["audio_error"] = (msg.strip().splitlines() or [str(e)])[-1][:200]
                        n_fail += 1
                    if "tags" not in rec and "audio" not in rec:
                        continue
                    out.write(json.dumps(rec, ensure_ascii=False) + "\n")
                    out.flush()
                    time.sleep(a.pause + random.random())
            if not stop:
                log(f"{min(b + TAG_BATCH, len(todo))}/{len(todo)} audio ok {n_ok}, failed {n_fail}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    if stop:
        log(f"stopped: audio ok {n_ok}, failed {n_fail}; the next run resumes")


def cmd_export_text(a):
    """CLaMP 3's text tower (XLM-R base + projection) for listen-server, plus reference vectors."""
    import torch
    from safetensors.torch import save_file

    clamp = Clamp3(a.clamp3, audio=False)
    dest = os.path.join(out_dir(a.out), "text")
    os.makedirs(dest, exist_ok=True)
    tensors = {k: v.detach().contiguous().cpu() for k, v in clamp.model.text_model.state_dict().items()}
    tensors["text_proj.weight"] = clamp.model.text_proj.weight.detach().contiguous().cpu()
    tensors["text_proj.bias"] = clamp.model.text_proj.bias.detach().contiguous().cpu()
    tensors = {k: v.to(torch.float16) if v.is_floating_point() else v for k, v in tensors.items()}
    save_file(tensors, os.path.join(dest, "model.safetensors"))
    clamp.model.text_model.config.to_json_file(os.path.join(dest, "config.json"))
    clamp.tokenizer.backend_tokenizer.save(os.path.join(dest, "tokenizer.json"))
    checks = ["느린 90년대 한국 발라드", "Brazilian samba with acoustic guitar", "트로트"]
    with open(os.path.join(dest, "check.json"), "w", encoding="utf-8") as f:
        json.dump([{"q": q, "vec": [float(x) for x in clamp.text([q])]} for q in checks], f, ensure_ascii=False)
    log(f"text tower written to {dest}")


def cmd_status(a):
    songs = store_songs(a.store)
    done = done_records(a.out)
    audio = sum(1 for s in songs if done.get(s, {}).get("audio"))
    tags = sum(1 for s in songs if done.get(s, {}).get("tags"))
    print(json.dumps({"store": len(songs), "lines": len(done), "audio": audio, "tags": tags,
                      "missing": len([s for s in songs if s not in done])}))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--store", default=DEFAULT_STORE, help="analysis store v1 folder")
    p.add_argument("--out", default=DEFAULT_OUT, help="embeddings folder (v1/clamp3/ goes inside)")
    p.add_argument("--clamp3", default=os.environ.get("CLAMP3_REPO", os.path.join(HOME, ".cache/listen-embed/clamp3")))
    p.add_argument("--machine", default=socket.gethostname().split(".")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="embed songs that have no line yet")
    r.add_argument("--limit", type=int, default=0, help="at most this many songs (0: all)")
    r.add_argument("--only", help="JSON list of song ids to restrict to")
    r.add_argument("--history", default=DEFAULT_HISTORY, help="history.jsonl: played songs go first")
    r.add_argument("--pause", type=float, default=1.5, help="seconds between downloads")
    r.add_argument("--navidrome", action="store_true", help="also Navidrome songs (needs LISTEN_ND_*)")
    r.add_argument("--retry-audio", action="store_true", help="retry songs whose audio failed before")
    sub.add_parser("export-text", help="write the text tower for listen-server")
    sub.add_parser("status", help="counts")
    a = p.parse_args()
    {"run": cmd_run, "export-text": cmd_export_text, "status": cmd_status}[a.cmd](a)


if __name__ == "__main__":
    main()
