# Security

Please report a vulnerability privately, through [GitHub's private reporting](https://github.com/wasi-master/wm-pass/security/advisories/new). Do not open a public issue. You will get an answer within a week, and fixes aim to ship within 90 days, after which the report may be published.

## What WM Pass protects against

- A stolen device while WM Pass is locked.
- A sync provider reading vault files, or replaying an older copy. Replays raise a warning.
- Other apps on the device.
- Network attackers.
- Someone demanding the password in person, up to a point. See the limits below.

## What it does not protect against

- A compromised operating system while the vault is unlocked.
- A copy of a vault taken before an entry self-destructed, or before a rekey. That copy still holds what it held.

## Limits you should know

- **Emergency access has no enforced wait.** Trustees holding enough Shamir shares can open the vault at once. The waiting period is advice. A claim notice can alert your devices, and rekeying stops the shares from opening anything written afterwards.
- **Duress and hidden vaults do not defeat forensics.** Flash storage does not reliably erase, and a prior disk image or a sync provider's file history survives a wipe. They help against a quick look under pressure.
- **Desktop builds are unsigned.** macOS and Windows warn on first run, and macOS has no biometric unlock, because Secure Enclave keychain items need a signed app. Verify a first download against the minisign signature published with each release.
- **A KeePass database opened in place** is only as strong as its own settings, and its writer is the `keepass` crate's experimental KDBX 4 writer. Every save is read back and compared, and the previous file is kept.

- **No third-party audit yet.** One is planned for `wmpass-core` before version 1.0.
- **Hardware keys** have been tested on simulated devices only.

## Design

The file format is specified in [docs/src/content/docs/security/format.md](docs/format.md). Only RustCrypto and dalek primitives are used, and no new constructions.
