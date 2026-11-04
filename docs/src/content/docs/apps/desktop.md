---
title: Desktop
description: The desktop app for macOS, Windows and Linux, with quick search, auto-type, an SSH agent, a tray icon and widgets on a Mac.
---

The desktop app has the same vault, screens and settings as the phone, laid out for a larger window. To bring passwords over from another app, see [Import and export](/guides/import-export/); most password managers export only from a computer.

## Quick search

A global shortcut, which you choose under Settings, Desktop, opens a small search window from anywhere. Enter copies the password, and Mod+Enter copies the username.

## Auto-type

Off by default. When on, Shift+Enter in quick search types the login into whichever window was in front. Turn this on only if you need it: typed text goes through the system's input queue, where other programs can see it.

## SSH agent

Under Settings, SSH agent, WM Pass can answer `ssh` and `git` with Ed25519 keys from your vault. It asks before each use unless you turn that off, and when the vault locks it has nothing left to offer.

```bash
export SSH_AUTH_SOCK=~/.wm-pass/ssh-agent.sock
```

Settings shows the exact path for your system.

## Widgets on a Mac

On macOS 14 and later, WM Pass has widgets for the desktop and Notification Centre: a quick bar, the lock status, health, backups and a generator. With **Show on widgets** turned on under Settings, Security, a Codes widget and a Favorites widget show codes and favorites while the vault is open, and nothing once it locks. A click on a widget brings WM Pass forward for a moment; for Lock and Generator it does what you asked and hides again. What each one shows is under [Widgets](/guide/how-it-works/#feature-home-widget).

## In the tray

Closing the window can leave WM Pass in the tray, where it still locks on time. The lock timer runs in the app itself, because a window hidden behind others can be paused by the system.

## Clipboard

Copied passwords are marked to stay out of clipboard history and are cleared after 60 seconds, or the time set under Settings, Security, unless you have copied something else by then.
