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

# A Secret Service for one command: gnome-keyring on a private session bus, its files in $1.
# The password must not be empty: with an empty one the daemon never creates the login
# keyring, every store fails with "no storage access", and the app quietly falls back to a
# key file instead.
with_keyring() {
  local home=$1; shift
  mkdir -p "$home"
  XDG_DATA_HOME="$home" dbus-run-session -- bash -c '
    printf "throwaway" | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1
    exec "$@"' keyring "$@"
}

desktop() {
  local keyring; keyring=$(mktemp -d /tmp/wmpass-keyring.XXXXXX)
  prepare \
    && run desktop-clippy cargo clippy -p wmpass-desktop --all-targets -- -D warnings \
    && run desktop-test cargo test -p wmpass-desktop -p wmpass-host -p wmpass-bridge \
    && run desktop-keychain with_keyring "$keyring" cargo test -p wmpass-desktop -- --ignored
}

# The debug build reads VITE_SELFTEST, runs desktop/src/selftest.ts in the real webview against a
# throwaway folder, writes settings/selftest.json there and quits by itself.
selftest() {
  local dir; dir=$(mktemp -d /tmp/wmpass-selftest.XXXXXX)
  (cd desktop && VITE_SELFTEST="$dir/backup" run selftest-build tauri build --debug --no-bundle) || return 1
  echo "== selftest-run ($dir)"
  local inner='
    target/debug/wm-pass &
    app=$!
    for _ in $(seq 120); do
      grep -q "\"done\"" "$WMPASS_DATA_DIR/settings/selftest.json" 2>/dev/null && break
      sleep 1
    done
    sleep 2
    import -window root "$WMPASS_DATA_DIR/screen.png" 2>/dev/null
    wait $app'
  # The app's sockets (SSH agent, browser bridge) go beside the data, not in the user's
  # runtime folder, where a real WM Pass would have its own.
  mkdir -m 700 "$dir.run"
  # Root cannot start WebKitGTK's sandbox; this run loads only the app's own pages.
  WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1 \
    WMPASS_DATA_DIR="$dir" WMPASS_RUN_DIR="$dir.run" \
    with_keyring "$dir.keyring" timeout 240 xvfb-run -a -s "-screen 0 1280x800x24" bash -c "$inner" \
    >"$logs/selftest-run.log" 2>&1
  if [ -f "$dir/settings/selftest.json" ]; then
    cat "$dir/settings/selftest.json"
    [ -f "$dir/screen.png" ] && echo "screenshot: $dir/screen.png"
    local ok=0
    grep -qE 'FAILED|"~' "$dir/settings/selftest.json" && ok=1
    # The sealing key belongs in the keyring; this file means the app could not reach it.
    if [ -f "$dir/sealed/key" ]; then
      echo "   the sealing key fell back to a file: the keyring was not reachable"
      ok=1
    fi
    return $ok
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
