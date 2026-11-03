# WM Pass — notes for Claude

## Linux cloud sessions

Cloud sessions on claude.ai/code run in the "WM Pass Linux" environment: Ubuntu 24.04, x86_64,
root, 4 vCPUs, 16 GB, no display. Its setup script is `scripts/cloud/setup.sh` (system packages,
rustup, cargo-deny, Tauri CLI), and `scripts/cloud/session.sh` runs at session start (pinned
toolchain, `npm ci`). A change to `setup.sh` reaches new sessions only after it is pasted into
the environment again.

Linux checks, one step at a time, each a few minutes:

```bash
scripts/cloud/linux.sh rust      # cargo test --workspace, clippy, cargo deny
scripts/cloud/linux.sh desktop   # desktop crates: clippy and tests
scripts/cloud/linux.sh selftest  # debug app on Xvfb, real flows, JSON results and a screenshot
scripts/cloud/linux.sh bundle    # .deb, .rpm, .AppImage under target/release/bundle
```

Run them in the background (a cold `cargo test --workspace` outlasts the 2-minute command
default). Logs are in `target/linux-logs/`.

Not covered there: hardware keys (no USB), the tray and global shortcut (no desktop shell on
Xvfb), the browser relay to a real browser, and arm64. The Android app and the docs build are
not Linux-specific; leave them to their own sessions.
