---
title: Import and export
description: Bring your passwords in from other password managers and authenticator apps, or take them out again.
---

Under Settings, Import, choose your old app's export file.

**Password managers:** KeePass (KDBX 4), Bitwarden (JSON and CSV), 1Password (1PUX), LastPass, Dashlane, Enpass, NordPass, Keeper, Proton Pass, Apple Passwords, SafeInCloud, RoboForm, Chrome, Firefox, KeePassXC, and any other CSV by choosing which column is which.

**Authenticators:** Aegis, 2FAS, andOTP, FreeOTP+, Raivo, Ente Auth, and Google Authenticator's migration QR code.

These importers were tested against sample files written by hand from each format's published or observed layout, not against exports from every real app. Compare the count afterwards, and tell us if anything is missing.

Export writes `.wmvault`, KDBX, Bitwarden JSON, CSV or a printable HTML page. CSV and HTML are plain text, so delete them once you are done.
