---
title: Browser extension
description: The Chrome and Firefox extension, how it fills logins, and how it connects to the desktop app.
---

The extension runs the same core as the other apps, as WebAssembly. It can hold its own vault, use the one open in the desktop app, or both. Settings, Import reads other apps' exports, as the [import guide](/guides/import-export/) describes.

## Filling

Click into a login field and a small key appears at its edge. Click that, then choose a login. Nothing fills when a page loads. The menu sits where the page's scripts cannot read or press it, and it ignores clicks that come too soon or land on a page that has faded or covered things, which is how sites try to steal a click.

If the form is inside a frame from another site, the menu tells you before you choose.

## Saving

After you sign in somewhere new, or with a changed password, a small bar offers to save or update it. Turn this off under Settings, Desktop app.

## Passkeys

Sites can create and use passkeys held in your vault. A window asks you first, and the site is told you were verified only if you typed your master password there. If you have no passkey for that site, the browser's own handling carries on as usual.

## Connecting to the desktop app

1. In the app, open Settings, Browser extension. Turn on **Let the extension connect**, then choose **Set up browsers**.
2. In the extension, open Settings, Desktop app, and choose **Connect**.
3. Both show a six-digit code. If they match, say so on both.

From then on the extension uses the vault open in the app, so you unlock once. The first fill on each site asks in the app, unless you turn that off. The relay program in between only moves sealed bytes and holds no keys.

While connected, the app also does what a browser cannot for the extension's own vault. Backups and sync can go to SFTP, FTP, SMB, NFS, IMAP and iCloud Drive through the app, which asks you once for each place. A YubiKey's challenge-response slot, and a security key set up by an older version of the app, work through it too, after one question.

## The full app in a tab

Click the arrow at the top of the popup to open WM Pass in a tab: the same screens and settings as the desktop app. The popup is the quick search. Type to search the whole vault, use the arrow keys, and press Enter to fill the page or copy the password; Mod+Enter copies the username.

Network features work from the extension itself once you allow them: breach checks, aliases, the disposable inbox, the username checker, and Bitwarden or Vaultwarden accounts. Their requests leave from the extension's background, never from a page you are visiting.

A security key works through the browser, and one you add here works in every WM Pass app. Under Settings, Security, **Unlock with a passkey** opens the vault with Touch ID, Windows Hello or your phone. In Chrome and Edge, a backup place can be a folder on this computer, and a KeePass file can stay where it is and be written back in place.

## What it cannot do

It cannot keep the SSH agent, type into other apps, or show widgets: those belong to the desktop app. Syncing directly with a phone or computer on your network is the desktop and phone apps' too; the extension's vault reaches them through a backup place. TikTok and Instagram answer only a browser that runs their scripts, so the username checker asks the desktop app for those two while it is connected.

Self-hosted servers are reached through the browser, which cannot pin a certificate the way the apps do.

The browser can stop the extension's background at any quiet moment. Your main vault comes back open, since its key stays in the browser's memory until you lock or close the browser, but a nested vault locks.
