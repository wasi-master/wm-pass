---
title: "The LAN protocol"
description: "How two devices find, pair with and sync to each other with no server."
---

Version 2. Devices find each other through mDNS (`_wmpass._tcp.local.`), with a random instance name and no text records. An advert identifies nothing, and whether an address is a paired device is decided by the handshake.

## Session

Every connection runs `Noise_XX_25519_ChaChaPoly_SHA256` with prologue `wmpass/lan/v1`. Frames are a big-endian `u16` length followed by the Noise message. An application message is one frame holding its `u32` total length, then frames of up to 65,519 plaintext bytes. Messages above 256 MiB are refused. Values are MessagePack with named fields.

The responder's static key travels encrypted only to the initiator's ephemeral key, so a device that connects and starts a handshake can learn a device's public key. It is a stable identifier on that network and grants nothing.

## Pairing

**By QR code.** The code holds a 256-bit secret, an address and an expiry two minutes ahead. The handshake becomes `Noise_XXpsk3` with that secret, so it cannot complete without having read the code.

**By comparing digits.** A six-digit secret cannot go into a handshake, because whoever answers the handshake could try all million values offline. So the handshake runs without one and the digits come afterwards, by numeric comparison:

1. The initiator sends `C = HMAC(Na, "wmpass/lan/commit/v1" || h)`, where `h` is the Noise transcript hash.
2. The responder sends `Nb`.
3. The initiator sends `Na`. The responder checks it against `C`.
4. Both show `HMAC(Na || Nb, "wmpass/lan/code/v1" || h)`, reduced to six digits.
5. A person compares the screens and accepts on both.

An attacker in the middle holds two sessions with different `h`. It must fix `Na'` towards the responder before it can see `Na`, which stays hidden behind `C`, so it cannot search for values that make the two screens agree. Each attempt has one chance in a million, and a failed one is visible to both people.

Pairing stores the other device's public key. Later connections are refused unless the key from the handshake is in that list.

## Opening

After the handshake, each side looks up the other's key among its paired devices. The initiator may narrow that list to the devices it is looking for, and hangs up on any other. It then sends an intent: the protocol version, and for a merge, which side keeps its vault. The responder answers `Go`, `Declined` or `Version`. A responder that sees the line close before any intent takes it for a device that was looking for someone else, and says nothing.

A failure is reported together with the other device's public key once the handshake has shown it, so a screen can put the failure beside the device it happened with.

A desktop app with its vault unlocked, and the phone app while it is on screen and unlocked, keep one responder running on a port of their own and under their own advert. Each also starts a sync with whatever paired device answers soon after unlocking, about 20 seconds after a local change, and every five minutes with some jitter. Two devices that start towards each other at the same moment would each hold their vault while waiting on the other, so a background responder that cannot take its vault within three seconds drops the connection, and the next round settles it.

## Sync

Being paired does not grant vault access. Each side first proves it holds the vault key, with `HMAC(K, role || h)` where `K = HKDF(vault key, vault id, "wmpass/lan/vault-proof/v1")`. The initiator proves first, and the responder answers only after checking. A responder whose check fails sends an empty message before closing, so the initiator reports a different vault rather than a dropped line. After a rekey, old devices fail this until unlocked with the new key.

Then, taking turns so that two large transfers cannot deadlock:

| Step | Sender | Message |
|---|---|---|
| 1 | Initiator | Digests |
| 2 | Responder | Digests, then delta |
| 3 | Initiator | Delta |
| 4 | Both | Blob names wanted |
| 5 | Initiator, then responder | Blobs asked for |
| 6 | Both | State hash |

A digest is BLAKE3 over an entry's MessagePack, truncated to 128 bits. A delta holds whole entries that differ, and merging applies the same field-by-field rules as file sync. If slot records changed, the delta also holds the sealed file, since only that can carry them.

A blob is sent only if the vault refers to it, and accepted only if this end asked for it, so a peer cannot read or write other files by naming them. Blobs are already encrypted under their own keys.

## Merging two vaults

Two vaults made apart have different keys, so the proof above always fails between them. A merge makes them one. The initiator's intent says which side keeps its vault. The responder does not answer at once: it shows its person who asks and which vault stays, and answers `Go` only on a yes, so a paired device alone can never start one. The initiator waits up to three minutes for that answer.

After `Go`, with `K` the side that keeps its vault and `G` the side that gives:

| Step | Sender | Message |
|---|---|---|
| 1 | G | Every entry outside the trash, and every folder |
| 2 | K | Blob names wanted |
| 3 | G | Blobs asked for |
| 4 | K | Its sealed vault file, after taking the entries in, and the blob names it refers to |
| 5 | G | Blob names wanted |
| 6 | K | Blobs asked for |
| 7 | G | Done |

`K` matches an incoming entry to one it has by id, or by site, username and password for a login, and merges the two field by field. Anything else is added, and folders are matched by name under the same parent. `G` then replaces its vault file with `K`'s and deletes its own, so a shared password cannot open the old one. The file is ciphertext and opens only with the kept vault's master password. Blob rules are as for sync. If the line drops before step 7, `K` may already hold the entries while `G` still has its own vault, and a second merge matches rather than doubles them.

## A new device

A device with no vault cannot make the vault-key proof, so joining one replaces it with the person's word. The new device listens, advertises itself as waiting to join (an mDNS TXT record `k=join`, and nothing else), and shows a pairing QR code. The device with the vault connects and writes one unauthenticated byte saying which way it will be vouched for: `Q` for the QR code, so the handshake is `Noise_XXpsk3` with the code's secret, or `C` for the six-digit comparison above. A forged byte gains nothing, since each way runs its full handshake.

Once both are satisfied, the sender sends a version, the sealed vault file and a count, then one message for each attachment the vault refers to. The receiver keeps the file only if its header parses, accepts at most 100,000 attachments, and refuses any name that is not plain letters, digits, `-`, `_` or `.`. Both sides store each other's public key, as pairing does.

What crosses is ciphertext. The new device learns nothing until its person types the master password, and a wrong one fails exactly as it would on any copy of the file.

## One-time sharing

**By QR, both ways.** The receiver shows `wmpass-offer:1:<public key>:<expiry>`, holding a throwaway X25519 key. The sender scans it and shows `wmpass-share:1:<ephemeral public key || sealed entry>`, sealed with XChaCha20-Poly1305 under `HKDF(X25519, sender || receiver, "wmpass/share/v1")`. A photograph of either code cannot open the entry. The receiver discards its key on opening.

**By invitation**, for entries too large for a QR code. The sender shows `wmpass-invite:1:<secret>:<expiry>:<address>` and serves one `XXpsk3` connection. A photograph of this one could be used by whoever connects first. The real receiver then sees a failure.

What is shared has no history, attachments, folder, tags or links, and takes a new id.
