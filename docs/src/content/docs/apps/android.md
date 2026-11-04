---
title: Android
description: The Android app, with its autofill service, passkeys, widgets, Quick Settings tile and panic controls.
---

The Android app is the fullest version of WM Pass.

## Moving from another app

Settings, Import reads exports from other password managers and authenticators, and scans Google Authenticator's transfer codes. **How to export from your old app** lists each app and opens its steps in WM Pass, with a tab for each device, Android first. The same pages are [on this site](/guides/import-export/#exporting-from-your-old-app), with their pictures.

**Import into** picks where the items go: this vault, or one of the vaults nested inside it.

Exports write an app in several ways: `androidapp://com.example.app`, the `android://<certificate>@com.example.app/` form Google Password Manager uses, or a bare package name typed where a website goes. WM Pass reads all three as apps, so they fill in that app and are listed under the item's apps, not as a website. A bare name counts as a package when it is installed on the phone, or when it starts the way package names do (`com.`, `org.`, `io.` and the like) and does not end in a common web ending such as `.com`. The guess can be wrong for an odd name, and the editor shows which way it went.

## Autofill

Set WM Pass as your autofill service in Settings, or tap **Turn on** on the card the vault list shows until you do. It matches apps by package name and signing certificate, not by name alone, and matches browsers by web address only for browsers it knows. A login never fills without your tap, and a WebView inside another app asks first and names what it is filling.

With inline suggestions on, saved logins also show in the suggestion row of any keyboard that has one, such as Gboard or WM Keyboard, and not only in a dropdown under the field.

A login can name the apps it is for as well as its websites. In the editor, open **Websites and apps** and tap **Add an app** to pick from the apps on your phone. WM Pass saves the app's package name and signing certificate, so a different app wearing the same name does not match.

When WM Pass offers to save a login you just typed somewhere, the name, username, password and website can all be changed before it is kept. A login typed into an app with no website of its own gets the sites that go with that app offered, the first one filled in, and is linked to the app as well.

The name, the websites and the apps suggest each other. Type facebook.com and **Goes with this login** offers Facebook, Facebook Lite and Messenger if they are installed, messenger.com, and Facebook as the name; start from the name or from an app and the rest follow the same way. The families come from Apple's list of sites that share one sign-in, with a few of WM Pass's own added.

## Adding and editing items

The editor's sections start folded, each with a line saying what is inside, so a long item opens short. Settings, Editor can keep open the ones you opened last time.

A new item starts with the fields its kind needs. Everything else waits behind **Add more**: a one-time code, notes, text or secret fields, security questions, an app, a file, a link to another item, and a reminder to change the password. Files and links chosen before the first save are kept with the item when you save it. A code can take a username and password too, and becomes a login that keeps its code.

Some fields offer common values as you type, such as a database's engine or an API key's provider.

For Wi-Fi, tap **Fill in from a QR code** and scan the code Android's Wi-Fi settings show when you share a network, or share that code's text to WM Pass. Typing a password on an open network switches its security to WPA, and clearing the password switches it back to open.

**Pick a network saved on this phone** fills the item from a network Android already knows, password included, and Import has **Wi-Fi saved on this phone** to add several at once. Android keeps those passwords from apps, so this goes through [Shizuku](https://shizuku.rikka.app/), which you install and start yourself and which asks once whether WM Pass may use it. WM Pass uses it only to read saved networks, and only when you open one of these. The way of asking comes from [wifi-password-manager](https://github.com/Khh-vu/wifi-password-manager).

A Wi-Fi item's page has two more buttons beside **Copy password**. **QR code** shows the same code Android's Wi-Fi settings show, so another phone's camera can join the network; the password is in the code, so anyone who can see it can read it. Enterprise networks have no such code. **Add to this phone**, on Android 11 and later, opens Android's own page for saving a network. Android asks before it saves anything, keeps the network with the ones you typed in yourself, and joins it when it is in range. It takes WPA, WPA3 and open networks; a WEP network, or a password written as 64 hex digits, has to go through Android's Wi-Fi settings.

For a payment card, tap **Scan the card** and hold the card inside the frame, or tap the picture button to read a photo of the card you already have. The number fills in once three camera frames agree on it and it passes the checksum every card number carries, and it stays visible so you can compare it with the card. Two readers can do this, and both run on the phone:

- **WM Pass's own reader** is Stripe's CardScan digit model (MIT), built into every build. It sends nothing to anyone, but it was trained on card numbers alone, so you type the expiry date and the name yourself.
- **Google's reader** is ML Kit's text recognizer, which Play services provides. It reads the number, the expiry date and the name. The picture and what it reads stay on the phone, though Google's library sends Google statistics about how it runs, with details of the phone and app. Play services fetches it the first time, and until then the scan falls back to WM Pass's own reader.

The standard build asks which one to use before the first scan, and you can change it later under Settings, Editor, **Card scanner**. The F-Droid and offline builds carry nothing of Google's, so they have only WM Pass's own reader. No frame is kept or sent anywhere.

## Tags

Tags show as chips on the item's page. The cross on a chip takes the tag off that item. Holding a chip renames the tag, gives it an icon or deletes it, on every item that has it. Below them, WM Pass suggests tags from the site, such as Banking or Social, and for a code also Authenticator.

A new item saved without tags gets the one its site suggests. Turn this off under Settings, Editor, **Tag new items**.

**Add a tag** lists the item's own tags and the ones you use most, with a search and a button for the rest.

Tag names and their icons are stored inside the encrypted vault.

## Icons

The picture beside each item comes from the sources under Settings, Icons, **Pictures, in order**, tried from the top: the brand marks that ship with the app, then the icon of the app an item is for if it is installed. **Website icons** can be added to the order; they are fetched once per site, from the site itself or from DuckDuckGo, kept encrypted on the phone, and never asked for local addresses. An item none of them covers shows its first letter on a soft color of its own.

The item's page lists **Websites and apps** together, each app with its name and icon, and opens an installed app when you tap it. For a login it also has **Change the password**, which opens the site's own page for that.

The icon picker shows what Automatic would draw, then every symbol and every brand in one grid that fills the screen's width. Search narrows both. Hold an icon to see its name and where it comes from: symbols come from the icon style you picked, and brand marks come from Simple Icons (CC0) and belong to their owners.

Settings, Icons has eight styles for the app's own symbols: four versions of Material Symbols (Apache License 2.0), Phosphor in regular, bold and light (MIT), and Lucide (ISC). All of them are in the app, so switching needs no download. Lucide is drawn in lines only, so a selected tab keeps its outline instead of filling in.

## Throwaway addresses and usernames

Under Settings, Tools, **Disposable inbox** makes throwaway email addresses for sign-ups that only need a confirmation mail. Each address stays in a list, kept inside the encrypted vault, until you close it, so leaving the page does not lose it. Addresses can come from mail.tm, mail.gw or Guerrilla Mail, from your own server running the mail.tm API, or from an [Inbucket](https://inbucket.org/) server you run. Opening an address shows its mail, checked every few seconds while the page is open.

A mail opens the way a mail app shows it, with its layout and links, but no script in it runs, and pictures stay unloaded until you tap **Load**, so the sender cannot see that you opened it. The page is fitted to the phone's width, as a mail app would show it. A code in the mail gets its own copy button. The share button forwards the mail to another app, and the menu copies its text or saves it as a file; it can also be saved to the vault as a note. Closing an address can also delete it on the service. The service, and possibly others, can read what arrives at these addresses: never use one for an account you want to keep.

**Username checker** asks every site at once whether a name is taken, and shows how far it has got. The sites are listed with the most used first. TikTok and Instagram only answer a browser, so they are read one after the other in a hidden browser view. A site that fails, for instance because it got too many requests, says so on its own row and can be asked again. Tapping a site opens that name's page there, and sites that give no readable answer are listed under **Look for yourself**.

## Passkeys

On Android 14 and up, WM Pass can be your passkey provider. Apps other than known browsers get a confirmation that names them, in place of a check against the site's published app list.

A site adds a passkey itself, when you create one there with WM Pass chosen as the provider. The editor's **Passkeys** section lists the ones a login has, with the site and the account each is for, but cannot add or change one: its private half never leaves WM Pass.

## Other ways in

- A Quick Settings tile that locks everything at once.
- A home screen widget for one-tap codes.
- App shortcuts, and a share target for adding things to your vault.
- Panic support, so Ripple and similar apps can lock or wipe it.

## Panic apps

Settings, Security, **When a panic app signals** starts at None, so no other app can act on WM Pass until you choose. **Lock** locks it when any panic app signals, or only the ones you pick under **Panic apps**. **Wipe** deletes the vault on this phone, and only for an app you picked, so choosing it with none picked opens the list. Apps known to send panic signals, such as Ripple and Panic Button, come first in the list.

## The vault list

The list opens on types, then folders, the way Bitwarden's vault does: a row for each type of item you have (logins, cards, notes and the rest) with its count, then a row for each folder and **No folder**. Each row opens in place. An item shows under its type and under its folder, and tapping a type chip above the list shows that type as one plain list. Settings, Vault list, **Grouping** changes this to **By site**, one list with several logins for one site folded into a row, or to **By folder**. Logins listed under an open site, type or folder are set in a little, so they read as inside it.

A scroll bar runs down the right edge of the vault list and the Health page. Drag it to move through a long list fast; while it is held, a label says which letter or part you are in.

Hold an item to start choosing several. With two or more chosen, **More**, **Link together** links each to the others, so every one lists the rest under **Linked**.

**Compact** fits more rows on screen, with a small icon and the second line beside the name.

## Health and breaches

The vault list's health row can be put away until something new needs attention, or for good. On the Health page, **Used more than once** groups logins by the password they share, and tapping a login offers **Change the password** on its site, **Ignore for this check**, or opening it. Settings, Health checks, **Leave out local addresses** keeps routers, printers and other devices on your own network out of the checks, and is on to start with.

Settings, Breaches checks every password against Have I Been Pwned or XposedOrNot. The check keeps running after you leave the page, with a notification and a **Stop** button, and the page says when it last ran and lets you sort what it found by how often each password was seen.

## Settings search

The search button on the Settings page turns its bar into a search field. Each result is a card with the icon of the page it opens and where it lives, and the words you typed in bold. Back out of a result and the same results are still there.

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
