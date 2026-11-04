---
title: Web vault
description: WM Pass in any Chrome, Edge or Firefox tab at wmpass.pages.dev/vault, and how it hands over to the extension.
---

The web vault is the whole app in a browser tab, at [wmpass.pages.dev/vault](https://wmpass.pages.dev/vault/). It runs the same engine as the other apps, as WebAssembly in the page, and keeps its vault in that site's storage in your browser. There is no account, and nothing about your vault is sent to the site. It does not fill logins into other sites; for that, use the [extension](/apps/extension/).

## With the extension installed

The page offers to open your vault in the extension instead, and that is the better choice. The extension's code comes signed from the browser's store and changes only with an update, where the web vault's code is sent by the website on each visit. Choose **Always open the extension from here** to skip the question next time. Firefox does not let a page reach an extension, so there the page never offers.

The two keep separate vaults. To use one vault in both, set up the same backup place in each, or move it with a backup file.

## What it can do

- Everything on the vault's screens: items, codes, health, the generator, import and export.
- Backups and sync to WebDAV, S3, Google Drive, Dropbox, OneDrive and Box, and in Chrome and Edge, to a folder on this computer.
- Network features, once you allow them, where the service lets a web page ask it. Have I Been Pwned's password check does; some alias services do not.
- Security keys through the browser, and **Unlock with a passkey** under Settings, Security.
- Working offline once it has loaded, and installing as an app from the browser's menu.

A WebDAV or S3 server has to allow requests from `https://wmpass.pages.dev` (CORS) before the web vault can reach it. Nextcloud and most S3 services have a setting for this. The extension does not need it.

## Keep a backup

Clearing this site's data in your browser deletes the vault with it. The page asks the browser to keep its storage, but set up a backup place, or save a backup file under Settings, Backups, before you rely on it.

## Security

The page runs under a strict Content-Security-Policy: its own scripts and WebAssembly only, no frames in or out, no forms, and requests only over https or to this computer. It is published only from a release tag, by this repository's own workflow. See the [threat model](/security/threat-model/) for what that does and does not cover.
