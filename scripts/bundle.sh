#!/usr/bin/env bash
# Builds "build/BTC Ticker.app" from the release build: Info.plist, icon from assets/icon.svg, ad-hoc signature.
set -euo pipefail
cd "$(dirname "$0")/.."

scripts/build.sh -c release -Xswiftc -warnings-as-errors
bin="$(swift build -c release --show-bin-path)"
app="build/BTC Ticker.app"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/BTCTicker" "$app/Contents/MacOS/BTCTicker"
cp Support/Info.plist "$app/Contents/Info.plist"

# Quick Look is the only SVG renderer that ships with macOS.
qlmanage -t -s 1024 -o "$work" assets/icon.svg >/dev/null
iconset="$work/AppIcon.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
	sips -z "$size" "$size" "$work/icon.svg.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
	sips -z $((size * 2)) $((size * 2)) "$work/icon.svg.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

codesign --force --sign - --timestamp=none "$app"
echo "built $app"
