#!/bin/bash
# Builds Wolfgang.app and the .dmg for macOS (Apple Silicon).
#
#   scripts/build-app.sh
#
# Needs: Rust, cmake + nasm (brew install cmake nasm), Tauri CLI
# (cargo install tauri-cli --version "^2" --locked).
# Result: target/release/bundle/macos/Wolfgang.app and target/release/bundle/dmg/*.dmg
#
# Self-update: with the signing key (TAURI_SIGNING_PRIVATE_KEY, or ~/.tauri/wolfgang.key
# plus wolfgang.key.password) it also builds Wolfgang.app.tar.gz + .sig and latest.json
# for GitHub Releases. Without the key the app is built without update files.

set -euo pipefail
cd "$(dirname "$0")/.."

step() { echo; echo "▶ $1"; }

step "Pixel art (assets/pixel) aktuell?"
cargo run --quiet --release -p wolfgang-art -- build >/dev/null

step "Kommandozeile bauen (kommt mit in die App)"
cargo build --release -p wolfgang-cli

KEY="$HOME/.tauri/wolfgang.key"
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -f "$KEY" ]]; then
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$KEY.password" 2>/dev/null)"
fi
UPDATER=()
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "(kein Signierschlüssel – baue ohne Update-Dateien)"
  UPDATER=(--config '{"bundle":{"createUpdaterArtifacts":false}}')
fi

step "App + DMG bauen"
(cd crates/wolfgang-app && cargo tauri build --bundles app,dmg ${UPDATER[@]+"${UPDATER[@]}"})

APP="target/release/bundle/macos/Wolfgang.app"
step "Prüfen"
[[ -x "$APP/Contents/MacOS/wolfgang" ]] || { echo "Kommandozeile fehlt in der App"; exit 1; }
"$APP/Contents/MacOS/wolfgang" --version
codesign --verify --deep --strict "$APP" && echo "Signatur (ad-hoc) ok"
/usr/libexec/PlistBuddy -c "Print :CFBundleDocumentTypes:0:LSHandlerRank" "$APP/Contents/Info.plist" | grep -q Alternate && echo "Dock-Ablage ok"

MACOS="target/release/bundle/macos"
if [[ -f "$MACOS/Wolfgang.app.tar.gz.sig" ]]; then
  step "Update-Datei latest.json"
  VERSION=$(python3 -c "import json; print(json.load(open('crates/wolfgang-app/tauri.conf.json'))['version'])")
  python3 - "$VERSION" "$MACOS" <<'PY'
import json, sys, datetime
version, macos = sys.argv[1], sys.argv[2]
repo = "https://github.com/k1tesurfen/wolfgang/releases/download"
json.dump({
    "version": version,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {"darwin-aarch64": {
        "signature": open(f"{macos}/Wolfgang.app.tar.gz.sig").read().strip(),
        "url": f"{repo}/v{version}/Wolfgang.app.tar.gz",
    }},
}, open(f"{macos}/latest.json", "w"), indent=2)
PY
  echo "ok ($MACOS/latest.json)"
fi

echo
ls -lh target/release/bundle/dmg/*.dmg $MACOS/*.tar.gz 2>/dev/null
