---
title: Backups and sync
description: Encrypted backups, sync through a file, over your local network, through a Bitwarden account or your own server.
---

## Backups

A backup is a bundle of your encrypted vault files. Whoever holds it still needs your password. Set one up under Settings, Backups, and choose a folder, WebDAV or S3; the Android app offers more destinations. Each new backup is read back and checked before older ones are removed, so a bad write cannot cost you your last good copy.

Restoring asks whether to replace what you have or merge into it.

## File sync

Sync writes your vault to a place you choose, and other devices merge from there. Changes merge field by field, and if two devices changed the same password, the losing value goes into that entry's history, so a conflict never destroys a password.

WM Pass warns when a location hands back an older copy than this device has already seen, which encryption alone would not catch.

## On your local network

Two devices can sync directly, with no server. Pair them once by QR code or a six-digit code. Details are in [the LAN protocol](/security/lan/).

## Accounts

A Bitwarden or Vaultwarden account can be used in place of a local vault, and so can a KeePass database opened where it is. You can also run the WM Pass server yourself, from `crates/wmpass-server`; `packaging/server` has a Docker setup. Bitwarden does not publish its API, so official changes can break this.

## Sharing

Share a vault with family through a shared file or a server organisation. Everyone holding it can read and write it. Someone you remove keeps any copy they already have, so change those passwords. To send one login once, use a one-time QR code between two devices.
