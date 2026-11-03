---
title: Browser extension
description: The Chrome and Firefox extension, how it fills logins, and how it connects to the desktop app.
---

The extension runs the same core as the other apps, as WebAssembly. It can hold its own vault, use the one open in the desktop app, or both.

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

## What it cannot do

Breach checks, aliases and other network features are in the desktop and phone apps only. Hardware keys are not available in the extension's own vault; connect to the desktop app to use one. Backups and sync work with WebDAV, S3, Google Drive, Dropbox, OneDrive and Box; a browser cannot open the raw connections SFTP, SMB or NFS need, so those stay with the desktop and phone apps.

The browser can stop the extension's background at any quiet moment. Your main vault comes back open, since its key stays in the browser's memory until you lock or close the browser, but a nested vault locks.
