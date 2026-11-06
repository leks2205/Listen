# listen-embed

Embeddings for "Describe it" (natural-language library search). Runs on a PC with an NVIDIA GPU; writes
`<embeddings>/v1/clamp3/<machine>.jsonl` (one line per analysed song, format
`listen_core::describe::EmbedRecord`) and CLaMP 3's text tower to `<embeddings>/v1/clamp3/text/` for
listen-server (`EMBED_DIR`). The default `<embeddings>` folder is `Listen/embeddings` in the Nextcloud sync folder (found like
the desktop app finds the analysis store), next to the analysis store it reads; `--store` / `--out` or
`LISTEN_ANALYSIS_DIR` / `LISTEN_EMBED_DIR` override.

Per song: a CLaMP 3 (SAAS) embedding of a 30-s clip from 35 % into the song, search tags Gemini writes from
artist and title (English and Korean), and the CLaMP 3 text embedding of those tags. Songs that were played
(desktop history) go first.

## Setup

```sh
python3 -m venv ~/.cache/listen-embed/venv
~/.cache/listen-embed/venv/bin/pip install --index-url https://download.pytorch.org/whl/cu130 torch torchaudio
~/.cache/listen-embed/venv/bin/pip install -r requirements.txt
git clone https://github.com/sanderwood/clamp3 ~/.cache/listen-embed/clamp3
git -C ~/.cache/listen-embed/clamp3 checkout 9016d2b0c8d12d1aa79c2e0ab201e6822bdc83a8
git -C ~/.cache/listen-embed/clamp3 apply "$PWD/clamp3-compat.patch"   # soundfile loader, newer transformers
# weights (MIT, ~1.8 GB) into the code folder:
cd ~/.cache/listen-embed/clamp3/code && curl -LO https://huggingface.co/sander-wood/clamp3/resolve/main/weights_clamp3_saas_h_size_768_t_model_FacebookAI_xlm-roberta-base_t_length_128_a_size_768_a_layers_12_a_length_128_s_size_768_s_layers_12_p_size_64_p_length_512.pth
```

Needs `yt-dlp` and `ffmpeg` on the PATH. MERT-v1-95M and XLM-R base come from Hugging Face on first use.

## Use

```sh
export GEMINI_API_KEY=…                      # the AI DJ's key
python listen_embed.py export-text           # once: text tower for listen-server (~560 MB, f16)
python listen_embed.py run --limit 500       # the next 500 songs without a line
python listen_embed.py run --navidrome       # also Navidrome songs: LISTEN_ND_URL / LISTEN_ND_USER / LISTEN_ND_PASS
python listen_embed.py status
```

The job appends and can be stopped at any time: SIGTERM or Ctrl+C finishes the song in hand and exits, and a line a killed run left half-written is closed off on the next start. Songs whose clip could not be fetched get a line with tags
only (`audio_error`); `run --retry-audio` tries them again. listen-server lists analysed songs without a line
at `GET /v1/search/missing` (count in `/v1/status`).

Spike results (295 songs, 30 queries, recall@10): audio alone 0.45, audio + tag embedding + tag words 0.68.
