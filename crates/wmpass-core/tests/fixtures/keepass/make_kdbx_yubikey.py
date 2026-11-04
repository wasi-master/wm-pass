#!/usr/bin/env python3
"""Writes the YubiKey fixtures beside this script: KDBX 3.1 and KDBX 4 databases whose
key includes a YubiKey's HMAC-SHA1 challenge-response, as KeePassXC sets one up.

No YubiKey is needed. A slot programmed with HMAC_SECRET answers a challenge with
HMAC-SHA1(HMAC_SECRET, challenge), so this script answers for it. The challenge is the
KDF seed in KDBX 4 and the master seed in KDBX 3.1, and the answer joins the key as its
SHA-256, after the password and the key file. The files are written from the format
alone, independently of the `keepass` crate WM Pass reads them with.

Run with `python3 make_kdbx_yubikey.py`; it needs the `cryptography` package. Seeds are
fixed, so the files change only when this script does.
"""

import base64
import hashlib
import hmac
import struct
from pathlib import Path

from cryptography.hazmat.primitives import padding
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

HERE = Path(__file__).parent
HMAC_SECRET = bytes(range(0x40, 0x54))
PASSWORD = "yk-pw"
ROUNDS = 1000
AES_CIPHER = bytes.fromhex("31c1f2e6bf714350be5805216afc5aff")
AES_KDF = bytes.fromhex("c9d9f39a628a4460bf740d08c18a4fea")
MASTER_SEED = bytes(range(0x80, 0xA0))
KDF_SEED = bytes(range(0xA0, 0xC0))
IV = bytes(range(0xC0, 0xD0))
STREAM_KEY = bytes(range(0xD0, 0xF0))
START_BYTES = bytes(range(0x10, 0x30))
GROUP_ID = bytes(range(0x60, 0x70))
ENTRY_ID = bytes(range(0x70, 0x80))


def response(challenge):
    return hmac.new(HMAC_SECRET, challenge, hashlib.sha1).digest()


def composite(password, key_file, challenge):
    parts = []
    if password is not None:
        parts.append(hashlib.sha256(password.encode("utf-8")).digest())
    if key_file is not None:
        # Only the 32-byte binary form is used here, which is taken as it is.
        assert len(key_file) == 32
        parts.append(key_file)
    parts.append(hashlib.sha256(response(challenge)).digest())
    return hashlib.sha256(b"".join(parts)).digest()


def aes_kdf(key, seed):
    encryptor = Cipher(algorithms.AES(seed), modes.ECB()).encryptor()
    for _ in range(ROUNDS):
        key = encryptor.update(key)
    return hashlib.sha256(key).digest()


def aes_cbc(key, data):
    padder = padding.PKCS7(128).padder()
    padded = padder.update(data) + padder.finalize()
    encryptor = Cipher(algorithms.AES(key), modes.CBC(IV)).encryptor()
    return encryptor.update(padded) + encryptor.finalize()


def xml(header_hash=None):
    b64 = lambda data: base64.b64encode(data).decode()
    meta_hash = f"<HeaderHash>{b64(header_hash)}</HeaderHash>" if header_hash else ""
    fields = [
        ("Title", "Mail"),
        ("UserName", "me"),
        ("Password", "yk-entry-pw"),
        ("URL", "https://mail.example"),
    ]
    strings = "".join(f"<String><Key>{k}</Key><Value>{v}</Value></String>" for k, v in fields)
    return (
        '<?xml version="1.0" encoding="utf-8" standalone="yes"?>\n'
        "<KeePassFile><Meta><Generator>make_kdbx_yubikey.py</Generator>"
        f"{meta_hash}<DatabaseName>YubiKey</DatabaseName></Meta>"
        f"<Root><Group><UUID>{b64(GROUP_ID)}</UUID><Name>Root</Name>"
        f"<Entry><UUID>{b64(ENTRY_ID)}</UUID>{strings}</Entry>"
        "</Group></Root></KeePassFile>"
    ).encode("utf-8")


def kdbx31(password, key_file):
    def field(kind, data):
        return struct.pack("<BH", kind, len(data)) + data

    header = b"".join(
        [
            bytes.fromhex("03d9a29a67fb4bb5") + struct.pack("<HH", 1, 3),
            field(2, AES_CIPHER),
            field(3, struct.pack("<I", 0)),
            field(4, MASTER_SEED),
            field(5, KDF_SEED),
            field(6, struct.pack("<Q", ROUNDS)),
            field(7, IV),
            field(8, STREAM_KEY),
            field(9, START_BYTES),
            field(10, struct.pack("<I", 2)),
            field(0, b"\r\n\r\n"),
        ]
    )
    body = xml(hashlib.sha256(header).digest())
    blocks = (
        struct.pack("<I", 0)
        + hashlib.sha256(body).digest()
        + struct.pack("<I", len(body))
        + body
        + struct.pack("<I", 1)
        + bytes(32)
        + struct.pack("<I", 0)
    )
    transformed = aes_kdf(composite(password, key_file, MASTER_SEED), KDF_SEED)
    master_key = hashlib.sha256(MASTER_SEED + transformed).digest()
    return header + aes_cbc(master_key, START_BYTES + blocks)


def kdbx40(password, key_file):
    def field(kind, data):
        return struct.pack("<BI", kind, len(data)) + data

    def item(kind, name, value):
        name = name.encode()
        return struct.pack("<Bi", kind, len(name)) + name + struct.pack("<i", len(value)) + value

    kdf = (
        struct.pack("<H", 0x0100)
        + item(0x42, "$UUID", AES_KDF)
        + item(0x05, "R", struct.pack("<Q", ROUNDS))
        + item(0x42, "S", KDF_SEED)
        + b"\x00"
    )
    header = b"".join(
        [
            bytes.fromhex("03d9a29a67fb4bb5") + struct.pack("<HH", 0, 4),
            field(2, AES_CIPHER),
            field(3, struct.pack("<I", 0)),
            field(4, MASTER_SEED),
            field(7, IV),
            field(11, kdf),
            field(0, b"\r\n\r\n"),
        ]
    )
    transformed = aes_kdf(composite(password, key_file, KDF_SEED), KDF_SEED)
    master_key = hashlib.sha256(MASTER_SEED + transformed).digest()
    hmac_key = hashlib.sha512(MASTER_SEED + transformed + b"\x01").digest()

    def block_key(index):
        return hashlib.sha512(struct.pack("<Q", index) + hmac_key).digest()

    inner = (
        struct.pack("<BI", 1, 4)
        + struct.pack("<I", 3)
        + struct.pack("<BI", 2, 64)
        + STREAM_KEY * 2
        + struct.pack("<BI", 0, 0)
    )
    payload = aes_cbc(master_key, inner + xml())
    out = header + hashlib.sha256(header).digest()
    out += hmac.new(block_key(2**64 - 1), header, hashlib.sha256).digest()
    for index, data in enumerate([payload, b""]):
        signed = struct.pack("<QI", index, len(data)) + data
        out += hmac.new(block_key(index), signed, hashlib.sha256).digest()
        out += struct.pack("<I", len(data)) + data
    return out


def main():
    key_file = (HERE / "key-binary32.key").read_bytes()
    files = {
        "yk-kdbx31-password.kdbx": kdbx31(PASSWORD, None),
        "yk-kdbx31-only.kdbx": kdbx31(None, None),
        "yk-kdbx40-password.kdbx": kdbx40(PASSWORD, None),
        "yk-kdbx40-key-file.kdbx": kdbx40(None, key_file),
    }
    for name, data in files.items():
        (HERE / name).write_bytes(data)


if __name__ == "__main__":
    main()
