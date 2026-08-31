#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVIDENCE_DIR="$(dirname "$SCRIPT_DIR")"
BASE_URL="http://localhost:1420/"

log() { echo "[run.sh] $*"; }

log "checking $BASE_URL ..."
if ! curl -s -o /dev/null -w "%{http_code}" "$BASE_URL" | grep -qE '^2'; then
  log "ERROR: app is not reachable at $BASE_URL"
  exit 1
fi
log "app reachable"

export DISPLAY="${DISPLAY:-:0}"

run_python() {
  local cmd="$1"
  shift
  if python3 "$SCRIPT_DIR/$cmd" "$@"; then
    return 0
  else
    log "ERROR: $cmd failed"
    return 1
  fi
}

log "running capture.py (headed on DISPLAY=$DISPLAY) ..."
run_python capture.py || exit 1

log "running measure.py ..."
run_python measure.py || exit 1

log "running ocr.py ..."
run_python ocr.py || exit 1

log "all steps completed; evidence in $EVIDENCE_DIR"
