#!/usr/bin/env python3
"""Writes the KeePass 1 (.kdb) fixtures beside this script.

No app still writes KeePass 1 databases, so these follow the format KeePass 1.x
wrote: a 124-byte header, then groups and entries as typed fields, encrypted with
AES-256-CBC under a key from AES-KDF. Run with `python3 make_kdb.py`; it needs the
`cryptography` package. Seeds are fixed, so the files change only when this
script does.
"""

import hashlib
import struct
from pathlib import Path

from cryptography.hazmat.primitives import padding
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

HERE = Path(__file__).parent
ROUNDS = 600
MASTER_SEED = bytes(range(16))
IV = bytes(range(16, 32))
TRANSFORM_SEED = bytes(range(32, 64))
NO_TIME = b"\x2e\xdf\x39\x7e\xfb"  # KeePass 1's "never expires": 2999-12-28 23:59:59


def field(kind, data):
    return struct.pack("<HI", kind, len(data)) + data


def text(value):
    return value.encode("utf-8") + b"\0"


def group(gid, name, level):
    return b"".join(
        [
            field(0x0001, struct.pack("<I", gid)),
            field(0x0002, text(name)),
            field(0x0007, struct.pack("<I", 1)),
            field(0x0008, struct.pack("<H", level)),
            field(0x0009, struct.pack("<I", 0)),
            field(0xFFFF, b""),
        ]
    )


def entry(number, gid, title, url, user, password, notes, attachment=None):
    parts = [
        field(0x0001, number.to_bytes(16, "little")),
        field(0x0002, struct.pack("<I", gid)),
        field(0x0003, struct.pack("<I", 0)),
        field(0x0004, text(title)),
        field(0x0005, text(url)),
        field(0x0006, text(user)),
        field(0x0007, text(password)),
        field(0x0008, text(notes)),
        field(0x000C, NO_TIME),
    ]
    if attachment:
        name, data = attachment
        parts += [field(0x000D, text(name)), field(0x000E, data)]
    else:
        parts += [field(0x000D, b"\0"), field(0x000E, b"")]
    parts.append(field(0xFFFF, b""))
    return b"".join(parts)


def transform(composite):
    ecb = Cipher(algorithms.AES(TRANSFORM_SEED), modes.ECB()).encryptor()
    key = composite
    for _ in range(ROUNDS):
        key = ecb.update(key)
    return hashlib.sha256(key).digest()


def key_file_part(data):
    """A key file as KeePass 1 read it: 32 raw bytes, 64 hex digits, or a hash."""
    if len(data) == 32:
        return data
    if len(data) == 64:
        try:
            return bytes.fromhex(data.decode("ascii"))
        except ValueError:
            pass
    return hashlib.sha256(data).digest()


def write(name, groups, entries, password=None, key_file=None, encoding="utf-8"):
    parts = []
    if password is not None:
        parts.append(hashlib.sha256(password.encode(encoding)).digest())
    if key_file is not None:
        parts.append(key_file_part(key_file))
    composite = parts[0] if len(parts) == 1 else hashlib.sha256(b"".join(parts)).digest()
    master = hashlib.sha256(MASTER_SEED + transform(composite)).digest()
    body = b"".join(groups) + b"".join(entries)
    header = struct.pack("<IIII", 0x9AA2D903, 0xB54BFB65, 0x3, 0x00030004)
    header += MASTER_SEED + IV
    header += struct.pack("<II", len(groups), len(entries))
    header += hashlib.sha256(body).digest() + TRANSFORM_SEED + struct.pack("<I", ROUNDS)
    padder = padding.PKCS7(128).padder()
    padded = padder.update(body) + padder.finalize()
    cbc = Cipher(algorithms.AES(master), modes.CBC(IV)).encryptor()
    (HERE / name).write_bytes(header + cbc.update(padded) + cbc.finalize())


GROUPS = [group(1, "Internet", 0), group(2, "Mail", 1), group(3, "Backup", 0)]
ENTRIES = [
    entry(1, 1, "Forum", "https://forum.example", "me", "forum-pass", "line one\nline two"),
    entry(2, 2, "Webmail", "https://mail.example", "me@example.com", "mail-pass", "",
          ("note.txt", b"attached text")),
    # Old copy of an edited entry, which KeePass 1 kept in its Backup group.
    entry(3, 3, "Webmail", "https://mail.example", "me@example.com", "old-pass", ""),
    # KeePass 1's own settings, hidden in KeePass 1.
    entry(4, 1, "Meta-Info", "$", "SYSTEM", "", "KPX_GROUP_TREE_STATE",
          ("bin-stream", b"\x01\x00\x00\x00\x01")),
]

write("kdb1-password.kdb", GROUPS, ENTRIES, password="kdb-pw")
# KeePass 1 hashed passwords as Windows-1252, not UTF-8.
write("kdb1-ansi-password.kdb", GROUPS[:1], ENTRIES[:1], password="Pässwörd€", encoding="cp1252")
KEY = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
(HERE / "kdb1.key").write_bytes(KEY)
write("kdb1-key-file.kdb", GROUPS[:1], ENTRIES[:1], key_file=KEY)
write("kdb1-password-and-key-file.kdb", GROUPS[:1], ENTRIES[:1], password="kdb-pw", key_file=KEY)
