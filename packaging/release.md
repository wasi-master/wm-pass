# Releasing WM Pass for desktop

A tag named `desktop-v<version>` runs `.github/workflows/desktop-release.yml`, which builds on macOS,
Windows and Linux and attaches everything to one GitHub release.

## What a release contains

- macOS: a universal `.dmg`, and an `.app.tar.gz` for the updater
- Windows: an NSIS installer and an `.msi`
- Linux: `.deb`, `.rpm` and `.AppImage`
- `latest.json`, the updater's manifest
- A `.sig` next to every updater file, and `SHA256SUMS` over everything

## Signing

Desktop builds carry no Apple or Windows certificate, so Gatekeeper and SmartScreen warn on the first
launch. Updates are signed with minisign instead. The public key is in `tauri.conf.json`, and the app
refuses an update that does not verify against it.

The private key is at `~/.config/wm-pass/updater.key` on the maintainer's machine, and goes into the
GitHub secret `TAURI_SIGNING_PRIVATE_KEY`. **Losing it ends updates for every installed copy**: they
would have to be reinstalled by hand. Keep a backup somewhere other than this computer.

## After the workflow finishes

1. Check the release notes and publish the draft.
2. Homebrew: `packaging/homebrew/stamp.py <version> <sha256 of the dmg>`, then push the cask to the tap.
3. AUR: `packaging/aur/stamp.py --version <version> --assets <folder of downloads>`, then push the
   `wm-pass-bin` folder to `ssh://aur@aur.archlinux.org/wm-pass-bin.git`.

## The relay program

`wm-pass-host` is what the browser starts for the extension. `scripts/build-host.mjs` builds it during
the Tauri build, and Tauri puts it beside the app. A release build stops with an error if only the empty
stand-in from `build.rs` is there.
