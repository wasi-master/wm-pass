---
title: Threat model
description: What WM Pass protects against, what it does not, and how each platform handles its keys.
sidebar:
  order: 1
---

## It protects against

- A stolen device while WM Pass is locked.
- A sync provider reading your files, or replaying an older copy. Replays raise a warning.
- Other apps on the device.
- People on your network: traffic uses TLS, or Noise on the local network.
- A hostile web page trying to read your logins or trick you into filling them.
- Someone demanding your password in person, up to the limits in [Duress](/guides/duress/).

## It does not protect against

- A compromised operating system while the vault is unlocked.
- A copy of a vault made before an entry self-destructed or before you rekeyed.
- An accessibility service you have turned on, while a secret is on show. Hidden password fields give these services dots in place of their text, but anything visible on screen is theirs to read.

## Keys

The vault key is random and sits in key slots. A password slot uses Argon2id with at least 64 MiB of memory, and readers refuse anything weaker, so a tampered header cannot downgrade it. Unused slots are random bytes, so a file does not reveal how many ways in it has, or whether one is a duress password.

Keys stay in Rust and are wiped on lock. View data crossing into a user interface never includes secret fields. A secret you choose to see or type does become a string in the JVM or JavaScript, and those cannot be wiped. On desktop, core dumps are off, though keys are not locked out of swap.

## Quick unlock by platform

| Platform | How | Strength |
|---|---|---|
| Android 11 and up | A per-use key in the Keystore, or StrongBox where present, behind a biometric prompt, and invalidated by a new enrolment | Bound to hardware |
| Android 8 to 10 | An older form of the same, biometric only | Bound to hardware |
| Linux | A wrap key in the Secret Service, off by default | Convenience: any program in your session can ask for it |
| macOS | None. The builds are unsigned, and Secure Enclave items need a signed app | n/a |
| Windows | None yet | n/a |
| Extension | The key stays in browser session memory until you lock or close it | Memory only |

## Between apps

**The extension and the desktop app** talk over Noise XX through a relay program that holds no keys. A new extension needs a code compared on both screens, and each side commits to its random value before seeing the other's, so someone in the middle gets one chance in a million. An extension can ask only for what matches a page and to fill an entry that matches it. The extension's own key is sealed by a browser key that scripts cannot read out, which stops a copied profile, but not code running as the extension.

**WM Keyboard** checks WM Pass's signing certificate, and WM Pass checks WM Keyboard's on every call, and it is off until you allow it.

## Supply chain

Only RustCrypto and dalek primitives are used, with no new constructions. `cargo deny` enforces permissive licences and known advisories on every change. There are property tests on merging and the format, and test vectors in the repository. Desktop updates carry a minisign signature that the app verifies.

A third-party audit of the core is planned before version 1.0. It has not happened yet.
