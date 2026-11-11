#!/bin/bash
# Releases from this Mac: tests, builds every released character (characters.json,
# "release": true — Wolfgang, Ully, Burgunde), tags and uploads it all to GitHub as
# one release.
#
#   scripts/release.sh             # creates a *draft* release to check first
#   scripts/release.sh --publish   # publishes right away (installed Wolfgangs see it)
#
# Before: on main, bump the version in Cargo.toml (workspace) and
# crates/wolfgang-app/tauri.conf.json, commit. Needs: everything for scripts/build-app.sh, the update key (~/.tauri/wolfgang.key),
# and the GitHub CLI logged in (gh auth login).

set -euo pipefail
cd "$(dirname "$0")/.."

PUBLISH=false
[[ "${1:-}" == "--publish" ]] && PUBLISH=true

step() { echo; echo "▶ $1"; }
fail() { echo "✗ $1" >&2; exit 1; }

VERSION=$(python3 -c "import json; print(json.load(open('crates/wolfgang-app/tauri.conf.json'))['version'])")
TAG="v$VERSION"
CARGO_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

step "Prüfen, ob alles bereit ist ($TAG)"
[[ "$CARGO_VERSION" == "$VERSION" ]] || fail "Cargo.toml sagt $CARGO_VERSION, tauri.conf.json sagt $VERSION"
[[ "$(git branch --show-current)" == main ]] || fail "Releases kommen nur von main (gerade: $(git branch --show-current))"
git diff --quiet && git diff --cached --quiet || fail "Es gibt noch nicht committete Änderungen"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && fail "Tag $TAG gibt es schon"
git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1 && fail "Tag $TAG gibt es auf GitHub schon"
[[ -f "$HOME/.tauri/wolfgang.key" || -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] || fail "Update-Schlüssel fehlt (~/.tauri/wolfgang.key)"
gh auth status >/dev/null 2>&1 || fail "GitHub CLI ist nicht angemeldet (gh auth login)"

step "Tests"
cargo test --workspace --quiet

DMG_DIR=target/release/bundle/dmg
MACOS=target/release/bundle/macos
# Tauri empties the dmg folder on every build, so each character's files are
# copied here right after its build (else only the last character's dmgs survive).
UPLOAD=target/release/upload
rm -rf "$UPLOAD" && mkdir -p "$UPLOAD"
FILES=()
NAMES=()
# key name feed of every character that's part of releases
while read -r KEY NAME FEED; do
  step "Bauen: $NAME"
  scripts/build-app.sh "$KEY"
  CHARACTER_FILES=("$DMG_DIR/${NAME}_${VERSION}_aarch64.dmg" "$DMG_DIR/$NAME.dmg" "$MACOS/$NAME.app.tar.gz" "$MACOS/$FEED")
  for f in "${CHARACTER_FILES[@]}"; do [[ -f "$f" ]] || fail "$f fehlt"; done
  grep -q "\"version\": \"$VERSION\"" "$MACOS/$FEED" || fail "$FEED passt nicht zu $VERSION"
  cp "${CHARACTER_FILES[@]}" "$UPLOAD/"
  for f in "${CHARACTER_FILES[@]}"; do FILES+=("$UPLOAD/$(basename "$f")"); done
  NAMES+=("$NAME")
done < <(python3 -c "import json; [print(c['key'], c['name'], c['feed']) for c in json.load(open('characters.json')) if c['release']]")
[[ ${#NAMES[@]} -gt 0 ]] || fail "In characters.json ist keine Figur für Releases markiert"
echo "Im Release: ${NAMES[*]}"

step "Hochladen"
# The commit goes to main, then the tag.
git push origin main
git tag "$TAG"
git push origin "$TAG"
NOTES=$(mktemp)
sed "s/{{VERSION}}/$TAG/" .github/release-body.md > "$NOTES"
ARGS=(--title "Wolfgang $TAG" --notes-file "$NOTES" --verify-tag)
$PUBLISH && ARGS+=(--latest) || ARGS+=(--draft)
gh release create "$TAG" "${FILES[@]}" "${ARGS[@]}"
rm -f "$NOTES"

echo
if $PUBLISH; then
  echo "✓ $TAG ist veröffentlicht (${NAMES[*]}). Installierte Apps bieten das Update beim nächsten Start an."
else
  echo "✓ $TAG liegt als Entwurf auf GitHub. Veröffentlichen: gh release edit $TAG --draft=false --latest"
fi
