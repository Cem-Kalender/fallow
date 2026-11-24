#!/bin/sh
# 016 (FR-014, research M-9, T044): on this macOS version, an app signed ad hoc gets provisional
# notification authorization without a prompt and delivers a notification. The notification
# spike's app (main.swift, Info.plist), built into ~/Applications (LaunchServices registers apps
# there), opened with `provisional`; fails unless granted=true and delivered >= 1.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
app=$HOME/Applications/FallowNotifyCheck.app
trap 'rm -rf "$work" "$app"' EXIT
mkdir -p "$work/FallowNotifyCheck.app/Contents/MacOS" "$HOME/Applications"
cp "$here/Info.plist" "$work/FallowNotifyCheck.app/Contents/"
swiftc -O "$here/main.swift" -o "$work/FallowNotifyCheck.app/Contents/MacOS/spike"
codesign --force --deep -s - "$work/FallowNotifyCheck.app"
rm -rf "$app"
cp -R "$work/FallowNotifyCheck.app" "$app"
# The app writes into an existing file only (FileHandle(forWritingAtPath:)).
: > "$work/result.txt"
open -W "$app" --args "$work/result.txt" provisional
cat "$work/result.txt"
grep -q '^granted=true' "$work/result.txt" || { echo "notify-check: not granted on $(sw_vers -productVersion)"; exit 1; }
n=$(sed -n 's/^delivered=//p' "$work/result.txt" | tail -1)
[ "${n:-0}" -ge 1 ] || { echo "notify-check: nothing delivered on $(sw_vers -productVersion)"; exit 1; }
echo "notify-check: provisional notifications work on macOS $(sw_vers -productVersion)"
