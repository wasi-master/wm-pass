---
title: WM Keyboard
description: Browse, type and add to your vault from WM Keyboard's WM Pass tool, get WM Pass's own buttons on the keyboard's strip, and keep the keyboard's API keys in your vault.
---

If you use [WM Keyboard](https://wmkeyboard.pages.dev), its WM Pass tool works as a password book on top of this app: it lists your vault, types from it, adds logins to it and changes passwords in it. Its code chip can offer the current code when you reach a code box.

**Let WM Keyboard fill**, under Settings, Security, starts on when WM Keyboard is installed and its signing certificate is WM Keyboard's own. Turn it off there and it stays off. The keyboard gets answers only while your vault is open, and it can open a locked vault only with you: your fingerprint, which WM Pass asks for, or your master password. Each app checks the other's signing certificate first.

A password typed this way passes through the keyboard's process, as anything you type does. If you would rather keep it out of there, use the autofill service instead.

## The WM Pass tool

The tool lists every item in your vault that is not in the trash or on the hidden shelf, logins, cards, notes, Wi-Fi networks and the rest. Those saved for the app in front come first. Its search uses the vault's own search.

- Open an item to see its fields. A plain field, such as a username or a website, shows as it is. A secret, such as a password or a card number, is only named until you tap it.
- **Type** puts one field into the box you are typing in. **Show**, beside a secret, shows it for ten seconds. Either way WM Pass sends that one value to the keyboard, and only when you tap.
- **Fill**, on a login, types the username, moves to the next box and types the password there, for a sign-in form in one tap.
- **Copy** asks WM Pass to copy the field. WM Pass puts it on the clipboard itself, marked sensitive and cleared after the time set under Settings, Security, so a copied secret never passes through the keyboard.
- **Open in WM Pass** opens the item here, once the vault is open.
- **Add a login** saves a name, username, password and website. A login added while an app is in front is tied to that app by its signing certificate, so it comes first there next time. In a browser it is not, since one browser shows every site: give it the website instead.
- **Change password** puts a new password on an item that has one. The old password stays in the item's history.
- **New password**, in either form, makes a password with the generator as you set it up here.

The keyboard types and copies nothing into a field that asks it not to learn, such as a private browser tab.

## Unlocking from the keyboard

While the vault is locked, the tool says so and offers two ways in. Neither takes you out of the app you are typing in.

- **Fingerprint** shows WM Pass's fingerprint prompt over that app, the same one autofill uses. The keyboard only starts it, and WM Pass checks the fingerprint. Close the prompt and you are back where you were; tap **Use password** and the door opens there instead. Without fingerprint unlock set up, the button reads **Unlock in WM Pass** and opens the door straight away.
- **Password** (**Type password** when there is no fingerprint) takes your master password in the keyboard's own box. The keyboard sends it to WM Pass and keeps nothing, and it never reaches the app's field. Wrong tries count toward the same waits as at the door, and a duress password does what it does there. A vault that also needs a key file or a hardware key opens at the door.

Once the vault is open, it locks after the time set under [Lock after leaving the app](/guide/settings/security/#setting-lock-after), counted from then and never sooner than a minute, since you are still typing in the other app. **Lock** in the tool's header locks it at once.

## Buttons on the keyboard's strip

While you type in some of WM Pass's own fields, WM Keyboard offers the buttons beside that field as chips above its keys. A tap on a chip does exactly what the button does.

- A card's number offers **Scan the card**. So do the name on the card and the expiry date when the card scanner is Google's, since only that one reads them.
- A Wi-Fi network's name and password offer **Fill in from a QR code** and **Pick a network saved on this phone**.

The fields tell the keyboard which buttons they have, never what they hold. Each screen names its fields afresh, and the keyboard sends that name back with the tap, so a tap meant for a screen that has closed does nothing. This works whether or not **Let WM Keyboard fill** is on, because nothing from the vault goes to the keyboard.

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
