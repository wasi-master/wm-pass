---
title: Your first vault
description: Make a vault, choose a password you will remember, and put a second way in place before you need it.
sidebar:
  order: 1
---

Open WM Pass and choose a master password. Nothing can recover it: there is no server and no reset link. The strength meter under the box is honest, so take a passphrase of five or more random words if it complains.

## Before you add much

Three things take a few minutes now and matter a lot later.

**Turn on backups.** A lost phone with no backup is a lost vault. See [Backups and sync](/guides/backup-sync/).

**Print an emergency kit.** Under Settings, Security, you can split a recovery key between people you trust, so that any few of them together can open the vault if you cannot. See [Emergency access](/guides/emergency-access/).

**Add a second factor if you own a security key.** A key file, a FIDO2 key or a YubiKey can be required alongside the password.

## Unlocking faster

On Android, fingerprint unlock wraps the vault key in the phone's secure hardware, and a new fingerprint on the phone invalidates it. On Linux, the keyring can hold it, though any program in your session could ask for it too, so it is off until you choose it. The macOS and Windows builds have no biometric unlock; on a Mac, the Secure Enclave needs a signed app and these builds are unsigned. In the browser extension and the web vault, **Unlock with a passkey** under Settings, Security opens the vault with Touch ID, Windows Hello or your phone.

## Automatic locking

On a computer the vault locks after five minutes without use, and on a phone a minute after you leave the app. It also locks when the computer sleeps or the phone's screen goes off. Both are under Settings, Security.
