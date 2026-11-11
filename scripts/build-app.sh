#!/bin/bash
# Builds one character's app and .dmg for macOS (Apple Silicon): Wolfgang, Ully or Burgunde.
#
#   scripts/build-app.sh              # Wolfgang
#   scripts/build-app.sh ully         # Ully (any key from characters.json)
#
# Needs: Rust, Tauri CLI (cargo install tauri-cli --version "^2" --locked) and the
# build tools for the codecs: brew install cmake nasm pkg-config autoconf automake libtool
# Result: target/release/bundle/macos/<Name>.app and target/release/bundle/dmg/<Name>*.dmg
#
# One source, three apps: the character decides the app's name, bundle identifier and
# update feed (characters.json), and WOLFGANG_CHARACTER tells the code who it is
# (the app and the `wolfgang` command inside it).
#
# Self-update: with the signing key (TAURI_SIGNING_PRIVATE_KEY, or ~/.tauri/wolfgang.key
# plus wolfgang.key.password) it also builds <Name>.app.tar.gz + .sig and the character's
# update feed (latest.json for Wolfgang, latest-ully.json, …) for GitHub Releases.
# Without the key the app is built without update files.

set -euo pipefail
cd "$(dirname "$0")/.."

step() { echo; echo "▶ $1"; }

CHARACTER="${1:-wolfgang}"
# name, identifier, feed of that character (fails for an unknown key)
read -r NAME IDENTIFIER FEED < <(python3 - "$CHARACTER" <<'PY'
import json, sys
c = {c["key"]: c for c in json.load(open("characters.json"))}.get(sys.argv[1])
if not c:
    sys.exit(f"Unbekannte Figur „{sys.argv[1]}“ (siehe characters.json)")
print(c["name"], c["identifier"], c["feed"])
PY
)
export WOLFGANG_CHARACTER="$CHARACTER"
echo "Figur: $NAME ($IDENTIFIER)"

step "Pixel art (assets/pixel) aktuell?"
cargo run --quiet --release -p wolfgang-art -- build >/dev/null

step "ffmpeg für Videos und Audio (wird nur gebaut, wenn sich was geändert hat)"
scripts/build-ffmpeg.sh

step "Kommandozeile bauen (kommt mit in die App)"
cargo build --release -p wolfgang-cli

KEY="$HOME/.tauri/wolfgang.key"
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -f "$KEY" ]]; then
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$KEY.password" 2>/dev/null)"
fi
UPDATER=true
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "(kein Signierschlüssel – baue ohne Update-Dateien)"
  UPDATER=false
fi
# The character's name, identifier and update feed on top of tauri.conf.json.
CONFIG=$(python3 - "$NAME" "$IDENTIFIER" "$FEED" "$UPDATER" <<'PY'
import json, sys
name, identifier, feed, updater = sys.argv[1:]
print(json.dumps({
    "productName": name,
    "identifier": identifier,
    "bundle": {"createUpdaterArtifacts": updater == "true"},
    "plugins": {"updater": {"endpoints": [f"https://github.com/k1tesurfen/wolfgang/releases/latest/download/{feed}"]}},
}))
PY
)

step "App + DMG bauen"
# CI=true makes Tauri skip the AppleScript that opens the new disk image in Finder
# to arrange its icons (a Finder window popping up and pulling you to another
# Space on every build). The dmg still has the app and the Programme link.
(cd crates/wolfgang-app && CI=true cargo tauri build --bundles app,dmg --config "$CONFIG")

MACOS="target/release/bundle/macos"
APP="$MACOS/$NAME.app"
step "Prüfen"
[[ -x "$APP/Contents/MacOS/wolfgang" ]] || { echo "Kommandozeile fehlt in der App"; exit 1; }
"$APP/Contents/MacOS/wolfgang" --version
HELP=$("$APP/Contents/MacOS/wolfgang" --help)
[[ "$HELP" == "$NAME "* ]] || { echo "Die Kommandozeile hält sich nicht für $NAME"; exit 1; }
echo "Kommandozeile ist $NAME"
for tool in ffmpeg ffprobe; do
  [[ -x "$APP/Contents/MacOS/$tool" ]] || { echo "$tool fehlt in der App"; exit 1; }
done
"$APP/Contents/MacOS/ffmpeg" -hide_banner -L | grep -q "Lesser General Public" && echo "ffmpeg (LGPL) ok"
codesign --verify --deep --strict "$APP" && echo "Signatur (ad-hoc) ok"
/usr/libexec/PlistBuddy -c "Print :CFBundleDocumentTypes:0:LSHandlerRank" "$APP/Contents/Info.plist" | grep -q Alternate && echo "Dock-Ablage ok"
/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$APP/Contents/Info.plist" | grep -qx "$IDENTIFIER" && echo "Kennung $IDENTIFIER ok"

# A copy with a fixed name, so the README's download button
# (releases/latest/download/<Name>.dmg) always gets the newest version.
APP_VERSION=$(python3 -c "import json; print(json.load(open('crates/wolfgang-app/tauri.conf.json'))['version'])")
cp "target/release/bundle/dmg/${NAME}_${APP_VERSION}_aarch64.dmg" "target/release/bundle/dmg/$NAME.dmg"

if $UPDATER; then
  step "Update-Datei $FEED"
  python3 - "$APP_VERSION" "$MACOS" "$NAME" "$FEED" <<'PY'
import json, sys, datetime
version, macos, name, feed = sys.argv[1:]
repo = "https://github.com/k1tesurfen/wolfgang/releases/download"
json.dump({
    "version": version,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {"darwin-aarch64": {
        "signature": open(f"{macos}/{name}.app.tar.gz.sig").read().strip(),
        "url": f"{repo}/v{version}/{name}.app.tar.gz",
    }},
}, open(f"{macos}/{feed}", "w"), indent=2)
PY
  echo "ok ($MACOS/$FEED)"
fi

echo
ls -lh "target/release/bundle/dmg/$NAME"*.dmg "$MACOS/$NAME.app.tar.gz" 2>/dev/null
