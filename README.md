# WriterMD

A clean, distraction-free Markdown editor built with [Tauri](https://tauri.app/) + [Leptos](https://leptos.dev/).

## Features

- **Distraction-free editing** — minimal UI with a fullscreen editor
- **File operations** — Open, Save, Save As (`.md`, `.markdown`, `.txt`)
- **Dark & Light themes** — toggle via hamburger menu
- **Custom title bar** — integrated hamburger menu, character count, filename, and window controls
- **WASM-powered** — Leptos CSR frontend compiled to WebAssembly

## Prerequisites

```sh
# Tauri CLI
cargo install tauri-cli --version "^2.0.0" --locked

# Rust stable
rustup toolchain install stable --allow-downgrade

# WASM target
rustup target add wasm32-unknown-unknown

# Trunk WASM bundler
cargo install --locked trunk

# wasm-bindgen
cargo install --locked wasm-bindgen-cli

# esbuild (required by tauri-sys)
npm install --global --save-exact esbuild

# TailwindCSS
npm install --global tailwindcss
```

## Running

### Dev mode

```bash
cargo tauri dev
```

### Production build

```bash
cargo tauri build
```

### Arch Linux AppImage workaround

Tauri's cached AppImage helpers currently need two Arch-specific workarounds in this repo:

- disable stripping because older bundled tooling fails on `.relr.dyn` ELF sections
- patch the cached GTK helper to tolerate Arch's missing `gdk-pixbuf-2.0/2.10.0` directory

Prepare the cache once after Tauri has downloaded its AppImage helpers:

```bash
./scripts/setup-appimage-arch.sh
```

Then build the AppImage with:

```bash
NO_COLOR=false RUST_BACKTRACE=1 LDAI_VERBOSE=1 NO_STRIP=1 cargo tauri bundle -v -b appimage
```

## License

See [LICENSE](LICENSE).
