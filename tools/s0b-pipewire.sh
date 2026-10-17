#!/bin/bash
# A private PipeWire + WirePlumber with one null sink ("s0b-sink"), for measuring
# mpv decks without touching the desktop's audio (see docs/dj-transitions.md).
#
#   tools/s0b-pipewire.sh start   # then run with XDG_RUNTIME_DIR=$DIR (printed)
#   tools/s0b-pipewire.sh stop
#
# DIR defaults to /tmp/listen-pw (override with LISTEN_PW_DIR). Unix socket paths
# are limited to ~107 bytes, so keep it short.
set -euo pipefail
DIR=${LISTEN_PW_DIR:-/tmp/listen-pw}

start() {
  if [[ -e $DIR/pipewire.pid ]] && kill -0 "$(cat "$DIR/pipewire.pid")" 2>/dev/null; then
    echo "already running in $DIR"
    return
  fi
  mkdir -p "$DIR"
  chmod 700 "$DIR"
  export XDG_RUNTIME_DIR=$DIR PIPEWIRE_RUNTIME_DIR=$DIR
  pipewire >"$DIR/pipewire.log" 2>&1 &
  echo $! >"$DIR/pipewire.pid"
  for _ in $(seq 50); do [[ -S $DIR/pipewire-0 ]] && break; sleep 0.1; done
  wireplumber >"$DIR/wireplumber.log" 2>&1 &
  echo $! >"$DIR/wireplumber.pid"
  sleep 1
  pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=s0b-sink
    node.description=S0bSink media.class=Audio/Sink object.linger=true
    audio.position=[FL FR] audio.rate=48000 }' >/dev/null
  for _ in $(seq 50); do
    pw-cli ls Node 2>/dev/null | grep -q '"s0b-sink"' && break
    sleep 0.1
  done
  # No hardware here: make the null sink the default for every stream. WirePlumber
  # follows only the configured default, so set both keys.
  for key in default.configured.audio.sink default.audio.sink; do
    pw-metadata -n default 0 "$key" '{ "name": "s0b-sink" }' Spa:String:JSON >/dev/null
  done
  echo "private PipeWire running; use XDG_RUNTIME_DIR=$DIR"
}

stop() {
  for p in wireplumber pipewire; do
    if [[ -e $DIR/$p.pid ]]; then
      kill "$(cat "$DIR/$p.pid")" 2>/dev/null || true
    fi
  done
  sleep 0.5
  rm -rf "$DIR"
  echo "stopped"
}

case ${1:-} in
  start) start ;;
  stop) stop ;;
  *) echo "usage: $0 start|stop" >&2; exit 2 ;;
esac
