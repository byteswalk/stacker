#!/bin/bash
# Builds Stacker for macOS on a Mac: Stacker.app, the .dmg people download, and the zipped
# app the in-app updater installs, with their SHA-256 sums.
#
#   scripts/build-mac.sh [aarch64-apple-darwin|x86_64-apple-darwin] [revision]
#
# The revision (r12) is shown beside the version in the app.
#
# With STACKER_SIGN=stdin the signing certificate arrives on stdin as two lines (base64
# PKCS#12, password; see read-mac-signing-cert.ps1 on the Windows release machine). It goes
# into a throwaway keychain deleted when the build ends, and the app is signed with it, so
# every version is the same program to macOS. Without it the app is signed ad hoc, which is
# enough to test but not to hand out.
set -euo pipefail

TARGET=${1:-aarch64-apple-darwin}
REVISION=${2:-}
case "$TARGET" in
  aarch64-apple-darwin) CHIP=arm64 ;;
  x86_64-apple-darwin) CHIP=x64 ;;
  *) echo "unknown target: $TARGET" >&2; exit 2 ;;
esac

cd "$(dirname "$0")/.."
VERSION=$(node -p "require('./package.json').version")
OUT="release/macos/v$VERSION"
IDENTITY="-"
WORK=$(mktemp -d)

if [ "${STACKER_SIGN:-}" = "stdin" ]; then
  read -r P12
  read -r PASSWORD
  KEYCHAIN="$WORK/stacker-signing.keychain-db"
  KEYCHAIN_PASSWORD="$(uuidgen)"
  ORIGINAL_KEYCHAINS=()
  while IFS= read -r line; do
    ORIGINAL_KEYCHAINS+=("$(echo "$line" | tr -d ' "')")
  done < <(security list-keychains -d user)
  cleanup() {
    security list-keychains -d user -s "${ORIGINAL_KEYCHAINS[@]}" || true
    security delete-keychain "$KEYCHAIN" 2>/dev/null || true
    rm -rf "$WORK"
  }
  security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
  security set-keychain-settings "$KEYCHAIN"
  security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
  echo "$P12" | base64 -D > "$WORK/id.p12"
  security import "$WORK/id.p12" -k "$KEYCHAIN" -P "$PASSWORD" -T /usr/bin/codesign >/dev/null
  rm -f "$WORK/id.p12"
  unset P12 PASSWORD
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
  # codesign looks the identity up in the user's search list.
  security list-keychains -d user -s "$KEYCHAIN" "${ORIGINAL_KEYCHAINS[@]}"
  # Self-signed, so not "valid" to find-identity -v: taken by its hash.
  IDENTITY="$(security find-identity -p codesigning "$KEYCHAIN" | awk '/"Stacker Code Signing"/ { print $2; exit }')"
  [ -n "$IDENTITY" ] || { echo "the signing certificate did not import" >&2; exit 1; }
  echo "Signing with Stacker Code Signing ($IDENTITY)"
else
  cleanup() { rm -rf "$WORK"; }
  echo "No signing certificate: signing ad hoc"
fi
trap cleanup EXIT

npm ci --no-audit --no-fund
npm run ext:build
VITE_STACKER_REVISION="$REVISION" npx tauri build --target "$TARGET" \
  --config src-tauri/tauri.bundle.conf.json

APP="src-tauri/target/$TARGET/release/bundle/macos/Stacker.app"
[ -d "$APP" ] || { echo "no app was built at $APP" >&2; exit 1; }
codesign --force --deep --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict "$APP"
echo "Designated requirement: $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p')"

mkdir -p "$OUT"
DMG="$OUT/Stacker-$VERSION-macos-$CHIP.dmg"
ZIP="$OUT/Stacker-$VERSION-update-macos-$CHIP.zip"
rm -f "$DMG" "$ZIP"

# The disk image holds the app and a link to Applications to drag it onto. It is left
# unsigned: a signed image that is not notarized is blocked once more on its own.
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Stacker" -srcfolder "$STAGE" -ov -format UDZO "$DMG"

# What the updater downloads: the app as `ditto` packs it, so its signature survives.
ditto -c -k --keepParent "$APP" "$ZIP"

(cd "$OUT" && shasum -a 256 "$(basename "$DMG")" "$(basename "$ZIP")" > "SHA256SUMS-macos-$CHIP.txt")
echo "built $DMG"
echo "built $ZIP"
