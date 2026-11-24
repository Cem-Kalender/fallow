#!/bin/sh
# 016 (research M-3, T002): Fallow.icns for the Mac app from fallow.ico (its largest image, 256 px,
# scaled to each size macOS asks for), and the menu bar template images (make_menubar.swift).
# Run on a Mac after changing fallow.ico or the logo:
#
#   sh assets/icon/make_icns.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sips -s format png "$here/fallow.ico" --out "$tmp/source.png" >/dev/null
set=$tmp/Fallow.iconset
mkdir "$set"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$tmp/source.png" --out "$set/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$tmp/source.png" --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o "$here/Fallow.icns"
swiftc -O "$here/make_menubar.swift" -o "$tmp/make_menubar"
"$tmp/make_menubar" "$here"
echo "wrote $here/Fallow.icns and the menu bar images"
