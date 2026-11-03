---
title: Android
description: The Android app, with its autofill service, passkeys, widgets, Quick Settings tile and panic controls.
---

The Android app is the fullest version of WM Pass.

## Autofill

Set WM Pass as your autofill service in Settings, or tap **Turn on** on the card the vault list shows until you do. It matches apps by package name and signing certificate, not by name alone, and matches browsers by web address only for browsers it knows. A login never fills without your tap, and a WebView inside another app asks first and names what it is filling.

A login can name the apps it is for as well as its websites. In the editor, open **Websites and apps** and tap **Add an app** to pick from the apps on your phone. WM Pass saves the app's package name and signing certificate, so a different app wearing the same name does not match.

## Adding and editing items

A new item starts with the fields its kind needs. Everything else waits behind **Add more**: a one-time code, notes, text or secret fields, security questions, an app, a file, a link to another item, and a reminder to change the password. Files and links chosen before the first save are kept with the item when you save it. A code can take a username and password too, and becomes a login that keeps its code.

Some fields offer common values as you type, such as a database's engine or an API key's provider.

For Wi-Fi, tap **Fill in from a QR code** and scan the code Android's Wi-Fi settings show when you share a network, or share that code's text to WM Pass. Typing a password on an open network switches its security to WPA.

**Pick a network saved on this phone** fills the item from a network Android already knows, password included, and Import has **Wi-Fi saved on this phone** to add several at once. Android keeps those passwords from apps, so this goes through [Shizuku](https://shizuku.rikka.app/), which you install and start yourself and which asks once whether WM Pass may use it. WM Pass uses it only to read saved networks, and only when you open one of these. The way of asking comes from [wifi-password-manager](https://github.com/Khh-vu/wifi-password-manager).

## Tags

Tags show as chips on the item's page. The cross on a chip takes the tag off that item. Holding a chip renames the tag, gives it an icon or deletes it, on every item that has it. Below them, WM Pass suggests tags from the site, such as Banking or Social, and for a code also Authenticator.

A new item saved without tags gets the one its site suggests. Turn this off under Settings, Editor, **Tag new items**.

Tag names and their icons are stored inside the encrypted vault.

## Icons

The icon picker shows what Automatic would draw, then every symbol and every brand in one grid that fills the screen's width. Search narrows both. Hold an icon to see its name and where it comes from: symbols are Material Symbols (Apache License 2.0), and brand marks come from Simple Icons (CC0) and belong to their owners.

## Passkeys

On Android 14 and up, WM Pass can be your passkey provider. Apps other than known browsers get a confirmation that names them, in place of a check against the site's published app list.

## Other ways in

- A Quick Settings tile that locks everything at once.
- A home screen widget for one-tap codes.
- App shortcuts, and a share target for adding things to your vault.
- Panic support, so Ripple and similar apps can lock or wipe it.

## QR codes

Wherever WM Pass asks you to scan a code, you can pick a picture of it instead, such as a screenshot. The picture is read on the phone through Android's photo picker, which shares only the one you choose, and WM Pass keeps nothing of it.

## Screenshots

Screenshots and screen recording are blocked by default. You can change this under Settings, Security.

## Accessibility services

A password field you have not shown gives screen readers and other accessibility services a row of dots, as Android's own password fields do. Once you tap Show, the text is read like anything else on screen.

## If the app keeps closing

If WM Pass closes by itself within a few seconds of opening, twice in a row, the next launch puts Appearance back to the standard style, theme, icons and list layout, and the unlock screen says why. Your vault and your other settings stay as they were. Leaving the app, locking it or swiping it away never counts, and you never need to clear the app's storage, which would delete the vault.

## The build without internet

An offline build drops the internet permission, so no line of code can reach a network. It cannot sync over your local network either, since that needs the same permission.
