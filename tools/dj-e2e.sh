#!/bin/bash
# End-to-end test of DJ transitions (docs/dj-transitions.md): synthetic songs on a
# local Navidrome, a private PipeWire null sink, Listen headless under Xvfb with
# Auto-DJ and mixing on; then summarises the mixer log and the sink recording.
#
#   tools/dj-e2e.sh [minutes]        # default 8
#
# Environment:
#   DJ_E2E_DIR      work folder (default /tmp/dj-e2e); music/ is kept, run-* removed
#   DJ_E2E_KEEP=1   keep the run folder (logs, recording) for inspection
#   DJ_E2E_RECORD=0 don't record the sink
#   DJ_E2E_ACTIONS=1  press MPRIS buttons during blends 2–5 (next, pause/play,
#                   seek back, next while preloading) to test the cancel paths
#   NAVIDROME       navidrome binary (default ~/navidrome/navidrome)
#   LISTEN_BIN      the app (default target/release/listen, built if missing)
#   DJ_E2E_MODELS   beat model cache kept between runs
#                   (default ~/.cache/listen-dj-e2e/models)
#
# Isolated: HOME and XDG dirs point into the run folder, `--login` writes no
# credentials, and the app gets its own D-Bus session and audio graph.
set -euo pipefail
MINUTES=${1:-8}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=${DJ_E2E_DIR:-/tmp/dj-e2e}
MUSIC=$WORK/music
RUN=$WORK/run-$(date +%Y%m%d-%H%M%S)
NAVIDROME=${NAVIDROME:-$HOME/navidrome/navidrome}
RECORD=${DJ_E2E_RECORD:-1}
ACTIONS=${DJ_E2E_ACTIONS:-0}
MODELS=${DJ_E2E_MODELS:-$HOME/.cache/listen-dj-e2e/models}
PW_DIR=/tmp/listen-pw
PORT=4533
USER_NAME=admin
PASSWORD=listen-test

PIDS=()
cleanup() {
  set +e
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null; done
  sleep 0.5
  for pid in "${PIDS[@]}"; do kill -9 "$pid" 2>/dev/null; done
  LISTEN_PW_DIR=$PW_DIR bash "$ROOT/tools/s0b-pipewire.sh" stop >/dev/null
  if [[ ${DJ_E2E_KEEP:-0} != 1 && -d $RUN ]]; then
    rm -rf -- "$RUN"
  fi
}
trap cleanup EXIT

LISTEN=${LISTEN_BIN:-$ROOT/target/release/listen}
if [[ ! -x $LISTEN ]]; then
  (cd "$ROOT" && cargo build --release -p listen)
fi
if [[ ! -f $MUSIC/truth.json ]]; then
  (cd "$ROOT" && cargo run --release -p listen-analysis --example djset -- "$MUSIC")
fi
(cd "$ROOT" && cargo build --release -p listen-analysis --example djrec -q)
DJREC=$ROOT/target/release/examples/djrec
mkdir -p "$RUN/nd" "$RUN/home"
LOG=$RUN/home/.local/share/listen/logs/listen.log

# Navidrome on a fresh database: the first user becomes the admin.
ND_MUSICFOLDER=$MUSIC ND_DATAFOLDER=$RUN/nd ND_PORT=$PORT ND_ADDRESS=127.0.0.1 \
  ND_LOGLEVEL=warn ND_ENABLEINSIGHTSCOLLECTOR=false ND_SCANNER_SCHEDULE=0 \
  "$NAVIDROME" >"$RUN/navidrome.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 100); do curl -sf -o /dev/null "http://127.0.0.1:$PORT/ping" && break; sleep 0.1; done
curl -sf -X POST -H 'Content-Type: application/json' \
  -d "{\"username\":\"$USER_NAME\",\"password\":\"$PASSWORD\"}" \
  "http://127.0.0.1:$PORT/auth/createAdmin" >/dev/null
want=$(find "$MUSIC" -name '*.mp3' -o -name '*.flac' | wc -l)
n=0
for _ in $(seq 100); do
  n=$(curl -sf "http://127.0.0.1:$PORT/rest/getRandomSongs?u=$USER_NAME&p=$PASSWORD&v=1.16.1&c=e2e&f=json&size=500" |
    grep -o '"isDir":false' | wc -l || true)
  [[ $n -ge $want ]] && break
  sleep 0.2
done
echo "navidrome: $n/$want songs"

LISTEN_PW_DIR=$PW_DIR bash "$ROOT/tools/s0b-pipewire.sh" start >/dev/null

REC_PID=
fail=0
RUN_START=$(date +%s.%N)
if [[ $RECORD == 1 ]]; then
  # Sink first (tools/s0b-pipewire.sh makes it the default), then a linked recorder
  # delivering data before the app starts; rec.start is taken once data flows.
  for _ in $(seq 100); do
    XDG_RUNTIME_DIR=$PW_DIR pw-cli ls Node 2>/dev/null | grep -q '"s0b-sink"' && break
    sleep 0.1
  done
  XDG_RUNTIME_DIR=$PW_DIR pw-record --target s0b-sink -P stream.capture.sink=true --rate 48000 \
    --channels 2 --format s16 "$RUN/rec.wav" 2>"$RUN/pw-record.log" &
  REC_PID=$!
  PIDS+=($REC_PID)
  linked=0
  for _ in $(seq 100); do
    kill -0 "$REC_PID" 2>/dev/null || break
    # A linked recorder shows up as a link from the sink's monitor ports.
    if XDG_RUNTIME_DIR=$PW_DIR pw-link -l 2>/dev/null | grep -q 's0b-sink:monitor_FL' &&
      [[ $(stat -c %s "$RUN/rec.wav" 2>/dev/null || echo 0) -gt 4096 ]]; then
      linked=1
      break
    fi
    sleep 0.1
  done
  if [[ $linked != 1 ]]; then
    echo "ERROR: pw-record never linked to s0b-sink (see $RUN/pw-record.log); the recording is not usable" >&2
    cat "$RUN/pw-record.log" >&2 || true
    exit 1
  fi
  date +%s.%N >"$RUN/rec.start"
fi

# The beat model is downloaded once and kept between runs.
mkdir -p "$RUN/home/.cache/listen/models" "$MODELS"
cp -a "$MODELS/." "$RUN/home/.cache/listen/models/" 2>/dev/null || true

# Buttons over MPRIS, timed by what the log says (see DJ_E2E_ACTIONS).
act() {
  local name
  name=$(DBUS_SESSION_BUS_ADDRESS=$(cat "$RUN/dbus.addr") dbus-send --session --print-reply \
    --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.ListNames |
    grep -o 'org.mpris.MediaPlayer2.Listen[^"]*' | head -1)
  echo "$(date +%s.%N) $1" >>"$RUN/actions.txt"
  DBUS_SESSION_BUS_ADDRESS=$(cat "$RUN/dbus.addr") dbus-send --session --type=method_call \
    --dest="$name" /org/mpris/MediaPlayer2 "org.mpris.MediaPlayer2.Player.$1" "${@:2}"
}
actions() {
  local blends=0 preloads=0 n
  while sleep 0.1; do
    [[ -f $LOG ]] || continue
    n=$(grep -c 'mix blend: starts in' "$LOG" || true)
    if ((n > blends)); then
      blends=$n
      case $blends in
        2) (sleep 3 && act Next) & ;;
        3) (sleep 2 && act Pause && sleep 3 && act Play) & ;;
        4) (sleep 2 && act Seek int64:-8000000) & ;;
      esac
    fi
    n=$(grep -c 'mix preload' "$LOG" || true)
    if ((n > preloads)); then
      preloads=$n
      [[ $preloads == 5 ]] && (sleep 1 && act Next) &
    fi
  done
}
if [[ $ACTIONS == 1 ]]; then
  actions &
  PIDS+=($!)
fi

echo "playing for $MINUTES min (log: $LOG)"
HOME=$RUN/home XDG_CONFIG_HOME=$RUN/home/.config XDG_DATA_HOME=$RUN/home/.local/share \
  XDG_CACHE_HOME=$RUN/home/.cache XDG_RUNTIME_DIR=$PW_DIR \
  LISTEN_PASSWORD=$PASSWORD LISTEN_ANALYSIS=1 LISTEN_AUTOPLAY_ALBUM=Run RUST_LOG=listen=info,warn \
  dbus-run-session -- bash -c 'echo "$DBUS_SESSION_BUS_ADDRESS" >"$0"; exec "$@"' "$RUN/dbus.addr" \
  env -u WAYLAND_DISPLAY timeout --signal=TERM "$((MINUTES * 60))" xvfb-run -a "$LISTEN" \
  --login "http://127.0.0.1:$PORT" "$USER_NAME" --autoplay >"$RUN/listen.out" 2>&1 || true
cp -a "$RUN/home/.cache/listen/models/." "$MODELS/" 2>/dev/null || true
cp "$LOG" "$WORK/last-listen.log"

echo
echo "== what played =="
grep -E 'INFO.*(mix “[^”]*” → “[^”]*”: [A-Z]|gapless advance|mix handover|mix finished|mix cancelled|mix phase lock|playing “)' "$LOG" |
  sed -E 's/^[0-9-]+T([0-9:]+)\.[0-9]+Z +[A-Z]+ +[a-z_:]+: /\1 /' || true
if [[ -f $RUN/actions.txt ]]; then
  echo
  echo "== buttons pressed =="
  while read -r t what; do echo "$(date -u -d "@$t" +%H:%M:%S) $what"; done <"$RUN/actions.txt"
fi
echo
echo "== summary =="
kinds=$(grep -oE 'INFO.*mix “[^”]*” → “[^”]*”: [A-Z][A-Za-z]*' "$LOG" | sed 's/.*: //' | sort | uniq -c |
  awk '{printf "%s %s, ", $1, $2}' || true)
echo "planned:   $(grep -cE 'INFO.*mix “[^”]*” → “[^”]*”: [A-Z]' "$LOG" || true) (${kinds%, })"
echo "blends:    $(grep -c 'mix blend: starts in' "$LOG" || true)"
echo "handovers: $(grep -c 'mix handover' "$LOG" || true)"
echo "finished:  $(grep -c 'mix finished' "$LOG" || true)"
echo "cancelled: $(grep -c 'mix cancelled' "$LOG" || true)"
echo "gapless:   $(grep -c 'gapless advance' "$LOG" || true)"
echo "errors:    $(grep -E ' (ERROR|WARN) listen' "$LOG" | grep -cv 'keyring unavailable' || true)"
# Phase errors once the lock has settled (2 s after the incoming deck started).
grep -o 'mix phase lock: .*' "$LOG" >/dev/null && "$DJREC" locks "$LOG"
if [[ $RECORD == 1 ]]; then
  echo
  echo "== recording =="
  # Stop the recorder cleanly (SIGINT finalises the WAV header) before reading.
  if [[ -n $REC_PID ]]; then
    kill -INT "$REC_PID" 2>/dev/null || true
    for _ in $(seq 50); do kill -0 "$REC_PID" 2>/dev/null || break; sleep 0.1; done
  fi
  RUN_END=$(date +%s.%N)
  # 48 kHz, 2 channels, s16 = 192000 bytes/s, 44 byte header.
  bytes=$(stat -c %s "$RUN/rec.wav" 2>/dev/null || echo 0)
  rec_s=$(awk -v b="$bytes" 'BEGIN { printf "%.1f", (b > 44 ? b - 44 : 0) / 192000 }')
  run_s=$(awk -v a="$(cat "$RUN/rec.start")" -v b="$RUN_END" 'BEGIN { printf "%.1f", b - a }')
  echo "recorded ${rec_s}s of ${run_s}s run"
  if awk -v r="$rec_s" -v t="$run_s" 'BEGIN { exit !(r < t - 5) }'; then
    echo "ERROR: recording is shorter than the run by more than 5 s; its gap/bump flags are not trustworthy" >&2
    fail=1
  fi
  # Silence check: some byte after the header is non-zero. Counted with `wc`, as
  # `head -c 1 | grep -q` would trip pipefail when `head` closes the pipe early.
  nonzero=$(tail -c +45 "$RUN/rec.wav" | tr -d '\0' | head -c 4096 | wc -c || true)
  if [[ $bytes -le 44 || ${nonzero:-0} -eq 0 ]]; then
    echo "ERROR: recording is pure silence; its gap/bump flags are not trustworthy" >&2
    fail=1
  fi
  echo "audio present: the recording has non-zero samples"
  "$DJREC" check "$RUN/rec.wav" "$(cat "$RUN/rec.start")" "$LOG" "$RUN/actions.txt"
fi
exit $fail
