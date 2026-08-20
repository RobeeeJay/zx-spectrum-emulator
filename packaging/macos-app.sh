#!/usr/bin/env bash
#
# Build ZX-Rustrum.app, and a .dmg holding it.
#
# The app is unsigned: macOS will refuse to open it on another machine until
# it is signed with a Developer ID and notarised. To do that, set
# MACOS_SIGN_IDENTITY to the identity name and this script will sign it; the
# notarisation step (xcrun notarytool) needs an Apple account and is left to
# whoever is doing the release.
#
# Usage: packaging/macos-app.sh [target-triple]
#        packaging/macos-app.sh --dmg-name [target-triple]   (print it and stop)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
name_only=""
if [ "${1:-}" = "--dmg-name" ]; then
    name_only=yes
    shift
fi
target="${1:-}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"

# The disk image carries the version and the architecture. Both macOS builds
# are made in the same workflow and their files end up in one directory: two
# images called the same thing would be one image, and whichever was uploaded
# second would be the release.
case "$target" in
    aarch64-*) arch=arm64 ;;
    x86_64-*)  arch=x86_64 ;;
    "")        arch="$(uname -m)" ;;
    *)         arch="${target%%-*}" ;;
esac
dmg="target/packaging/ZX-Rustrum-$version-macos-$arch.dmg"
if [ -n "$name_only" ]; then
    echo "$dmg"
    exit 0
fi

cd "$root"
if [ -n "$target" ]; then
    cargo build --release --target "$target"
    binary="target/$target/release/zx-rustrum"
else
    cargo build --release
    binary="target/release/zx-rustrum"
fi

app="target/packaging/ZX-Rustrum.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/roms"
cp "$binary" "$app/Contents/MacOS/ZX-Rustrum"

# The icon: macOS wants an .icns, which iconutil builds from a directory of
# PNGs at fixed sizes.
iconset="target/packaging/icon.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size "$here/icon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$here/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/ZX-Rustrum.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>ZX-Rustrum</string>
    <key>CFBundleDisplayName</key><string>ZX-Rustrum</string>
    <key>CFBundleExecutable</key><string>ZX-Rustrum</string>
    <key>CFBundleIdentifier</key><string>uk.co.example.zx-rustrum</string>
    <key>CFBundleIconFile</key><string>ZX-Rustrum</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Tape, snapshot or ROM image</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSItemContentTypes</key><array><string>public.data</string></array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>tzx</string><string>tap</string>
                <string>p</string><string>81</string><string>p81</string>
                <string>sna</string><string>z80</string>
                <string>rom</string><string>bin</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
PLIST

# ROMs are copyrighted and are not shipped; the directory tells the user where
# theirs should go, and the app searches it (see src/resources.rs).
cat > "$app/Contents/Resources/roms/README.txt" <<'TXT'
Put ROM images here:

  48.rom     16K   ZX Spectrum 48K
  128.rom    32K   ZX Spectrum 128K
  plus3.rom  64K   ZX Spectrum +2A/+3
  zx81.rom    8K   ZX81

None are included: they are still under copyright. The emulator also looks
in ~/Library/Application Support/ZX Spectrum Emulator, which survives an
upgrade of the app, and in whichever directory you last opened a ROM from.
TXT

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
    codesign --force --deep --options runtime --timestamp \
        --sign "$MACOS_SIGN_IDENTITY" "$app"
    codesign --verify --strict --verbose=2 "$app"
else
    echo "note: unsigned. Set MACOS_SIGN_IDENTITY to sign, then notarise." >&2
fi

rm -f "$dmg"
staging="target/packaging/dmg"
rm -rf "$staging"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "ZX-Rustrum" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null

echo "built $app"
echo "built $dmg"
