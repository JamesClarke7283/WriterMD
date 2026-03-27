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

## License

See [LICENSE](LICENSE).
