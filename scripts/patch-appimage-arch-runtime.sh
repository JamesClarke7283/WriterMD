#!/usr/bin/env bash

set -euo pipefail

log() {
  printf '%s\n' "$*"
}

require_path() {
  local path="$1"
  local kind="$2"

  if [[ ! -e "$path" ]]; then
    log "missing ${kind}: $path"
    exit 1
  fi
}

main() {
  if [[ $# -ne 1 ]]; then
    log "usage: $0 <AppDir>"
    exit 1
  fi

  local appdir="$1"
  local app_run="$appdir/AppRun"
  local usr_lib="$appdir/usr/lib"
  local gtk_hook="$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh"
  local appimage_dir
  local -a lib_patterns=(
    "libgtk-3.so*"
    "libgdk-3.so*"
    "libgdk_pixbuf-2.0.so*"
    "libglib-2.0.so*"
    "libgobject-2.0.so*"
    "libgio-2.0.so*"
    "libcairo*.so*"
    "libpango*.so*"
    "libatk*.so*"
    "libsoup-3.0.so*"
    "libjavascriptcoregtk-4.1.so*"
    "libwebkit2gtk-4.1.so*"
  )
  local -a dir_paths=(
    "$usr_lib/gtk-3.0"
    "$usr_lib/gio/modules"
    "$usr_lib/gdk-pixbuf-2.0"
    "$usr_lib/webkit2gtk-4.1"
  )

  require_path "$appdir" "AppDir"
  require_path "$app_run" "AppRun"
  require_path "$usr_lib" "usr/lib"
  require_path "$gtk_hook" "gtk hook"

  cat > "$gtk_hook" <<'EOF'
#! /usr/bin/env bash

export APPDIR="${APPDIR:-"$(readlink -f "$(dirname "$0")/..")"}"
export GDK_BACKEND=x11

unset GTK_DATA_PREFIX
unset GTK_EXE_PREFIX
unset GTK_PATH
unset GTK_IM_MODULE_FILE
unset GDK_PIXBUF_MODULE_FILE
unset GIO_EXTRA_MODULES
unset GSETTINGS_SCHEMA_DIR
EOF
  chmod +x "$gtk_hook"

  shopt -s nullglob
  for pattern in "${lib_patterns[@]}"; do
    local matches=("$usr_lib"/$pattern)
    if (( ${#matches[@]} > 0 )); then
      rm -f "${matches[@]}"
    fi
  done
  shopt -u nullglob

  for dir_path in "${dir_paths[@]}"; do
    rm -rf "$dir_path"
  done

  appimage_dir="$(dirname "$appdir")"
  shopt -s nullglob
  for appimage in "$appimage_dir"/*.AppImage; do
    chmod +x "$appimage"
  done
  shopt -u nullglob

  log "patched Arch AppImage runtime in $appdir"
}

main "$@"
