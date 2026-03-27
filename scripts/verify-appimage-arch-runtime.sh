#!/usr/bin/env bash

set -euo pipefail

APPDIR="${1:-target/release/bundle/appimage/WriterMD.AppDir}"
APP_RUN="$APPDIR/AppRun"
LOG_FILE="$(mktemp)"

cleanup() {
  rm -f "$LOG_FILE"
}
trap cleanup EXIT

if [[ ! -x "$APP_RUN" ]]; then
  printf 'missing executable AppRun: %s\n' "$APP_RUN" >&2
  exit 1
fi

set +e
timeout 8s "$APP_RUN" >"$LOG_FILE" 2>&1
status=$?
set -e

if grep -Fq 'Unable to load resource for composite template' "$LOG_FILE"; then
  cat "$LOG_FILE" >&2
  printf '\nGTK resource failure detected\n' >&2
  exit 1
fi

if grep -Fq 'GtkFileChooserDialog' "$LOG_FILE"; then
  cat "$LOG_FILE" >&2
  printf '\nGtkFileChooserDialog failure detected\n' >&2
  exit 1
fi

if grep -Fqi 'dumped core' "$LOG_FILE"; then
  cat "$LOG_FILE" >&2
  printf '\ncore dump detected\n' >&2
  exit 1
fi

if [[ $status -ne 0 && $status -ne 124 ]]; then
  cat "$LOG_FILE" >&2
  printf '\nAppRun exited unexpectedly with status %s\n' "$status" >&2
  exit 1
fi

printf 'AppImage Arch runtime smoke check passed for %s\n' "$APPDIR"
