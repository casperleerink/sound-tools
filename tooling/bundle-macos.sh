#!/usr/bin/env bash
# Builds Sound Tools.app from the runtime binary.
#
#   tooling/bundle-macos.sh [--zip] [output-folder]
#
# The app goes to dist/ by default. --zip also writes Sound-Tools-<version>-macos-<arch>.zip
# next to it, for a release. The app is signed ad hoc, which is enough to run it on this Mac.
# It asks for nothing, so CI can run it.
set -euo pipefail

zip=false
output=""
for argument in "$@"; do
  case "$argument" in
    --zip) zip=true ;;
    -*) echo "usage: tooling/bundle-macos.sh [--zip] [output-folder]" >&2; exit 2 ;;
    *) output="$argument" ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
output="${output:-dist}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"

version="$(tooling/version.sh)"

cargo build --release -p runtime --locked
# .cargo/config.toml moves the build output out of the repository, so ask cargo where it is.
target="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
binary="$target/release/runtime"

app="$output/Sound Tools.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/sound-tools"

# The icon: every size macOS asks for, from the one PNG in the repository.
iconset="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" tooling/icon/sound-tools.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" tooling/icon/sound-tools.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$iconset")"

# macOS 11 is the oldest a build for Apple silicon runs on, and GPUI asks for less.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Sound Tools</string>
	<key>CFBundleExecutable</key>
	<string>sound-tools</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>CFBundleIdentifier</key>
	<string>com.casperleerink.sound-tools</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Sound Tools</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$version</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.music</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSMicrophoneUsageDescription</key>
	<string>Sound Tools records audio from the default input of macOS onto the tracks you arm.</string>
</dict>
</plist>
PLIST
plutil -lint "$app/Contents/Info.plist" >/dev/null

# Ad hoc: no certificate, no hardened runtime. The entitlements let it record and load the
# CLAP and VST 3 plugins of other makers.
codesign --force --deep --sign - --entitlements tooling/macos/entitlements.plist "$app"
codesign --verify --deep --strict "$app"
"$app/Contents/MacOS/sound-tools" --version
echo "built $app ($version)"

if [[ "$zip" == true ]]; then
  # arm64 on Apple silicon. A release has no Intel build.
  archive="$output/Sound-Tools-$version-macos-$(uname -m).zip"
  rm -f "$archive"
  ditto -c -k --keepParent "$app" "$archive"
  echo "built $archive"
fi
