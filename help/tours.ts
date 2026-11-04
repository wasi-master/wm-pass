// Tours: a few steps, each pointing at one part of the window and saying what it is for. A step
// whose part is not on screen, such as the sidebar in a narrow window, is skipped.

import type { Tour } from './schema.ts';

export const TOURS: readonly Tour[] = [
  {
    id: 'tour.window',
    title: 'Around the window',
    shells: ['desktop', 'extension'],
    width: 'wide',
    steps: [
      {
        anchor: 'sidebar.tabs',
        before: { go: 'all' },
        title: 'Places in your vault',
        body: 'All items, favorites, one-time codes, the password generator, health checks and the trash. Which ones show, and in what order, is under Settings, Appearance, Sidebar.',
        placement: 'right',
      },
      {
        anchor: 'list.search',
        title: 'Search',
        body: 'Type part of a name, a website, a username or a tag. {{key:search}} brings you here from anywhere in the window.',
      },
      {
        anchor: 'list.add',
        title: 'Add an item',
        body: 'Choose what kind it is: a login, a one-time code, a note, a card and more. {{key:add}} adds the first kind on the menu.',
      },
      {
        anchor: 'list.rows',
        title: 'Your items',
        body: 'Click one to see it. Right-click for the rest: copy, edit, favorite, pin, hide on the shelf or move to the trash.',
        placement: 'right',
      },
      {
        anchor: 'detail.actions',
        before: { open: 'sample' },
        title: 'Copy without showing it',
        body: 'These buttons copy the password or the username, or open the site. A copied secret is cleared from the clipboard after a while.',
      },
      {
        anchor: 'sidebar.lock',
        title: 'Lock',
        body: 'Locks the vault now. It also locks by itself after a while with nothing happening, which you can change under Settings, Security.',
        placement: 'top',
      },
      {
        anchor: 'sidebar.settings',
        title: 'Settings and this guide',
        body: 'Every setting has a **?** beside it that says what it does. {{key:help}} explains whatever you are looking at, and Settings, Guide has the rest.',
        placement: 'top',
      },
    ],
  },
  {
    id: 'tour.window_narrow',
    title: 'Around the window',
    shells: ['desktop', 'extension'],
    width: 'narrow',
    steps: [
      { anchor: 'list.search', before: { go: 'all' }, title: 'Search', body: 'Type part of a name, a website, a username or a tag.' },
      { anchor: 'list.add', title: 'Add an item', body: 'Choose what kind it is: a login, a one-time code, a note, a card and more.' },
      { anchor: 'list.rows', title: 'Your items', body: 'Click one to see it and copy from it. Right-click for the rest.' },
      {
        anchor: 'narrow.tabs',
        title: 'Pages',
        body: 'Your items, one-time codes, the generator and settings. Every setting has a **?** that says what it does.',
        placement: 'top',
      },
    ],
  },
  {
    id: 'tour.home',
    title: 'Around the app',
    shells: ['android'],
    steps: [
      { anchor: 'list.search', title: 'Search', body: 'Type part of a name, a website, a username or a tag.' },
      { anchor: 'list.add', title: 'Add an item', body: 'Choose what kind it is: a login, a one-time code, a note, a card and more.', placement: 'top' },
      { anchor: 'list.rows', title: 'Your items', body: 'Tap one to see it and copy from it. Swipe a row for a quick action, which you can choose under Settings, Appearance, Vault list.' },
      { anchor: 'narrow.tabs', title: 'Pages', body: 'Your items, one-time codes, the generator and health checks. Which ones show, and in what order, is under Settings, Appearance, Navigation.', placement: 'top' },
      { anchor: 'sidebar.lock', title: 'Lock', body: 'Locks the vault now. It also locks by itself after you leave the app, which you can change under Settings, Security.' },
      { anchor: 'sidebar.settings', title: 'Settings and this guide', body: 'Every setting has a **?** beside it that says what it does, and Settings, Guide has the rest.' },
    ],
  },
  {
    id: 'tour.first_item',
    title: 'Your first item',
    shells: ['desktop', 'extension'],
    steps: [
      {
        anchor: 'list.add',
        before: { go: 'all' },
        title: 'Every kind of item starts here',
        body: 'Logins, one-time codes, notes, cards, identities, Wi-Fi, SSH keys and more. This tour opens a new login for you.',
      },
      {
        anchor: 'editor.name',
        before: { open: 'editor' },
        title: 'Its name',
        body: 'What the item is called in your list. Leave it empty and the website’s name is used.',
      },
      {
        anchor: 'editor.fields',
        title: 'Username and password',
        body: 'Type them in, or generate a password from the button in its field. Nothing is saved until you save.',
      },
      {
        anchor: 'editor.folds',
        title: 'Everything else folds away',
        body: 'Websites, a one-time code, notes and more open when you need them. Which ones start open is under Settings, Adding and editing, Editor.',
      },
      {
        anchor: 'editor.save',
        title: 'Save',
        body: 'Saves the item to the vault. Close leaves without saving, and asks first if you changed anything.',
        placement: 'left',
      },
    ],
  },
  {
    id: 'tour.codes',
    title: 'One-time codes',
    steps: [
      {
        anchor: 'codes.list',
        before: { go: 'codes' },
        title: 'Every code in one place',
        body: 'Each item that has a one-time code is listed here. Click a code to copy it; double-click to open its item.',
      },
      {
        anchor: 'codes.add',
        title: 'Add a code',
        body: 'Paste or scan the setup key a site shows when you turn on two-step sign-in, or bring your codes over from another app.',
      },
    ],
  },
  {
    id: 'tour.generator',
    title: 'The password generator',
    steps: [
      {
        anchor: 'generator.value',
        before: { go: 'generator' },
        title: 'A new password',
        body: 'A fresh one each time something changes. Copy it, or save it as a new login.',
      },
      {
        anchor: 'generator.modes',
        title: 'What kind',
        body: 'Random characters, a few words, something you can say aloud, a PIN, or a pattern of your own.',
      },
      {
        anchor: 'generator.controls',
        title: 'Length and characters',
        body: 'Changes apply at once. What the generator starts with is under Settings, Adding and editing, Generator.',
      },
      {
        anchor: 'generator.save',
        title: 'Keep it',
        body: 'Starts a new login with this password already filled in.',
      },
    ],
  },
  {
    id: 'tour.health',
    title: 'Health',
    steps: [
      {
        anchor: 'health.score',
        before: { go: 'health' },
        title: 'The score',
        body: 'Out of 100: higher means fewer problems across your vault. It can be hidden under Settings, Health checks.',
      },
      {
        anchor: 'health.groups',
        title: 'What to fix',
        body: 'Each group is one kind of problem, such as a password used more than once. Open a group to see its items and go to each one.',
      },
    ],
  },
  {
    id: 'tour.security',
    title: 'Security settings',
    steps: [
      {
        anchor: 'group:security.locking',
        before: { go: 'settings/security' },
        title: 'When the vault locks',
        body: 'After a time with nothing happening, when the computer sleeps, and what the clipboard keeps.',
      },
      {
        anchor: 'group:security.master',
        title: 'Your master password',
        body: 'Change it here. Rekeying also gives the vault a new key, so an old copy of the file cannot follow what you add later.',
      },
      {
        anchor: 'group:security.methods',
        title: 'Ways in',
        body: 'Extra passwords, a key file or a security key. Each one opens the same vault.',
      },
      {
        anchor: 'group:security.duress',
        title: 'If you are made to open it',
        body: 'Passwords that look ordinary at the door but erase, lock or open a decoy instead.',
      },
      {
        anchor: 'group:security.emergency',
        title: 'If you cannot open it',
        body: 'Split a recovery key between people you trust, so that enough of them together can get in.',
      },
      {
        anchor: 'group:security.network',
        title: 'Offline unless you say',
        body: 'Nothing leaves this device until this is on, and each network feature says what it sends.',
        placement: 'top',
      },
    ],
  },
  {
    id: 'tour.backup',
    title: 'Backups',
    steps: [
      {
        anchor: 'group:backup.places',
        before: { go: 'settings/backup' },
        title: 'Where copies go',
        body: 'A folder, a WebDAV server, S3 storage or a cloud drive. A backup is the encrypted file itself, so only your password opens it.',
      },
      {
        anchor: 'group:backup.schedule',
        title: 'How often, and how many',
        body: 'Copies can be made every day or every week while the app is running. The newest is checked before an older one is removed.',
      },
      {
        anchor: 'group:backup.manual',
        title: 'By hand',
        body: 'Save a copy as a file wherever you like, or merge in a file from elsewhere.',
        placement: 'top',
      },
    ],
  },
  {
    id: 'tour.sync',
    title: 'Sync with another device',
    steps: [
      {
        anchor: 'group:sync.lan',
        before: { go: 'settings/sync' },
        title: 'On the same network',
        body: 'Pair a phone or another computer once, by comparing a code. After that they sync directly, with nothing in between.',
      },
      {
        anchor: 'group:sync.places',
        title: 'Through a folder or server',
        body: 'Use a place you already have for backups. Each device writes its own encrypted file there and merges the others.',
      },
      {
        anchor: 'group:sync.accounts',
        title: 'Accounts held elsewhere',
        body: 'Open a Bitwarden account, your own server or a KeePass file alongside this vault.',
        placement: 'top',
      },
    ],
  },
  {
    id: 'tour.browser',
    title: 'Browser extension',
    shells: ['desktop'],
    steps: [
      {
        anchor: 'group:browser.connect',
        before: { go: 'settings/browser' },
        title: 'Let it connect',
        body: 'Turn this on, then **Set up browsers** so they can find WM Pass. The extension then uses the vault open here, so you unlock once.',
      },
      {
        anchor: 'group:browser.setup',
        title: 'Check the fingerprint',
        body: 'The first time an extension connects, it shows a fingerprint. It should match this one.',
      },
      {
        anchor: 'group:browser.paired',
        title: 'Connected extensions',
        body: 'Every extension that has connected. Forget one you no longer use.',
      },
    ],
  },
  {
    id: 'tour.palette',
    title: 'Quick search and auto-type',
    shells: ['desktop'],
    steps: [
      {
        anchor: 'group:desktop.palette',
        before: { go: 'settings/desktop' },
        title: 'From anywhere',
        body: 'A shortcut that works in every program opens a small search window. Enter copies the password.',
      },
      {
        anchor: 'group:desktop.autotype',
        title: 'Typing for you',
        body: 'With this on, Shift+Enter in quick search types the username and password into the window that was in front.',
        placement: 'top',
      },
    ],
  },
];
