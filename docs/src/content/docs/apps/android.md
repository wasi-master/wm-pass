---
title: Android
description: The Android app, with its autofill service, passkeys, widgets, Quick Settings tile and panic controls.
---

The Android app is the fullest version of WM Pass.

## Autofill

Set WM Pass as your autofill service in Settings. It matches apps by package name and signing certificate, not by name alone, and matches browsers by web address only for browsers it knows. A login never fills without your tap, and a WebView inside another app asks first and names what it is filling.

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
