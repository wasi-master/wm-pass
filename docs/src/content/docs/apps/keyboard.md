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
