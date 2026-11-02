# Bundled data

| File | Source | Licence |
|---|---|---|
| `domains.tsv` | [2FA directory](https://2fa.directory) (2factorauth and contributors), plus `domains-extra.tsv`; built by `tools/data/build_domains.py` | MIT |
| `domains-extra.tsv`, `keywords.tsv` | Written for this project | MIT |
| `categoriser.bin` | Trained from the two files above by `cargo run -p wmpass-core --example train_categoriser` | MIT |
| `password-rules.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/password-rules.json` | MIT, © Apple Inc. |

EFF word lists come through the `diceware_wordlists` crate. They are CC BY 3.0 US, © Electronic Frontier Foundation, and must keep that attribution on the app's About page.
