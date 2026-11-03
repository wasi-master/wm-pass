# WM Pass

A password manager from the WM Suite that keeps its vault on your own devices. There is no account to make, it works with the network off, and every network feature starts switched off. MIT licensed.

It runs on Android, macOS, Windows, Linux, Chrome, Firefox, Wear OS and the terminal, over one Rust core. Read [the docs](https://wmpass.pages.dev), or start with [the threat model](docs/src/content/docs/security/threat-model.md) and [the file format](docs/src/content/docs/security/format.md). To report a vulnerability, see [SECURITY.md](SECURITY.md).

## What is where

| Path | What it holds |
|---|---|
| `crates/wmpass-core` | Vault format, key slots, model, merge, generator, health, search, import and export. No platform code and no network |
| `crates/wmpass-session` | The session every shell talks to, over one frame format |
| `crates/wmpass-net`, `-sinks`, `-lan`, `-bw`, `-hwkey` | Network features, backup places, local network sync, Bitwarden client, hardware keys |
| `crates/wmpass-bridge`, `-host` | The sealed conversation between extension and desktop app, and the relay between them |
| `crates/wmpass-server` | A server you can run yourself |
| `crates/wmpass-cli`, `-tui` | `wmpass`: the full-screen terminal app, and subcommands for scripts |
| `crates/wmpass-jni`, `-wasm`, `-ffi` | The bindings for Android, browsers and other shells |
| `android/` | The Android app, Wear OS app and the watch link |
| `ui/` | Svelte screens shared by desktop and extension |
| `desktop/`, `extension/` | Those two shells |
| `docs/` | The documentation site |
| `packaging/` | Release notes and recipes for each channel |

## Building

```bash
git submodule update --init
```

```bash
cargo test --workspace
```

```bash
npm ci && npm run extension:build
```

```bash
cargo tauri build
```

```bash
cargo run -p wmpass-cli
```

The `cargo tauri build` step runs from `desktop/src-tauri`, and `cargo run -p wmpass-cli` opens the terminal app. The Android app builds from `android/` with `./gradlew assembleDebug`, and needs the NDK and `cargo-ndk`. Pass `-Pwmpass.fdroid=true` to leave out Google's Wearable API, or `-Pwmpass.offline=true` to drop the internet permission.

The browser tests run with `node extension/test/run.mjs` after building the extension.

Data sources and their licences are listed in [data/README.md](data/README.md).
