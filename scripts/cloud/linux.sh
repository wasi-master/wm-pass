#!/bin/bash
# The Linux checks, in the order CI runs them. Each step stands alone:
#   scripts/cloud/linux.sh rust      cargo test, clippy, deny over the workspace
#   scripts/cloud/linux.sh desktop   clippy and tests for the desktop crates
#   scripts/cloud/linux.sh selftest  debug app on a virtual display, real flows, prints the results
#   scripts/cloud/linux.sh bundle    release .deb, .rpm and .AppImage, unsigned
#   scripts/cloud/linux.sh all       every step above
# Logs go to target/linux-logs/<step>.log; the last lines of a failing step are printed.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
export PATH="$HOME/.cargo/bin:$PATH"
logs=$PWD/target/linux-logs
mkdir -p "$logs"

run() {
  local name=$1; shift
  echo "== $name"
  if "$@" >"$logs/$name.log" 2>&1; then
    echo "   ok"
  else
    echo "   FAILED ($logs/$name.log)"
    tail -40 "$logs/$name.log"
    return 1
  fi
}

# tauri-build refuses to compile the desktop crate without the page build and the relay binary
# beside it, and --workspace includes that crate.
prepare() {
  run desktop-front npm run desktop:build \
    && TAURI_ENV_DEBUG=true run relay node scripts/build-host.mjs
}

rust() {
  prepare \
    && run cargo-test cargo test --workspace \
    && run cargo-clippy cargo clippy --workspace --exclude wmpass-desktop --all-targets --all-features -- -D warnings \
    && run cargo-deny cargo deny check
}

desktop() {
  prepare \
    && run desktop-clippy cargo clippy -p wmpass-desktop --all-targets -- -D warnings \
    && run desktop-test cargo test -p wmpass-desktop -p wmpass-host -p wmpass-bridge
}

# The debug build reads VITE_SELFTEST, runs desktop/src/selftest.ts in the real webview against a
# throwaway folder, writes settings/selftest.json there and quits by itself.
selftest() {
  local dir; dir=$(mktemp -d /tmp/wmpass-selftest.XXXXXX)
  (cd desktop && VITE_SELFTEST="$dir/backup" run selftest-build tauri build --debug --no-bundle) || return 1
  echo "== selftest-run ($dir)"
  local inner='
    printf "" | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1
    target/debug/wm-pass &
    app=$!
    for _ in $(seq 120); do
      grep -q "\"done\"" "$WMPASS_DATA_DIR/settings/selftest.json" 2>/dev/null && break
      sleep 1
    done
    sleep 2
    import -window root "$WMPASS_DATA_DIR/screen.png" 2>/dev/null
    wait $app'
  # Root cannot start WebKitGTK's sandbox; this run loads only the app's own pages.
  WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1 \
    WMPASS_DATA_DIR="$dir" timeout 240 \
    dbus-run-session -- xvfb-run -a -s "-screen 0 1280x800x24" bash -c "$inner" \
    >"$logs/selftest-run.log" 2>&1
  if [ -f "$dir/settings/selftest.json" ]; then
    cat "$dir/settings/selftest.json"
    [ -f "$dir/screen.png" ] && echo "screenshot: $dir/screen.png"
    ! grep -qE 'FAILED|"~' "$dir/settings/selftest.json"
  else
    echo "   no results; app log:"
    tail -40 "$logs/selftest-run.log"
    return 1
  fi
}

bundle() {
  # The updater key stays on the owner's Mac, so the updater artifacts are left out.
  (cd desktop && APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true run bundle \
    tauri build -c '{"bundle":{"createUpdaterArtifacts":false}}') \
  && ls -la target/release/bundle/{deb,rpm,appimage} 2>/dev/null
}

case "${1:-all}" in
  rust) rust ;;
  desktop) desktop ;;
  selftest) selftest ;;
  bundle) bundle ;;
  all) r=0; rust || r=1; desktop || r=1; selftest || r=1; bundle || r=1; exit $r ;;
  *) echo "usage: $0 rust|desktop|selftest|bundle|all" >&2; exit 2 ;;
esac
