---
title: Install
description: Where to get WM Pass on each platform, and how to check a desktop download is really ours.
sidebar:
  order: 2
---

## Android

Download the APK from [GitHub releases](https://github.com/wasi-master/wm-pass/releases). F-Droid and Play listings will follow.

## Desktop

Downloads for macOS, Windows and Linux are on the same page. On a Mac with Homebrew, and on Arch through the AUR:

```bash
brew install --cask wm-pass
```

```bash
yay -S wm-pass-bin
```

### Checking a download

Desktop builds carry no Apple or Windows signature, so macOS and Windows warn the first time you open them. That also means the warning cannot tell you the file is genuine, so check it yourself. Every release has a `SHA256SUMS` file, and updater files carry a minisign signature against the public key in the app. Later updates verify that signature automatically and refuse a file that fails.

On macOS, open the app with right-click, then Open. On Windows, choose More info, then Run anyway.

## Browser

Load the extension from its release zip, or from the store once it is listed. See [Browser extension](/apps/extension/).
