---
title: Import and export
description: Bring your passwords in from other password managers and authenticator apps, or take them out again.
---

Under Settings, Import, choose your old app's export file. For Google Authenticator, tap Scan Google Authenticator's code instead, or choose a screenshot of the code as the file.

**Password managers:** KeePass (KDBX 4), Bitwarden (JSON and CSV), 1Password (1PUX), LastPass, Dashlane, Enpass, NordPass, Keeper, Proton Pass, Apple Passwords, SafeInCloud, RoboForm, Chrome, Firefox, KeePassXC, and any other CSV by choosing which column is which.

**Authenticators:** Aegis, 2FAS, andOTP, FreeOTP+, Raivo, Ente Auth, and Google Authenticator's migration QR code.

These importers were tested against sample files written by hand from each format's published or observed layout, not against exports from every real app. Compare the count afterwards, and tell us if anything is missing.

Export writes `.wmvault`, KDBX, Bitwarden JSON, CSV or a printable HTML page. CSV and HTML are plain text, so delete them once you are done.

## Exporting from your old app

Each link below goes to that app's own guide, which has pictures of every step. Menus move between versions, so if a step does not match what you see, follow the guide. Most exports are plain text: delete the file once WM Pass has read it.

### Password managers

- **1Password**, from the desktop app only. On a Mac, File, Export, then the account; on Windows and Linux, the ⋯ menu in the sidebar, then Export. Choose 1PUX, since CSV leaves custom fields out. [1Password's guide](https://support.1password.com/export/)
- **Apple Passwords**. On a Mac, in the Passwords app, File, Export All Passwords to File. On an iPhone, Settings, Apps, Safari, Export, with only Passwords selected. Wi-Fi passwords and passwords shared in groups are not included. [Mac guide](https://support.apple.com/guide/passwords/export-passwords-mchl35b12625/mac), [iPhone guide](https://support.apple.com/guide/iphone/export-passwords-iphf28f2e93e/ios)
- **Bitwarden**. In the web vault, Tools, Export; in the app, Settings, Vault, Export vault. Choose `.json`, not `.json (Encrypted)`. CSV leaves out cards, identities and passkeys. [Bitwarden's guide](https://bitwarden.com/help/export-your-data/)
- **Chrome and Google Password Manager**. On a computer, ⋮, Passwords and autofill, Google Password Manager, Settings, Export passwords. On Android, Settings, Autofill and passwords, Google Password Manager, Settings, Export passwords & passkeys. [Google's guide](https://support.google.com/chrome/answer/13068232)
- **Dashlane**, from the web app: Settings, Export data, Export to CSV. It gives a ZIP of several CSV files; choose the one named credentials. The DASH format is encrypted and cannot be read. [Dashlane's guide](https://support.dashlane.com/hc/en-us/articles/32905278138002-Export-your-Dashlane-data-to-a-CSV)
- **Enpass**, from the desktop app: File, Export, choose the vault, then `.json`. [Enpass's guide](https://help.enpass.io/personal/latest/desktop/importing-from-enpass)
- **Firefox**. Menu, Passwords, then ⋯, Export Passwords. [Mozilla's guide](https://support.mozilla.org/en-US/kb/export-login-data-firefox)
- **KeePass and KeePassXC** need no export. Choose the `.kdbx` database itself, wherever you saved it, and enter its password. [KeePassXC's guide](https://keepassxc.org/docs/KeePassXC_UserGuide#_storing_your_database)
- **Keeper**, from the web vault or desktop app: Settings, Export, then JSON or CSV. A company account's admin can turn export off. [Keeper's guide](https://docs.keeper.io/en/user-guides/export-and-reports/vault-export)
- **LastPass**, from the browser extension only: Account, Fix a problem yourself, Export vault items, Export data for use anywhere. It leaves out attachments and one-time code secrets. [LastPass's guide](https://support.lastpass.com/s/document-item?language=en_US&bundleId=lastpass&topicId=LastPass%2Fexport-generic-csv.html)
- **NordPass**, from the desktop app or web vault: Settings, Import and Export, Export. [NordPass's guide](https://support.nordpass.com/hc/en-us/articles/360007646477-How-to-export-passwords-from-NordPass)
- **Proton Pass**, from the web app, browser extension or Windows app, not the phone app: Settings, Export. Choose CSV or the unencrypted ZIP, not the PGP-encrypted one. [Proton's guide](https://proton.me/support/pass-export)
- **RoboForm**, from the browser extension: ⋯, Settings, Account & Data, Export. [RoboForm's guide](https://help.roboform.com/hc/en-us/articles/230425008-How-to-export-your-RoboForm-logins-into-a-CSV-file)
- **SafeInCloud**, from the desktop app only, as XML. Its guide names the formats but not the menu. [SafeInCloud's notes](https://kb.safe-in-cloud.com/en/troubleshooting/import-export-issues.html)

### Authenticators

- **Google Authenticator**: Menu, Transfer codes, Export codes, choose the accounts, Next. A long list comes as several QR codes. In WM Pass, tap Scan Google Authenticator's code and scan each one in turn, then import them together. A screenshot of each code works too, if your phone allows one there. [Google's guide](https://support.google.com/accounts/answer/1066447)
- **2FAS**: Settings, 2FAS Backup, Export. WM Pass reads the backup only without a password, so leave that blank. [2FAS's guide](https://2fas.com/support/2fas-auth-mobile-app/i-want-to-move-copy-transfer-tokens-codes-between-ios-and-android/)
- **Ente Auth**: Settings, Data, Export, then Plain text. Ente's guide shows the encrypted export; the plain one sits beside it. [Ente's guide](https://ente.com/help/auth/migration/export)
- **Aegis**: export from its settings with encryption turned off. Aegis has no export guide of its own. [Aegis](https://github.com/beemdevelopment/Aegis)
- **andOTP**: make a plain-text JSON backup from its backup menu. andOTP is no longer maintained and has no export guide. [andOTP](https://github.com/andOTP/andOTP)
- **FreeOTP+**: export from its settings to a file. [FreeOTP+](https://github.com/helloworld1/FreeOTPPlus)
- **Raivo** exports only a password-protected ZIP. Open it with the password and choose the JSON file inside. [Raivo](https://github.com/raivo-otp/ios-application)
