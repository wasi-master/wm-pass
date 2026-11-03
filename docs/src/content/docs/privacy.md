---
title: Privacy policy
description: What WM Pass collects (nothing), what it sends when you turn a network feature on, and what it does with access to your Google Drive, Dropbox, OneDrive or Box.
---

Last updated 3 November 2026.

WM Pass is a password manager that keeps its vault on your own devices. It has no account to make and no server of ours that your data passes through. The developer does not collect, receive or sell anything about you.

## What stays on your device

Your vault, its settings and the places you back up to are stored on your device, encrypted. WM Pass has no analytics, no crash reporting and no advertising.

## When something leaves your device

Every network feature starts switched off. When you turn one on, WM Pass talks directly to the service that feature names, and only for that feature. Backups and sync send your vault file as it is stored: encrypted under keys that never leave your device, so the place holding it cannot read your passwords.

## Google Drive, Dropbox, OneDrive and Box

If you choose one of these as a backup or sync place, you sign in on that service's own page, so WM Pass never sees your password for it. The service hands back a token that WM Pass keeps on your device, encrypted, and uses only to reach that place.

- **Google Drive.** WM Pass asks for `drive.appdata`, a hidden folder only WM Pass can see, or `drive.file` if you choose a visible folder, which reaches only files WM Pass created. It cannot see the rest of your Drive.
- **Dropbox.** WM Pass reads and writes files in the folder you point it at.
- **OneDrive.** WM Pass asks for `Files.ReadWrite.AppFolder`, its own app folder, and nothing else in your OneDrive.
- **Box.** WM Pass reads and writes files in the folder you point it at.

With that access WM Pass writes encrypted backups, lists them, reads them back to check or restore them, and deletes old ones you no longer keep. It does not read other files, and the data it handles is not sent anywhere else.

WM Pass's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements. Data from Google is not used for advertising, not sold, not read by people and not used to train models.

To revoke access, remove the place in WM Pass, then remove WM Pass from your account's connected apps on the service. Deleting the backup files from the service removes the data itself.

## Children

WM Pass is not aimed at children and collects nothing from anyone.

## Changes and contact

Changes to this policy are published on this page with a new date. Questions go to the [issue tracker on GitHub](https://github.com/wasi-master/wm-pass/issues).
