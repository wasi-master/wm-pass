---
title: Terminal
description: The full-screen terminal app and the wmpass command line, for macOS, Linux and Windows.
---

`wmpass` is one program with two faces. Run it on its own in a terminal and it opens a full-screen app with your items, one-time codes, the generator and the health report. Give it a subcommand and it does that one thing and exits, which is what scripts want.

```bash
wmpass
```

```bash
wmpass tui ~/Sync/Personal.wmvault
```

With no file named, it opens the WM Pass desktop app's vault when the app is on this computer, and otherwise the vault you used last.

## Getting it

Downloads for macOS, Linux and Windows are on the [releases page](https://github.com/wasi-master/wm-pass/releases), under tags that start with `cli-v`. Each holds a single file to put somewhere on your `PATH`. To build it yourself from a checkout of the repository:

```bash
cargo install --path crates/wmpass-cli
```

## The same vault as the desktop app

On a computer with the WM Pass desktop app, the terminal app opens the app's own vault. The app and the terminal are then working on one vault, with no copy to keep in step. Each one merges in what the other wrote within a few seconds, and a change made in both at once keeps both edits. Settings that follow the vault, such as how long a revealed password stays, reach the terminal the same way. With the app's vault open, the terminal also uses the app's auto-lock and clipboard timers, which never leave this computer.

Wrong passwords count against the app's own limit, whichever one they are typed into. A duress password does what it is set to do in the app.

A vault the terminal made before it knew about the app can move in: run **Keep this vault in the WM Pass app on this computer** from the `:` palette. The old file stays where it was until you delete it.

Subcommands take `@app` in place of a file for the same vault:

```bash
wmpass ls @app
```

The app's folder is `~/Library/Application Support/com.wasimaster.wmpass.desktop` on macOS, `%APPDATA%\com.wasimaster.wmpass.desktop` on Windows and `~/.local/share/com.wasimaster.wmpass.desktop` on Linux. `WMPASS_APP_DIR` points somewhere else.

## Getting your vault from another device

If your vault is on a phone or another computer, there is no need to copy the file by hand. Run `wmpass join` with where to keep it:

```bash
wmpass join ~/vaults/personal.wmvault
```

It opens a full screen with a QR code and waits for two minutes. On the device that has the vault, open Settings, Sync and accounts, and choose **Add a new device**. From there, any of three ways works:

- **Scan its code** with a phone's camera.
- **Find it on this network**, then check that both screens show the same six digits. Click **They match** or press `y`.
- **Paste its code**: press `c` here to copy the code's text, then paste it on the other device.

The vault arrives encrypted, and you open it with its master password. If the code is hard to scan, `z` switches between a big code and a small one. A big code needs a terminal about 45 lines tall. In the full-screen app, `Ctrl+G` at the unlock screen opens the same screen. Without a terminal, as in a script, `wmpass join` prints the same steps as plain lines.

Going the other way, `wmpass add-device` sends a vault to a new device that is waiting to join:

```bash
wmpass add-device ~/vaults/personal.wmvault
```

It lists the devices waiting on this network; pick one and compare six digits, since a terminal cannot scan a code. Or paste the text under the new device's QR code into the box on the same screen. With `--code` and that text, it sends without the full screen. From the full-screen app, it is **Add a new device** in the `:` palette. Both devices need to be on the same network.

## Finding your way

The four tabs along the top are Items, Codes, Generator and Health. Press `1` to `4` to switch, or click them. In Items, the sidebar lists kinds, folders, tags and the trash, the middle shows what matches, and the right side shows the item you picked.

Press `?` for every key, or `:` for a list of commands you can search by name. The ones you will use most:

| Key | Does |
|---|---|
| `/` | Search, with the same words as the apps: `tag:work`, `kind:card`, `has:otp`, `weak` |
| `c` | Copy the password |
| `u` | Copy the username |
| `t` | Copy the one-time code |
| `r` | Show the password for a few seconds |
| `n`, `e` | New item, edit item |
| `d` | Move to the trash; in the trash, `u` brings it back |
| `w` | Show a Wi-Fi network's QR code, for another phone's camera |
| `Ctrl+L` | Lock now |

On a tag in the sidebar, `R` renames it on every item that has it and `D` deletes it from all of them; the items stay. A login saved for an Android app lists the app by its name when WM Pass knows it, such as Messenger for `com.facebook.orca`.

Arrows and `h` `j` `k` `l` both move. The mouse works everywhere too. Click a tab, a row, a field or a command in the `:` list. Double-click a row to open it, or a line in an item to copy it. The key hints along the bottom are buttons. The wheel scrolls whatever is under the pointer, and over a generator setting it changes the value.

## Editing

The editor has the same fields as the phone and desktop apps for every kind. In a password field, `Ctrl+G` fills in a strong one. A secret field opens empty and stays as it was unless you type something, so fixing a title never brings the password onto the screen. `Ctrl+R` shows what you typed, or loads the saved value if you want to change part of it. `Ctrl+S` saves.

Every change is written to the file at once, with the previous version kept beside it as `.bak`. If a sync app changes the file while WM Pass has it open, the two are merged rather than one overwriting the other.

## What stays private

A copied password is kept out of clipboard history where the system allows it, and cleared after 30 seconds unless you have copied something else since. Quitting or locking clears it straight away. On Linux the clipboard only holds what WM Pass copied while WM Pass is running.

Over SSH there is no local clipboard to reach, so copies go through the terminal as an OSC 52 sequence. Most modern terminals pass that to your computer's clipboard; tmux needs `set -g set-clipboard on`.

A shown password hides itself after 20 seconds. The vault locks after 5 minutes without a key press; change that under `:` then Lock after. The app draws on the terminal's alternate screen, so nothing is left in the scrollback when it closes.

## Colours and icons

The default theme follows the WM Pass shield on a dark background. `Ctrl+T` cycles through a light theme, a softer dark one, one that uses only the terminal's own sixteen colours, and one with no colour at all. Terminals without 24-bit colour start on the sixteen-colour theme, and setting `NO_COLOR` turns colour off.

If your terminal uses a [Nerd Font](https://www.nerdfonts.com), WM Pass can draw proper icons for each kind of item, the sidebar and the tabs. Turn them on with `:` then Nerd Font icons, or set `WMPASS_ICONS=nerd`. They are off at first because without such a font every icon shows as an empty box.

## Settings that follow the vault

The theme, the icons and how long a shown secret stays on screen travel inside the vault, so another computer opening the same vault in the terminal picks them up. The time to hide a shown secret is shared with the phone and desktop apps; the theme and icons are the terminal's own. Turn this off with `:` then **Sync settings with the vault on or off**. To keep one of them on this computer only, list its key (`tui_theme`, `tui_icons` or `reveal_seconds`) under `sync_skip` in `tui.json`, in the state folder (`$XDG_STATE_HOME/wmpass`, or `WMPASS_STATE_DIR` when set). The lock and clipboard timers never leave this computer.

## In scripts

Every subcommand reads the password from the terminal. For scripts and tests, `WMPASS_PASSWORD` can supply it, though any other program running as you can read a process's environment. `WMPASS_KEY_FILE` names a key file.

```bash
wmpass ls Personal.wmvault "tag:work has:otp"
```

```bash
wmpass otp Personal.wmvault GitHub
```

```bash
wmpass gen --words
```

`wmpass --help` lists the rest: `create`, `show`, `add`, `rm`, `health`, `import`, `export`, `merge`, `duress`, `shares` and `recover`. When no terminal is attached, `wmpass` on its own prints that list and exits with an error, so a script never waits on a screen nobody sees.

## What it cannot do

The terminal app opens vault files directly. It does not sign in to Bitwarden accounts, open KeePass databases in place, or talk to hardware keys; a vault that needs a security key to unlock has to be opened in the phone or desktop app. As on the command line, a duress password looks like a wrong password here and takes no action.
