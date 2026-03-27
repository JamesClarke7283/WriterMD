#!/usr/bin/env bash

set -euo pipefail

TAURI_CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/tauri"
LINUXDEPLOY_PATH="$TAURI_CACHE_DIR/linuxdeploy-x86_64.AppImage"
GTK_PLUGIN_PATH="$TAURI_CACHE_DIR/linuxdeploy-plugin-gtk.sh"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/latest/download/linuxdeploy-x86_64.AppImage"

log() {
  printf '%s\n' "$*"
}

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required tool: $1"
    exit 1
  fi
}

patch_gtk_plugin() {
  if [[ ! -f "$GTK_PLUGIN_PATH" ]]; then
    log "gtk plugin not found at $GTK_PLUGIN_PATH"
    log "run a Tauri AppImage bundle once so Tauri downloads its helper scripts, then rerun this script"
    exit 1
  fi

  if grep -q 'gdk-pixbuf binary directory not found' "$GTK_PLUGIN_PATH"; then
    log "gtk plugin already patched"
    return
  fi

  python - <<'PY' "$GTK_PLUGIN_PATH"
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()

old = """gdk_pixbuf_query=\"$(search_tool \"gdk-pixbuf-query-loaders\" \"gdk-pixbuf-2.0\")\"\ncopy_tree \"$gdk_pixbuf_binarydir\" \"$APPDIR/\"\ncat >> \"$HOOKFILE\" <<EOF\nexport GDK_PIXBUF_MODULE_FILE=\"\\$APPDIR/$gdk_pixbuf_cache_file\"\nEOF\nif [ -x \"$gdk_pixbuf_query\" ]; then\n    echo \"Updating pixbuf cache in $APPDIR/$gdk_pixbuf_cache_file\"\n    \"$gdk_pixbuf_query\" > \"$APPDIR/$gdk_pixbuf_cache_file\"\nelse\n    echo \"WARNING: gdk-pixbuf-query-loaders not found\"\nfi\nif [ ! -f \"$APPDIR/$gdk_pixbuf_cache_file\" ]; then\n    echo \"WARNING: loaders.cache file is missing\"\nfi\nsed -i \"s|$gdk_pixbuf_moduledir/||g\" \"$APPDIR/$gdk_pixbuf_cache_file\"\n"""

new = """gdk_pixbuf_query=\"$(search_tool \"gdk-pixbuf-query-loaders\" \"gdk-pixbuf-2.0\")\"\nif [ -d \"$gdk_pixbuf_binarydir\" ]; then\n    copy_tree \"$gdk_pixbuf_binarydir\" \"$APPDIR/\"\nelse\n    echo \"WARNING: gdk-pixbuf binary directory not found: $gdk_pixbuf_binarydir\"\nfi\nmkdir -p \"$APPDIR/$(dirname \"$gdk_pixbuf_cache_file\")\"\ncat >> \"$HOOKFILE\" <<EOF\nexport GDK_PIXBUF_MODULE_FILE=\"\\$APPDIR/$gdk_pixbuf_cache_file\"\nEOF\nif [ -x \"$gdk_pixbuf_query\" ]; then\n    echo \"Updating pixbuf cache in $APPDIR/$gdk_pixbuf_cache_file\"\n    \"$gdk_pixbuf_query\" > \"$APPDIR/$gdk_pixbuf_cache_file\" || true\nelse\n    echo \"WARNING: gdk-pixbuf-query-loaders not found\"\nfi\nif [ ! -f \"$APPDIR/$gdk_pixbuf_cache_file\" ]; then\n    echo \"WARNING: loaders.cache file is missing\"\nfi\nif [ -f \"$APPDIR/$gdk_pixbuf_cache_file\" ]; then\n    sed -i \"s|$gdk_pixbuf_moduledir/||g\" \"$APPDIR/$gdk_pixbuf_cache_file\"\nfi\n"""

old_patch_array = """PATCH_ARRAY=(\n    \"$gtk3_immodulesdir\"\n    \"$gtk3_printbackendsdir\"\n    \"$gdk_pixbuf_moduledir\"\n)\n"""
new_patch_array = """PATCH_ARRAY=(\n    \"$gtk3_immodulesdir\"\n    \"$gtk3_printbackendsdir\"\n)\nif [ -d \"$gdk_pixbuf_moduledir\" ]; then\n    PATCH_ARRAY+=(\"$gdk_pixbuf_moduledir\")\nfi\n"""

if old not in text or old_patch_array not in text:
    raise SystemExit("expected gtk plugin block not found; upstream helper changed")

text = text.replace(old, new, 1)
text = text.replace(old_patch_array, new_patch_array, 1)
path.write_text(text)
PY

  chmod +x "$GTK_PLUGIN_PATH"
  log "patched gtk plugin at $GTK_PLUGIN_PATH"
}

main() {
  require_tool curl
  require_tool chmod
  require_tool python

  mkdir -p "$TAURI_CACHE_DIR"

  log "downloading current linuxdeploy to $LINUXDEPLOY_PATH"
  curl -fL -o "$LINUXDEPLOY_PATH" "$LINUXDEPLOY_URL"
  chmod 770 "$LINUXDEPLOY_PATH"

  patch_gtk_plugin

  cat <<'EOF'

Arch AppImage cache prepared.

Bundle with:
  NO_COLOR=false RUST_BACKTRACE=1 LDAI_VERBOSE=1 NO_STRIP=1 cargo tauri bundle -v -b appimage
EOF
}

main "$@"
