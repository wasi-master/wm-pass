#!/usr/bin/env bash
# Builds the widget extension and its WidgetCenter library for the desktop app's bundle.
#
#   packaging/macos/widgets/build.sh [out dir]
#
# Tauri runs it as build.beforeBundleCommand (desktop/src-tauri/tauri.conf.json), which copies the
# results into WM Pass.app through bundle.macOS.files. No Xcode project: swiftc from the Command
# Line Tools is enough. Off macOS it does nothing.
#
# The architectures follow Tauri's target (TAURI_ENV_TARGET_TRIPLE), both for a universal build,
# and default to this Mac's own. Signing is ad hoc, like the app: the extension must carry the
# sandbox entitlement or pluginkit silently drops it.
set -euo pipefail

[[ "$(uname -s)" == "Darwin" ]] || exit 0

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
OUT="${1:-$ROOT/desktop/src-tauri/widgets-out}"
IDENTITY="${WMPASS_SIGN_IDENTITY:--}"
VERSION="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$ROOT/desktop/src-tauri/tauri.conf.json" | head -1)"
BUILD="${WMPASS_BUILD_NUMBER:-$VERSION}"

case "${TAURI_ENV_TARGET_TRIPLE:-}" in
  universal-apple-darwin) archs=(arm64 x86_64) ;;
  x86_64-apple-darwin) archs=(x86_64) ;;
  aarch64-apple-darwin) archs=(arm64) ;;
  *) archs=("$(uname -m)") ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
SDK="$(xcrun --sdk macosx --show-sdk-path)"
APPEX="$OUT/WMPassWidgets.appex"
rm -rf "$APPEX" "$OUT/libWMPassWidgetCenter.dylib"
mkdir -p "$APPEX/Contents/MacOS"

widgets=(); shims=()
for arch in "${archs[@]}"; do
  swiftc -O -swift-version 5 -parse-as-library -application-extension -sdk "$SDK" \
    -target "$arch-apple-macos14.0" \
    -Xlinker -e -Xlinker _NSExtensionMain \
    -o "$TMP/widgets-$arch" \
    "$HERE/WidgetViews.swift" "$HERE/WMPassWidgets.swift"
  swiftc -O -swift-version 5 -parse-as-library -emit-library -sdk "$SDK" \
    -target "$arch-apple-macos14.0" \
    -Xlinker -install_name -Xlinker @rpath/libWMPassWidgetCenter.dylib \
    -o "$TMP/shim-$arch.dylib" \
    "$HERE/WidgetCenterShim.swift"
  widgets+=("$TMP/widgets-$arch"); shims+=("$TMP/shim-$arch.dylib")
done
lipo -create "${widgets[@]}" -output "$APPEX/Contents/MacOS/WMPassWidgets"
lipo -create "${shims[@]}" -output "$OUT/libWMPassWidgetCenter.dylib"

sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" "$HERE/Info.plist" > "$APPEX/Contents/Info.plist"

codesign --force --sign "$IDENTITY" --timestamp=none "$OUT/libWMPassWidgetCenter.dylib" >/dev/null
codesign --force --sign "$IDENTITY" --timestamp=none --entitlements "$HERE/widgets.entitlements" "$APPEX" >/dev/null
echo "widgets: $APPEX (${archs[*]})"
