# Bundled data

| File | Source | Licence |
|---|---|---|
| `domains.tsv` | [2FA directory](https://2fa.directory) (2factorauth and contributors), plus `games.tsv` and `domains-extra.tsv`; built by `tools/data/build_domains.py` | MIT |
| `domains-extra.tsv`, `keywords.tsv` | Written for this project | MIT |
| `games.tsv` | Written for this project: games played with an account, by name and site, which file under Gaming | MIT |
| `categoriser.bin` | Trained from `domains.tsv`, `keywords.tsv` and `games.tsv` by `cargo run -p wmpass-core --example train_categoriser` | MIT |
| `related/shared-credentials.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/shared-credentials.json` | MIT, © Apple Inc. |
| `related/change-password-urls.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/change-password-URLs.json` | MIT, © Apple Inc. |
| `related/accounts.txt` | Written for this project | MIT |
| `password-rules.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/password-rules.json` | MIT, © Apple Inc. |
| `cardscan/darknite.tflite` | Stripe CardScan's digit detector, [stripe-android](https://github.com/stripe/stripe-android) `stripecardscan/src/main/assets/darknite_1_1_1_16.tflite` (SHA-256 `8d8e3f79aa0783ab0cfa5c8d65d663a9da6ba99401efb2298aaaee387c3b00d6`), its float16 weights widened to float32 by `tools/data/widen_cardscan.py` so tract can run it. Same outputs as the original under LiteRT. | MIT, © Stripe, Inc.; license text in `cardscan/LICENSE-darknite.txt` |

EFF word lists come through the `diceware_wordlists` crate. They are CC BY 3.0 US, © Electronic Frontier Foundation, and must keep that attribution on the app's About page.
