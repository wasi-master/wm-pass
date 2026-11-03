---
title: WM Keyboard
description: Type logins and one-time codes from WM Pass through WM Keyboard's WM Pass tool, and keep the keyboard's API keys in your vault.
---

If you use [WM Keyboard](https://wmkeyboard.pages.dev), its WM Pass tool can type logins from this app, and its code chip can offer the current code when you reach a code box.

**Let WM Keyboard fill**, under Settings, Security, starts on when WM Keyboard is installed and its signing certificate is WM Keyboard's own. Turn it off there and it stays off. The keyboard can only ask while your vault is open, and it can never unlock it. Each app checks the other's signing certificate first.

A password typed this way passes through the keyboard's process, as anything you type does. If you would rather keep it out of there, use the autofill service instead.

## API keys

The key fields in WM Keyboard's settings (translation, AI and search services, smart home tokens, the GitHub token and others) get a WM Pass button once your vault exists and **Let WM Keyboard fill** is on.

- **Fill from WM Pass** lists your API key items, with the ones named for that service first, and puts the key you pick into the field.
- **Save in WM Pass** keeps what the field holds as an API key item named for the service. If an API key item of that name is already there, its key is replaced and the old one stays in the item's history. A service with a key and a secret ends up as one item that holds both.

If the vault is locked, the button opens WM Pass so you can unlock it, then you go back and press it again.

## Places for copies

Both apps can keep copies in the same kinds of place: a folder, a cloud account or a server. A place set up in one can be picked in the other, so its address and password are typed only once.

- In WM Pass, **Add a place** has **Use a place from WM Keyboard**. WM Keyboard opens its list of places, behind its own app lock if you set one, and you tap one. The place fills in the sheet here; look it over and save it.
- In WM Keyboard, **Add a location** has **Use a place from WM Pass**. WM Pass opens a page that waits until your vault is open, then lists your places and sends only the one you tap.

A login a server checks comes across whole: a WebDAV, FTP, SFTP, SMB, email or NAS password, an SFTP key, an S3 key pair, a Git token. A sign-in does not. Google Drive, Dropbox, OneDrive and Box sign each app in separately, and a token from one does nothing in the other. Box and Proton Drive replace their tokens on every use, so two apps sharing one would sign each other out, and pCloud, MEGA and iCloud Drive keep sessions too. The app that receives one of those asks you to sign in. A folder on the phone comes without the folder, because Android lets an app into a folder only when you pick it there.

Nothing crosses until you tap a place on the other app's page. Each app answers only the other, checked by its signing certificate, and the place goes straight from one app's memory to the other's. From then on each app keeps its own copy, and changing or removing it in one leaves the other alone.

## Clipboard

When WM Pass clears a copied secret from the clipboard, WM Keyboard removes it from its clipboard history at the same moment. If you copy something else first, WM Pass leaves the clipboard alone, and the copy in the keyboard's history follows the keyboard's own rules.
