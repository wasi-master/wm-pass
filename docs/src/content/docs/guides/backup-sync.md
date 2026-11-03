---
title: Backups and sync
description: Encrypted backups, sync through a file, over your local network, through a Bitwarden account or your own server.
---

## Backups

A backup is a bundle of your encrypted vault files. Whoever holds it still needs your password. Set one up under Settings, Backups, and choose where it goes:

- A folder on the device, which Syncthing or a cloud provider's own app can carry on from there.
- An account: Google Drive, Dropbox, OneDrive, Box, pCloud, MEGA, iCloud Drive or Proton Drive.
- A server: WebDAV, S3, SFTP, FTP, SMB, NFS, or a Synology or QNAP NAS. WebDAV fills in the address for Nextcloud, ownCloud, Seafile, Koofr, pCloud, Yandex Disk, kDrive, Hetzner Storage Box, 4shared and Tailscale's Taildrive; S3 does the same for Amazon, Cloudflare R2, Backblaze B2, Wasabi, Google Cloud Storage and about ten more.
- A Git repository on GitHub, GitLab, Gitea or Forgejo, or a mailbox over IMAP.

Google Drive, Dropbox, OneDrive and Box sign in on their own page in your browser, so WM Pass never sees that password. pCloud, MEGA and Proton Drive sign in inside WM Pass and keep only the session they hand back; iCloud keeps the password too, because Apple's sessions last about a day, and asks for a fresh code about once a month. The Android and desktop apps have every destination. The browser extension has all of them except a folder, iCloud Drive and the ones that need a raw connection (SFTP, FTP, SMB, NFS and IMAP); it also cannot accept a NAS's own certificate, so a NAS needs one from a known authority there.

Each new backup is read back and checked before older ones are removed, so a bad write cannot cost you your last good copy.

Restoring asks whether to replace what you have or merge into it.

## File sync

Sync writes your vault to a place you choose, and other devices merge from there. Changes merge field by field, and if two devices changed the same password, the losing value goes into that entry's history, so a conflict never destroys a password.

WM Pass warns when a location hands back an older copy than this device has already seen, which encryption alone would not catch.

## On your local network

Two devices can sync directly, with no server. Pair them once by QR code or a six-digit code. Details are in [the LAN protocol](/security/lan/).

## Accounts

A Bitwarden or Vaultwarden account can be used in place of a local vault, and so can a KeePass database opened where it is. You can also run the WM Pass server yourself, from `crates/wmpass-server`; `packaging/server` has a Docker setup. Bitwarden does not publish its API, so official changes can break this.

Add one under Settings, Sync and accounts. An account opens in place of your vault, so WM Pass then offers to lock the vault and take you to the unlock screen, with the account chosen and its password asked for.

The first sign-in from a device Bitwarden has not seen before is held back until you confirm it, unless the account has two-step login. Bitwarden emails a code, and the unlock screen asks for it beside your password. Two-step login asks for its code in the same place.

## Sharing

Share a vault with family through a shared file or a server organisation. Everyone holding it can read and write it. Someone you remove keeps any copy they already have, so change those passwords. To send one login once, use a one-time QR code between two devices.
