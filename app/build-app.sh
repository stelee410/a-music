#!/bin/bash
# Build "Agent Groovebox.app": Rust engine (agb) + SwiftUI app, ad-hoc signed.
# Usage: app/build-app.sh            → dist/Agent Groovebox.app
#        app/build-app.sh --install  → also copy to /Applications
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
DIST="$REPO/dist"
APP="$DIST/Agent Groovebox.app"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> building engine (cargo, release)"
(cd "$REPO" && cargo build --release -p agb-cli)

echo "==> building app (swift, release)"
(cd "$HERE" && swift build -c release)
BIN="$(cd "$HERE" && swift build -c release --show-bin-path)/AgentGroovebox"

echo "==> icon"
swift "$HERE/scripts/make-icon.swift" "$WORK/icon.png"
mkdir -p "$WORK/AppIcon.iconset"
for s in 16 32 128 256 512; do
  sips -z $s $s "$WORK/icon.png" --out "$WORK/AppIcon.iconset/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) "$WORK/icon.png" --out "$WORK/AppIcon.iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$WORK/AppIcon.iconset" -o "$WORK/AppIcon.icns"

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/AgentGroovebox"
cp "$REPO/target/release/agb" "$APP/Contents/Resources/agb"
cp "$WORK/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
VERSION="$(grep -m1 '^version' "$REPO/Cargo.toml" | cut -d'"' -f2)"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Agent Groovebox</string>
  <key>CFBundleDisplayName</key><string>Agent Groovebox</string>
  <key>CFBundleIdentifier</key><string>app.agent-groovebox</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>AgentGroovebox</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT License</string>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$APP"
echo "==> done: $APP"
if [[ "${1:-}" == "--install" ]]; then
  rm -rf "/Applications/Agent Groovebox.app"
  cp -R "$APP" /Applications/
  echo "==> installed to /Applications/Agent Groovebox.app"
fi
