# WriterMD
<img width="1024" height="700" alt="image" src="https://github.com/user-attachments/assets/3e044c61-b5d9-4488-98a4-b8eec879b42d" />

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

## Releasing

- Bump the release version in [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml).
- If the source icon changes, regenerate the Tauri icon set with:

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo tauri icon src-tauri/icons/app-icon.png --output src-tauri/icons
```

- Push a matching git tag in the form `vX.Y.Z`.
- GitHub Actions publishes these release artifacts:
- Linux `x86_64`: AppImage, `.deb`, `.rpm`
- Linux `arm64`: AppImage, `.deb`, `.rpm`
- macOS `x86_64`: `.dmg`
- macOS `arm64`: `.dmg`
- Windows `x86_64`: `.exe`
- Windows `arm64`: `.exe`

### Arch Linux AppImage workaround

Tauri's cached AppImage helpers currently need two Arch-specific workarounds in this repo:

- disable stripping because older bundled tooling fails on `.relr.dyn` ELF sections
- patch the cached GTK helper to tolerate Arch's missing `gdk-pixbuf-2.0/2.10.0` directory
- replace the broken bundled GTK/WebKit runtime in locally built AppImages with the host Arch runtime after bundling

Prepare the cache once after Tauri has downloaded its AppImage helpers:

```bash
./scripts/setup-appimage-arch.sh
```

Then build the AppImage with:

```bash
NO_COLOR=false RUST_BACKTRACE=1 LDAI_VERBOSE=1 NO_STRIP=1 cargo tauri bundle -v -b appimage
```

Patch the generated AppDir so the local Arch AppImage uses the host GTK/WebKit stack:

```bash
./scripts/patch-appimage-arch-runtime.sh target/release/bundle/appimage/WriterMD.AppDir
```

Run the smoke check before launching the patched AppImage:

```bash
./scripts/verify-appimage-arch-runtime.sh
```

This Arch-only workaround is intentional. The locally bundled GTK runtime currently starts with missing GTK template resources and later crashes on save through `GtkFileChooserDialog`, so the local AppImage now prefers the host Arch GTK/WebKit runtime instead of the broken bundled copy.

## License

See [LICENSE](LICENSE).
