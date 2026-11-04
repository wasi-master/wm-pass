# WM Pass

A password manager from the WM Suite that keeps its vault on your own devices. There is no account to make, it works with the network off, and every network feature starts switched off. MIT licensed.

It runs on Android, macOS, Windows, Linux, Chrome, Firefox, the web, Wear OS and the terminal, over one Rust core. Read [the docs](https://wmpass.pages.dev), or start with [the threat model](docs/src/content/docs/security/threat-model.md) and [the file format](docs/src/content/docs/security/format.md). To report a vulnerability, see [SECURITY.md](SECURITY.md).

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
| `ui/` | Svelte screens shared by desktop, extension and the web vault |
| `desktop/`, `extension/`, `web/` | Those three shells. `web/` is served at wmpass.pages.dev/vault |
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

The `cargo tauri build` step runs from `desktop/src-tauri`, and `cargo run -p wmpass-cli` opens the terminal app. The Android app builds from `android/` with `./gradlew assembleDebug`, and needs the NDK and `cargo-ndk`. Pass `-Pwmpass.fdroid=true` to leave out Google's SDKs (the Wearable API and ML Kit), or `-Pwmpass.offline=true` to drop the internet permission and those SDKs with it.

The browser tests run with `node extension/test/run.mjs` after building the extension, and `node web/test/run.mjs` after `npm run web:build`. `npm run site:build` puts the web vault under the docs site's `/vault/`, with its headers from `docs/public/_headers`; the `Site release` workflow publishes it from a `site-v*` tag, with the `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets.

Google Drive, Dropbox, OneDrive and Box need WM Pass registered in each service's console, and a build offers only the services it was given ids for. The ids are not secrets apart from Box's and Google's client secrets, which those services require even from apps on a device.

| Service | Android (`local.properties` or `-P`) | Desktop and extension (environment, at build time) | Redirect to register |
|---|---|---|---|
| Google Drive | `wmpass.driveClientId` (an iOS-type client) | `WMPASS_DRIVE_DESKTOP_CLIENT_ID`, `WMPASS_DRIVE_DESKTOP_SECRET`; extension and web vault `WMPASS_DRIVE_WEB_CLIENT_ID`, `WMPASS_DRIVE_WEB_SECRET`, each from a Web application client of its own (the site workflow reads the web vault's from `WMPASS_SITE_DRIVE_CLIENT_ID` and `WMPASS_SITE_DRIVE_SECRET`) | Android: the reversed client id; desktop: `http://127.0.0.1:53683/oauth`; extension: `https://<extension id>.chromiumapp.org/oauth`; web vault: `https://wmpass.pages.dev/vault/oauth.html` |
| Dropbox | `wmpass.dropboxAppKey` | `WMPASS_DROPBOX_APP_KEY` | `wmpass://oauth`, `http://127.0.0.1:53683/oauth`, the extension's, the web vault's |
| OneDrive | `wmpass.oneDriveClientId` | `WMPASS_ONEDRIVE_CLIENT_ID` | the first three as "Mobile and desktop" redirects, the web vault's as "Single-page application" |
| Box | `wmpass.boxClientId`, `wmpass.boxClientSecret` | `WMPASS_BOX_CLIENT_ID`, `WMPASS_BOX_CLIENT_SECRET` | `https://wmpass.pages.dev/oauth/`, `http://127.0.0.1:53683/oauth`, the extension's, the web vault's; and `https://wmpass.pages.dev` as an allowed origin |

Data sources and their licences are listed in [data/README.md](data/README.md).
