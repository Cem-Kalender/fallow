#!/bin/sh
# 016 (research M-19, M-20, data-model "App bundle", "Release files", T032): Fallow.app from the
# release build, signed once after its last change, and the disk image with its SHA-256:
#
#   cargo build --release --locked
#   sh installer/macos/make-app.sh <version> <out-dir>
#
# Writes <out-dir>/Fallow.app, <out-dir>/Fallow-<version>-macos-arm64.dmg and its .sha256.
#
# Signing: ad hoc by default (no Apple membership); with FALLOW_SIGN_ID set to a Developer ID
# Application identity, signed with it and a secure timestamp, ready for notarization (FR-024).
# The hardened runtime is on either way: notarization asks for it.
set -eu

version=${1:?usage: make-app.sh <version> <out-dir>}
out=${2:?usage: make-app.sh <version> <out-dir>}
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=$root/target/release/fallow
sign_id=${FALLOW_SIGN_ID:--}

[ -x "$bin" ] || { echo "no release build at $bin: run cargo build --release --locked" >&2; exit 1; }
case $version in
    *[!0-9A-Za-z.+-]* | "") echo "not a version: $version" >&2; exit 1 ;;
esac

mkdir -p "$out"
app=$out/Fallow.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/@VERSION@/$version/g" "$root/installer/macos/Info.plist.in" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
cp "$bin" "$app/Contents/MacOS/fallow"
cp "$root/assets/icon/Fallow.icns" "$app/Contents/Resources/Fallow.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"
# Nothing of the tests goes in (data-model "App bundle": not shipped).
for f in fallow-fake-agent fallow-fake-host; do
    [ ! -e "$app/Contents/MacOS/$f" ] || { echo "$f must not be in the app" >&2; exit 1; }
done

# Signed once, last (research M-19): a change after this would make macOS call the app damaged.
if [ "$sign_id" = "-" ]; then
    timestamp=--timestamp=none
else
    timestamp=--timestamp
fi
codesign --force --deep --options runtime "$timestamp" \
    --entitlements "$root/installer/macos/entitlements.plist" \
    --sign "$sign_id" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
if ! codesign -dv "$app" 2>&1 | grep -q 'flags=.*runtime'; then
    echo "the app is not signed with the hardened runtime" >&2
    exit 1
fi
if ! codesign -dv "$app" 2>&1 | grep -q '^Identifier=dev.fallow.Fallow$'; then
    echo "the app's signature does not carry the identifier dev.fallow.Fallow" >&2
    exit 1
fi

# The disk image: the app and a link to Applications (the usual Mac layout), compressed.
dmg=$out/Fallow-$version-macos-arm64.dmg
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
ditto "$app" "$stage/Fallow.app"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -quiet -format UDZO -fs APFS -volname Fallow -srcfolder "$stage" "$dmg"
(cd "$out" && shasum -a 256 "$(basename "$dmg")" > "$(basename "$dmg").sha256")
cat "$dmg.sha256"
