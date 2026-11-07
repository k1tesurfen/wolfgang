#!/bin/bash
# Builds Wolfgang.app and the .dmg for macOS (Apple Silicon).
#
#   scripts/build-app.sh
#
# Needs: Rust, cmake + nasm (brew install cmake nasm), Tauri CLI
# (cargo install tauri-cli --version "^2" --locked).
# Result: target/release/bundle/macos/Wolfgang.app and target/release/bundle/dmg/*.dmg

set -euo pipefail
cd "$(dirname "$0")/.."

step() { echo; echo "▶ $1"; }

step "Pixel art (assets/pixel) aktuell?"
cargo run --quiet --release -p wolfgang-art -- build >/dev/null

step "Kommandozeile bauen (kommt mit in die App)"
cargo build --release -p wolfgang-cli

step "App + DMG bauen"
(cd crates/wolfgang-app && cargo tauri build --bundles app,dmg)

APP="target/release/bundle/macos/Wolfgang.app"
step "Prüfen"
[[ -x "$APP/Contents/MacOS/wolfgang" ]] || { echo "Kommandozeile fehlt in der App"; exit 1; }
"$APP/Contents/MacOS/wolfgang" --version
codesign --verify --deep --strict "$APP" && echo "Signatur (ad-hoc) ok"
/usr/libexec/PlistBuddy -c "Print :CFBundleDocumentTypes:0:LSHandlerRank" "$APP/Contents/Info.plist" | grep -q Alternate && echo "Dock-Ablage ok"

echo
ls -lh target/release/bundle/dmg/*.dmg
