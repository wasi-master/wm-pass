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
| Extension and web vault, with a passkey | A passkey on this device (Touch ID, Windows Hello, a phone) answers a PRF request, and the engine seals the vault key under that answer. Only the sealed copy is kept in the browser | Bound to the passkey: a copied profile opens nothing |

## Between apps

**The extension and the desktop app** talk over Noise XX through a relay program that holds no keys. A new extension needs a code compared on both screens, and each side commits to its random value before seeing the other's, so someone in the middle gets one chance in a million. An extension can ask only for what matches a page, to fill an entry that matches it, and to have the app check a username while network features are allowed there.

It can also have the app do two things a browser cannot, for the extension's own vault. The app reaches a backup place over SFTP, FTP, SMB, NFS, IMAP or iCloud with the details the extension sends, keeps none of them, and asks its person once for each place, named by its kind, server and account. And it asks a security key over USB, for a YubiKey's challenge-response slot or a key set up before browsers could reach it, after asking its person once. Neither lets an extension read the app's own vault. The app's network switch holds for these runs too. File bytes cross in parts, at most 512 MiB in flight per connection.

The extension's own key is sealed by a browser key that scripts cannot read out, which stops a copied profile, but not code running as the extension.

**The web vault** at wmpass.pages.dev/vault runs the same engine as WebAssembly in the page, with its files in that site's storage in your browser. Nothing about the vault reaches the site: there is no account and no server side. What the site can do is send different code on a later visit, which the extension cannot, since its code comes signed from the browser's store. So where the extension is installed, the web vault offers to hand over to it first, and the extension answers that site alone, telling it only that it is there and opening its own vault page in that tab when asked.

For the web vault itself, the defences are in what the page may do. Its Content-Security-Policy allows only its own scripts and WebAssembly, no frames either way, no forms and no plugins, and requests only over https or to this computer. It sets no cookies and loads nothing from anyone else. It is published only from a release tag in this repository by its own workflow, and the deploy can be made to wait for an owner's approval. None of that helps if that workflow or the hosting account is taken over, which is the risk the extension does not carry.

**Security keys and passkeys in a browser** go through WebAuthn's PRF extension. The credentials belong to `wmpass.pages.dev`, which the extension may claim because it may reach every site, and which a copy of the page served from anywhere else cannot. A security key is asked without its PIN, as the desktop app asks it, so the answer is the same in every app.

**WM Keyboard** checks WM Pass's signing certificate, and WM Pass checks WM Keyboard's on every call, and it is off until you allow it.

**The username checker** loads TikTok and Instagram in a hidden browser view, because their pages only answer once their own scripts run. Those scripts run inside the desktop and phone apps, so the view gets nothing from the app: it keeps no cookies or storage afterwards, may only load https pages on that site's domain, and has no route to the app's commands. On desktop, Tauri refuses commands from any page it did not ship. On Android, the view has no JavaScript bridge. The app reads only the text the page shows.

## Supply chain

Only RustCrypto and dalek primitives are used, with no new constructions. `cargo deny` enforces permissive licences and known advisories on every change. There are property tests on merging and the format, and test vectors in the repository. Desktop updates carry a minisign signature that the app verifies.

A third-party audit of the core is planned before version 1.0. It has not happened yet.
