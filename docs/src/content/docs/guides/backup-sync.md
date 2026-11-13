---
title: Backups and sync
description: Encrypted backups, sync through a file, over your local network, through a Bitwarden account or your own server.
---

## Backups

A backup is a bundle of your encrypted vault files. Whoever holds it still needs your password. Set one up under Settings, Backups, and choose where it goes:

- A folder on the device, which Syncthing or a cloud provider's own app can carry on from there.
- An account: Google Drive, Dropbox, OneDrive, Box, pCloud, MEGA, iCloud Drive or Proton Drive.
- A server: WebDAV, S3, SFTP, FTP, SMB, NFS, or a Synology or QNAP NAS. WebDAV fills in the address for Nextcloud, ownCloud, Seafile, Koofr, pCloud, Yandex Disk, kDrive, Hetzner Storage Box, 4shared and Tailscale's Taildrive; S3 does the same for Amazon, Cloudflare R2, Backblaze B2, Wasabi, Google Cloud Storage and nine more.
- A Git repository on GitHub, GitLab, Gitea or Forgejo, or a mailbox over IMAP.

Google Drive, Dropbox, OneDrive and Box sign in on their own page in your browser, so WM Pass never sees that password. They are offered only by builds made with an app id for that service. pCloud, MEGA and Proton Drive sign in inside WM Pass and keep only the session they hand back; iCloud keeps the password too, because Apple's sessions last about a day, and asks for a fresh code about once a month.

The Android and desktop apps have every destination. The browser extension reaches a folder only in Chrome and Edge, which let a page use a folder you pick. A browser cannot open raw connections, so SFTP, FTP, SMB, NFS and IMAP, and iCloud Drive, go through the desktop app while the extension is connected to it, and the app asks you once for each place. The extension also cannot accept a NAS's own certificate, so a NAS needs one from a known authority there.

On the phone, every place but a folder is checked before it is saved, so a wrong address or login is said at once. On a computer, an account or a NAS signs in before it is saved, and any other place is saved as you typed it; the first backup runs straight away, and its row shows whether it worked.

Each new backup is read back and checked before older ones are removed, so a bad write cannot cost you your last good copy.

To bring a copy back into the vault you have open, choose the place under Settings, Backups, then **Bring back a copy**, or use **Merge a file** for a backup file. Merging replaces nothing: where an item differs, the newer value wins and the older one goes into that item's history.

On a new device, or after a wipe, use **Restore from a file** or **Restore from a backup location** at the unlock screen. On a device that already has a vault, the restored copy does not replace it. It waits, and merges in the next time you unlock.

## File sync

Under Settings, Sync and accounts, turn on the switch beside any place you set up for backups. Each device leaves its own encrypted copy there and merges the others', so two devices syncing at once cannot overwrite each other. Changes merge field by field, and if two devices changed the same password, the losing value goes into that entry's history, so a conflict never destroys a password.

WM Pass warns when a location hands back an older copy than this device has already seen, which encryption alone would not catch.

## On your local network

Two devices can sync directly, with no server, while both are on the same network with the vault unlocked. Pair them once by QR code or by comparing six digits. Pairing alone is not enough: both devices must hold the same vault, and **Merge vaults** makes one vault of two that were set up apart. The browser extension does not sync this way. Details are in [the LAN protocol](/security/lan/).

## Accounts

A Bitwarden or Vaultwarden account can be used in place of a local vault, and so can a KeePass database opened where it is. You can also run the WM Pass server yourself, from `crates/wmpass-server`; `packaging/server` has a Docker setup. Bitwarden does not publish its API, so official changes can break this.

Add one under Settings, Sync and accounts. An account opens in place of your vault, so WM Pass then offers to lock the vault and take you to the unlock screen, with the account chosen and its password asked for.

The first sign-in from a device Bitwarden has not seen before is held back until you confirm it, unless the account has two-step login. Bitwarden emails a code, and the unlock screen asks for it beside your password. Two-step login asks for its code in the same place.

An account made on bitwarden.eu is unknown to bitwarden.com, and Bitwarden answers that the same way as a wrong password. On the phone, when the password is refused, the unlock screen names the email and the server it tried, and the account's row under Sync and accounts shows them too. If the server is wrong, add the account again with **Bitwarden EU** chosen.

Accounts made years ago may still keep their key in Bitwarden's oldest format. WM Pass opens those as Bitwarden's own apps do. If Bitwarden accepts the password but the key still does not open, the unlock screen says so instead of calling the password wrong.

## Sharing

A shared vault, under Settings, Sync and accounts, Nested and shared vaults, lives at a place everyone can reach, such as a shared folder or a WebDAV server. Each person has their own key to it, and the owner adds and removes people. Everyone holding it can read and write it. Someone you remove keeps any copy they already have, so change those passwords.

To send one login once, use **Receive an item** on one device and **Send to a device** on the other. The two devices swap QR codes, and nothing travels over a network.
