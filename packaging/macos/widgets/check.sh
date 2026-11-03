#!/usr/bin/env bash
# Checks that the extension still reads what the app writes: a Rust test writes a snapshot, and a
# small program built from WidgetViews.swift decodes it and checks a few fields.
#
#   packaging/macos/widgets/check.sh
set -euo pipefail
[[ "$(uname -s)" == "Darwin" ]] || { echo "macOS only"; exit 0; }

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

(cd "$ROOT" && WMPASS_WIDGET_FIXTURE="$TMP/snapshot.json" cargo test -q -p wmpass-desktop the_snapshot_keeps_its_field_names >/dev/null)

cat > "$TMP/Check.swift" <<'SWIFT'
import Foundation

@main
struct Check {
    static func main() throws {
        let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let s = try decoder.decode(Snapshot.self, from: data)
        precondition(s.state == "open" && s.access, "state")
        precondition(s.codes.first?.steps.isEmpty == false, "code steps")
        precondition(s.favorites.first?.title == "Bank", "favorites")
        precondition(s.open(at: Date(timeIntervalSince1970: 0)), "open before hide_after")
        print("the extension reads the app's snapshot")
    }
}
SWIFT
swiftc -swift-version 5 -parse-as-library -sdk "$(xcrun --sdk macosx --show-sdk-path)" -target "$(uname -m)-apple-macos14.0" \
  -o "$TMP/check" "$HERE/WidgetViews.swift" "$TMP/Check.swift"
"$TMP/check" "$TMP/snapshot.json"
