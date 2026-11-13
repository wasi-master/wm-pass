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

On macOS 14 and earlier, open the app with right-click, then Open. On macOS 15 and later, try to open it once, then choose Open Anyway under System Settings, Privacy & Security. The Homebrew cask clears this warning for you. On Windows, choose More info, then Run anyway.

## Terminal

`wmpass`, the terminal app and command line, has its own downloads for macOS, Linux and Windows on the releases page, under tags that start with `cli-v`, with a `SHA256SUMS` file beside them. See [Terminal](/apps/terminal/).

## Browser

The extension's downloads are on the releases page under tags that start with `extension-v`. In Chrome, Edge or Brave, unzip `wm-pass-chrome.zip` and load the folder with Load unpacked on the browser's extensions page, with developer mode on. Firefox installs the signed `wm-pass-firefox.xpi`. See [Browser extension](/apps/extension/).
