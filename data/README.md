# Bundled data

| File | Source | Licence |
|---|---|---|
| `domains.tsv` | [2FA directory](https://2fa.directory) (2factorauth and contributors), plus `games.tsv`, `domains-extra.tsv` and `wikidata/`; built by `tools/data/build_domains.py` | MIT; the `wikidata/` rows CC0 |
| `domains-extra.tsv`, `keywords.tsv` | Written for this project | MIT |
| `games.tsv` | Written for this project: games played with an account, by name and site, which file under Gaming | MIT |
| `wikidata/sites.tsv`, `wikidata/games.tsv`, `wikidata/apps.tsv`, `wikidata/brands.tsv` | [Wikidata](https://www.wikidata.org), queried through [QLever](https://qlever.dev) by `tools/data/wikidata.py`: organisations by class and industry, online and Android games, Android apps with a site, and official sites for Simple Icons brands | CC0 1.0 |
| `categoriser.bin` | Trained from `domains.tsv`, `keywords.tsv`, `games.tsv` and the `wikidata/` names and games by `cargo run -p wmpass-core --example train_categoriser` | MIT |
| `related/shared-credentials.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/shared-credentials.json` | MIT, © Apple Inc. |
| `related/change-password-urls.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/change-password-URLs.json` | MIT, © Apple Inc. |
| `related/accounts.txt`, `related/apps.tsv` | Written for this project; the Google family lists the domains of `google.com/supported_domains` | MIT |
| `password-rules.json` | [Apple password-manager-resources](https://github.com/apple/password-manager-resources) `quirks/password-rules.json` | MIT, © Apple Inc. |
| `cardscan/darknite.tflite` | Stripe CardScan's digit detector, [stripe-android](https://github.com/stripe/stripe-android) `stripecardscan/src/main/assets/darknite_1_1_1_16.tflite` (SHA-256 `8d8e3f79aa0783ab0cfa5c8d65d663a9da6ba99401efb2298aaaee387c3b00d6`), its float16 weights widened to float32 by `tools/data/widen_cardscan.py` so tract can run it. Same outputs as the original under LiteRT. | MIT, © Stripe, Inc.; license text in `cardscan/LICENSE-darknite.txt` |

EFF word lists come through the `diceware_wordlists` crate. They are CC BY 3.0 US, © Electronic Frontier Foundation, and must keep that attribution on the app's About page.

To refresh everything from its sources, in this order:

```bash
python3 tools/data/wikidata.py fetch && python3 tools/data/wikidata.py build
curl -o tfa.json https://api.2fa.directory/v3/tfa.json && python3 tools/data/build_domains.py tfa.json
python3 tools/icon/gen_brands.py <simple-icons 16.36.0> <13.21.0> <9.21.0>
cargo run --release -p wmpass-core --example train_categoriser
```
