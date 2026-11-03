#!/bin/bash
# SessionStart hook: brings a cloud session's clone up to date. Does nothing on a laptop.
[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0
cd "$CLAUDE_PROJECT_DIR" || exit 0
export PATH="$HOME/.cargo/bin:$PATH"

# Installs the channel rust-toolchain.toml pins, if the setup script's copy is older.
rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install

# npm ci only when the lock file changed since the last good install.
stamp=node_modules/.cloud-session.sha256
if ! sha256sum --check --status "$stamp" 2>/dev/null; then
  rm -f "$stamp"
  npm ci --no-audit --no-fund >/dev/null 2>&1 && sha256sum package.json package-lock.json > "$stamp"
fi
exit 0
