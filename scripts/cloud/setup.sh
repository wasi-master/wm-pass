#!/bin/bash
# Setup script for the "WM Pass Linux" cloud environment on claude.ai/code.
# Paste this file into the environment's Setup script box (or load it with Choose file).
# It runs as root on Ubuntu 24.04 x86_64 before Claude starts, and the result is cached
# for about a week, so it only provisions the machine. Repository steps (pinned Rust
# toolchain, npm ci) live in scripts/cloud/session.sh, which a SessionStart hook runs.
# Keep it under five minutes or the environment is not cached.
set -uo pipefail
export DEBIAN_FRONTEND=noninteractive

# Tauri's WebKitGTK stack, the hardware-key crates, and what the Linux self-test needs:
# a virtual display, a session bus, a Secret Service keyring, and software GL.
apt-get update -q
apt-get install -y -q --no-install-recommends \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
  libudev-dev libpcsclite-dev \
  build-essential pkg-config file patchelf rpm desktop-file-utils \
  xvfb xauth dbus dbus-x11 at-spi2-core gnome-keyring libsecret-tools \
  libgl1-mesa-dri mesa-utils imagemagick xdotool openssh-client \
  || exit 1

# rustup, so rust-toolchain.toml's pinned channel installs itself. The download host for
# sh.rustup.rs is not on the Trusted list; static.rust-lang.org is.
if ! command -v rustup >/dev/null; then
  curl -fsSLo /tmp/rustup-init https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --profile minimal --default-toolchain none \
    && rm /tmp/rustup-init
fi
export PATH="$HOME/.cargo/bin:$PATH"
# Same channel as rust-toolchain.toml; session.sh catches it up if the pin moves.
rustup toolchain install 1.99.1 --profile minimal -c clippy,rustfmt || true

# cargo-deny as a prebuilt binary: building it from source takes longer than the cache allows.
DENY=0.20.2
curl -fsSL "https://github.com/EmbarkStudios/cargo-deny/releases/download/$DENY/cargo-deny-$DENY-x86_64-unknown-linux-musl.tar.gz" \
  | tar xz -C /usr/local/bin --strip-components=1 "cargo-deny-$DENY-x86_64-unknown-linux-musl/cargo-deny" || true

# Tauri's CLI, for packaging (.deb, .rpm, .AppImage) and the self-test build.
npm install -g @tauri-apps/cli@2.12.4 || true

# Cloud sessions run as root, where WebKitGTK's bubblewrap sandbox cannot start.
# The self-test only loads the app's own pages, so it turns the sandbox off for itself.
exit 0
