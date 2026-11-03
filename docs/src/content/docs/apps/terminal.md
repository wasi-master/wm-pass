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

With no file named, it opens the vault you used last.

## Getting it

Downloads for macOS, Linux and Windows are on the [releases page](https://github.com/wasi-master/wm-pass/releases), under tags that start with `cli-v`. Each holds a single file to put somewhere on your `PATH`. To build it yourself from a checkout of the repository:

```bash
cargo install --path crates/wmpass-cli
```

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
| `Ctrl+L` | Lock now |

Arrows and `h` `j` `k` `l` both move. The mouse works too: click a row, scroll a list.

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
