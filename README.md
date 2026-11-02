# WM Pass

A password manager from the WM Suite, with security as its first requirement. Everything works offline, and every network feature starts switched off. MIT licensed.

This repository is being built in phases, and the Rust core comes first. See [SECURITY.md](SECURITY.md) for the threat model and [docs/format.md](docs/format.md) for the vault format.

| Crate | What it holds |
|---|---|
| `wmpass-core` | Vault format, key slots, data model, merge, generator, health, search, import and export. No platform code and no network |
| `wmpass-cli` | The `wmpass` command |

```bash
cargo test --workspace
```

```bash
cargo run -p wmpass-cli -- create my.wmvault
```

```bash
cargo run -p wmpass-cli -- add my.wmvault GitHub --username octo --url https://github.com
```

Data sources and their licences are listed in [data/README.md](data/README.md).
