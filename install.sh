#!/bin/bash
# Baut Wolfgang 2 aus diesem Ordner und installiert den Befehl `wolfgang`.
#
#   ./install.sh
#
# Braucht beim Bauen: Homebrew (für cmake, nasm & Co.) und Rust. Fehlt etwas,
# wird es nach Rückfrage installiert. Zum *Benutzen* braucht Wolfgang nichts.
# (Die alte Bash-Version liegt in legacy/.)

set -euo pipefail
cd "$(dirname "$0")"

TARGET_DIR="/usr/local/bin"
# ffmpeg für Videos/Audio liegt bewusst nicht im PATH, damit es kein anderes ffmpeg verdeckt.
TOOLS_DIR="/usr/local/libexec/wolfgang"
say() { echo "wolfgang-installer: $1"; }
fail() { echo "wolfgang-installer: FEHLER: $1" >&2; exit 1; }
ask() { read -r -p "wolfgang-installer: $1 [J/n] " a; [[ -z "$a" || "$a" =~ ^[JjYy] ]]; }

[[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]] || fail "Wolfgang 2 läuft bisher nur auf Macs mit Apple-Chip."

# --- Homebrew + Bauwerkzeuge -------------------------------------------------
if ! command -v brew &>/dev/null; then
  [[ -x /opt/homebrew/bin/brew ]] && eval "$(/opt/homebrew/bin/brew shellenv)"
fi
if ! command -v brew &>/dev/null; then
  ask "Homebrew fehlt. Jetzt installieren?" || fail "Ohne Homebrew kann ich nicht bauen."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  eval "$(/opt/homebrew/bin/brew shellenv)"
fi
for tool in cmake nasm pkg-config autoconf automake glibtoolize; do
  if ! command -v "$tool" &>/dev/null; then
    say "Installiere $tool (nur zum Bauen) …"
    brew install "$([[ $tool == glibtoolize ]] && echo libtool || echo "$tool")"
  fi
done

# --- Rust ----------------------------------------------------------------------
if ! command -v cargo &>/dev/null; then
  [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
fi
if ! command -v cargo &>/dev/null; then
  ask "Rust fehlt. Jetzt über rustup installieren?" || fail "Ohne Rust kann ich nicht bauen."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
fi

# --- Bauen ---------------------------------------------------------------------
say "Baue Wolfgang (das dauert beim ersten Mal eine Viertelstunde) …"
scripts/build-ffmpeg.sh
cargo build --release -p wolfgang-cli
BIN="target/release/wolfgang"
[[ -x "$BIN" ]] || fail "Bauen hat nicht geklappt."

# --- Installieren ----------------------------------------------------------------
if [[ ! -d "$TARGET_DIR" ]]; then
  say "$TARGET_DIR fehlt, lege es an (evtl. fragt macOS nach deinem Passwort) …"
  sudo mkdir -p "$TARGET_DIR"
fi
SUDO=()
if [[ ! -w "$TARGET_DIR" ]] || ! mkdir -p "$TOOLS_DIR" 2>/dev/null || [[ ! -w "$TOOLS_DIR" ]]; then
  say "Kopiere nach $TARGET_DIR (evtl. fragt macOS nach deinem Passwort) …"
  SUDO=(sudo)
fi
${SUDO[@]+"${SUDO[@]}"} install -m 755 "$BIN" "$TARGET_DIR/wolfgang"
${SUDO[@]+"${SUDO[@]}"} install -d "$TOOLS_DIR"
${SUDO[@]+"${SUDO[@]}"} install -m 755 target/ffmpeg/bin/ffmpeg target/ffmpeg/bin/ffprobe "$TOOLS_DIR/"

echo ""
echo "  Fertig! Wolfgang $("$TARGET_DIR/wolfgang" --version | awk '{print $2}') ist installiert."
echo ""
echo "  Probier's aus:   wolfgang ~/Pictures/irgendein-ordner"
echo "  Hilfe:           wolfgang --help"
if ! command -v wolfgang &>/dev/null; then
  echo ""
  echo "  Hinweis: $TARGET_DIR ist nicht in deinem PATH. Öffne ein neues Terminal-Fenster"
  echo "  oder füge  export PATH=\"$TARGET_DIR:\$PATH\"  zu ~/.zshrc hinzu."
fi
echo ""
