---
title: "The vault file format"
description: "Every byte of a .wmvault file, for anyone auditing it or writing another reader."
---

All integers are little-endian. Key derivation is Argon2id version 0x13. Sub-keys are HKDF-SHA256 with the vault id as salt and the named `info` string. Sealed values are XChaCha20-Poly1305 as `nonce (24) || ciphertext || tag (16)`.

## Layout

| Offset | Bytes | Field |
|---|---:|---|
| 0 | 4 | Magic `WMPV` |
| 4 | 2 | Format version, 1 |
| 6 | 2 | Reserved, zero |
| 8 | 4 | Argon2 memory, KiB |
| 12 | 4 | Argon2 passes |
| 16 | 4 | Argon2 lanes |
| 20 | 32 | Salt |
| 52 | 16 | Vault id, random |
| 68 | 8 × 74 | Slot records |
| 660 | 8 × 256 | Hint records |
| 2708 | 72 | File key sealed under the body key, with the vault id as AAD |
| 2780 | 19 | STREAM nonce prefix |
| 2799 | 32 | HMAC-SHA256 over bytes 0 to 2798, under the header key |
| 2831 | to end | Body |

The header has a fixed size. Unused slot and hint records are random bytes, so a reader without a password cannot count unlock methods or tell a vault from a chaff file. Readers must refuse Argon2 settings below 64 MiB and 3 passes, or above 4 GiB, 64 passes and 16 lanes, before deriving anything.

## Keys

```
root      = Argon2id(password, salt)
slot key  = HKDF(root,                vault id, "wmpass/slot/v1")
          or HKDF(root || factor,     vault id, "wmpass/slot/v1")
hint key  = HKDF(root,                vault id, "wmpass/hint/v1")
header key= HKDF(vault key,           vault id, "wmpass/header/v1")
body key  = HKDF(vault key,           vault id, "wmpass/body/v1")
```

`factor` is BLAKE3 of a key file, a FIDO2 hmac-secret output, or a YubiKey HMAC-SHA1 response. A recovery key rebuilt from Shamir shares replaces `root` with `HKDF(key, "wmpass", "wmpass/recovery-root/v1")`.

Files in one set share a salt and settings, so one derivation can be tried against all of them. This keeps unlock time independent of how many real, decoy or chaff files exist.

## Records

A slot record seals 34 bytes: `1`, an action byte (0 open, 1 wipe local, 2 hard lock), then the 32-byte vault key. The AAD is `vault id || slot index`.

A hint record is sealed under the hint key with the same AAD, over 216 bytes of padded plaintext. It tells a client that holds the right password which second factor to ask for: `1` key file; `2` FIDO2 (`u16` credential id length, 32-byte salt, credential id); `3` YubiKey (slot number, 32-byte challenge).

## Body

A fresh random file key and nonce prefix are chosen on every save. The plaintext is framed as `u64 length || data`, padded with zeros to the Padmé length of the framed size, then split into 64 KiB chunks. Each chunk is sealed without a stored nonce, using `prefix (19) || big-endian u32 index || last-chunk flag` as the nonce and the header MAC as AAD. This is the STREAM construction, so truncation, reordering and splicing all fail.

The data is a MessagePack map with named fields, described by `VaultDoc` in `crates/wmpass-core/src/model.rs`. Its `generation` rises with every save, and clients remember the highest they have seen to detect a replayed file. Its `minor` number marks additive changes: a client seeing a higher one than it knows must open the vault read-only, because saving would drop fields it cannot see.

## Attachments

Each attachment is a separate file named with a random id: a 19-byte nonce prefix, then a STREAM under a random key with the id as AAD. The key lives in the entry, so deleting the entry destroys the attachment.

## Chaff

A chaff file has the same public fields as its neighbours, then random bytes sized like a real body. Nothing distinguishes it without a password.
