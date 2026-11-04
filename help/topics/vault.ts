// Settings, Folders, Trash, Import and Export.

import type { Topic } from '../schema.ts';

export const vault = [
  // ---- folders ----

  {
    id: 'page.folders',
    kind: 'page',
    title: 'Folders',
    page: 'folders',
    what: 'Every folder in this vault, with how many items are in each. Add, rename or remove them here.',
    how: [
      'Put an item in a folder from the editor, under **Organize**. An item is in one folder at most; tags can mark it in more than one way.',
      'The pencil beside a folder renames it, and the bin removes it. Removing a folder keeps what was in it: its items stay in the vault, in no folder.',
      'The number beside a folder counts the items in it, leaving out the trash and the hidden shelf.',
      'Folders can also show in the [[setting.sidebar_folders|sidebar]], and [[setting.grouping|group the vault list]].',
    ],
    related: ['setting.grouping', 'setting.sidebar_folders', 'page.import'],
    words: 'folder group organize category',
    on: {
      android: {
        how: [
          'Put an item in a folder from the editor, under **Organize**. An item is in one folder at most; tags can mark it in more than one way.',
          'Tap a folder to rename it, or to choose **Remove the folder**. Removing a folder keeps what was in it: its items stay in the vault, in no folder.',
          'The number beside a folder counts the items in it, leaving out the trash and the hidden shelf.',
          'Folders can also [[setting.grouping|group the vault list]].',
        ],
        related: ['setting.grouping', 'page.import'],
      },
    },
  },
  {
    id: 'action.new_folder',
    kind: 'action',
    title: 'New folder',
    page: 'folders',
    what: 'Adds an empty folder. Type its name and choose **Done**.',
    related: ['page.folders'],
  },

  // ---- trash ----

  {
    id: 'page.trash',
    kind: 'page',
    title: 'Trash',
    page: 'trash',
    what: 'Items you deleted wait here before they are gone for good. Open one to restore it, or empty the trash to delete everything in it now.',
    how: [
      'An item in the trash is left out of lists, search, autofill and the Health page, and out of every export except the WM Pass vault. It keeps its history and attachments until it is deleted for good.',
      'Open an item here and choose **Restore** to put it back as it was, or **Delete for good** to remove it now.',
      'Items that have been here longer than [[action.trash_keep|Keep deleted items for]] are deleted for good on their own.',
    ],
    caveats: 'Deleting an item for good removes it from this vault, and from your other devices as they sync. A backup or copy of the vault made before then still holds it.',
    related: ['screen.trash', 'action.trash_keep', 'action.empty_trash', 'page.backup'],
    words: 'deleted bin recycle restore undelete',
    on: {
      android: {
        how: [
          'An item in the trash is left out of lists, search, autofill and the Health page, and out of every export except the WM Pass vault. It keeps its history and attachments until it is deleted for good.',
          'Open an item here and choose **Restore** to put it back as it was, or **Delete for good** to remove it now.',
          'How long items wait here is set by [[action.trash_keep|Keep deleted items for]].',
        ],
      },
    },
  },
  {
    id: 'action.trash_keep',
    kind: 'action',
    title: 'Keep deleted items for',
    page: 'trash',
    what: 'How long an item stays in the trash before it is deleted for good, with its attachments.',
    how: [
      'The choice is saved in the vault itself, not in this app\'s settings. It goes wherever the vault goes, and it is not part of a settings file.',
      'Items past this age are deleted the next time the vault is opened.',
    ],
    effects: [
      { value: 7, label: '7 days', text: 'For a vault you tidy often. A mistake has a week to be noticed.' },
      { value: 30, label: '30 days', text: 'Enough time to notice most mistakes.' },
      { value: 90, label: '90 days', text: 'Room for a mistake you find only when you next need the item.' },
      { value: 365, label: '365 days', text: 'Deleted items stay for a year.' },
    ],
    related: ['page.trash', 'page.file'],
    words: 'retention empty automatically days',
    on: {
      android: {
        how: 'The choice is saved in the vault itself, not in this app\'s settings. It goes wherever the vault goes, and it is not part of a settings file.',
      },
    },
  },
  {
    id: 'action.empty_trash',
    kind: 'action',
    title: 'Empty the trash',
    page: 'trash',
    what: 'Deletes every item in the trash for good, with their attachments, once you confirm.',
    caveats: 'There is no undo. Only a backup made before you emptied it still holds the items.',
    related: ['page.trash', 'page.backup'],
    words: 'purge delete forever',
  },

  // ---- import ----

  {
    id: 'page.import',
    kind: 'page',
    title: 'Import',
    page: 'import',
    what: 'Brings your items over from another password manager or authenticator app, from the file it exports.',
    how: [
      'Pick the file with **Choose a file**. WM Pass works out the format itself and shows how many items it found, the first few names, and any it could not read. Nothing is added until you confirm.',
      'It reads these files:',
      '- 1Password (1PUX), Bitwarden (JSON or CSV), and KeePass or KeePassXC (the .kdbx database itself, KeePass\'s XML export, or KeePassXC\'s CSV).',
      '- SafeInCloud\'s own database file (SafeInCloud.db, or a backup its app made), or its XML export.',
      '- CSV from Apple Passwords, Avast, Avira, Buttercup, Chrome and other Chromium browsers, Dashlane, Firefox, LastPass, mSecure, NordPass, RoboForm and Zoho Vault.',
      '- JSON from Avast, Enpass, Keeper and Psono, Proton Pass\'s JSON or unencrypted ZIP, XML from Password Safe and Sticky Password, and Kaspersky Password Manager\'s text file.',
      '- Authenticators: Aegis, 2FAS, andOTP, Authenticator Pro (Stratum), Bitwarden Authenticator, FreeOTP+, LastPass Authenticator, Proton Authenticator and Raivo files, and a text file of otpauth links, one per line, which is what Ente Auth\'s plain-text export is.',
      '- Google Authenticator\'s export codes, with [[action.import_scan_google]], or a screenshot of a code chosen as the file.',
      '- A CSV that WM Pass exported.',
      'With nested vaults, **Import into** picks where the items go: this vault or one of them. The nested vault is opened for the import and closed again when you leave the page.',
      'Folders in the file become folders here, matched by name. Attachments come along from KeePass, SafeInCloud and 1Password files. A KeePass or SafeInCloud database, or a password-protected Proton Authenticator export, asks for its password. A KeePass database can also take its key file, and **Use security key** adds a YubiKey that KeePassXC set up for challenge-response, in slot 2 unless you chose slot 1.',
      'A login with the same site, username and password as one already in the vault is skipped, so importing the same file twice adds nothing.',
      'The file is read on this device. Nothing is sent anywhere.',
      '**How to export from your old app** lists the apps WM Pass reads. Choose one to read its export steps here, with a tab for each device; **Open on the website** has the same page with its pictures.',
    ],
    when: 'Once, when you move to WM Pass, or to bring in one-time codes from an authenticator app.',
    caveats: [
      'Most export files hold your passwords in plain text. Delete the file once WM Pass has read it, and empty the recycle bin.',
      'Encrypted exports from Bitwarden, Proton Pass, Aegis, 2FAS and Authenticator Pro cannot be read. Export them again without encryption. A KeePass database locked with a YubiKey opens only where WM Pass can reach the key: in the desktop app by USB, not in the browser extension.',
      'The importers were tested against sample files written from each format\'s published or observed layout, not against exports from every real app. Compare the count afterwards.',
    ],
    related: ['feature.import_export', 'page.export', 'page.folders'],
    docs: '/guides/import-export/',
    words: '1password bitwarden lastpass keepass kdbx chrome firefox dashlane proton csv json aegis 2fas google authenticator safeincloud kaspersky zoho sticky avast avira psono buttercup msecure pwsafe stratum migrate move',
    on: {
      android: {
        how: [
          'Tap **Choose a file**. WM Pass works out the format itself and shows how many items it found, the first few names, and any it could not read. Nothing is added until you confirm.',
          'It reads these files:',
          '- 1Password (1PUX), Bitwarden (JSON or CSV), and KeePass or KeePassXC (the .kdbx database itself, KeePass\'s XML export, or KeePassXC\'s CSV).',
          '- SafeInCloud\'s own database file (SafeInCloud.db, or a backup its app made), or its XML export.',
          '- CSV from Apple Passwords, Avast, Avira, Buttercup, Chrome and other Chromium browsers, Dashlane, Firefox, LastPass, mSecure, NordPass, RoboForm and Zoho Vault.',
          '- JSON from Avast, Enpass, Keeper and Psono, Proton Pass\'s JSON or unencrypted ZIP, XML from Password Safe and Sticky Password, and Kaspersky Password Manager\'s text file.',
          '- Authenticators: Aegis, 2FAS, andOTP, Authenticator Pro (Stratum), Bitwarden Authenticator, FreeOTP+, LastPass Authenticator, Proton Authenticator and Raivo files, and a text file of otpauth links, one per line, which is what Ente Auth\'s plain-text export is.',
          '- Google Authenticator\'s export codes, with [[action.import_scan_google]], or a screenshot of a code chosen as the file.',
          '- A CSV that WM Pass exported.',
          'With nested vaults, **Import into** picks where the items go: this vault or one of them. The nested vault is opened for the import and closed again when you leave the page.',
          'Folders in the file become folders here, matched by name. Attachments come along from KeePass, SafeInCloud and 1Password files. A KeePass or SafeInCloud database, or a password-protected Proton Authenticator export, asks for its password. A KeePass database can also take its key file, and **Use security key** adds a YubiKey that KeePassXC set up for challenge-response, in slot 2 unless you chose slot 1.',
          'A login with the same site, username and password as one already in the vault is skipped, so importing the same file twice adds nothing.',
          'The file is read on the phone. Nothing is sent anywhere.',
          '**How to export from your old app** lists the apps WM Pass reads. Tap one to read its export steps here, with a tab for each device; **Open on the website** has the same page with its pictures.',
        ],
        caveats: [
          'Most export files hold your passwords in plain text. Delete the file once WM Pass has read it.',
          'Encrypted exports from Bitwarden, Proton Pass, Aegis, 2FAS and Authenticator Pro cannot be read. Export them again without encryption. A KeePass database locked with a YubiKey needs the key by NFC or USB.',
          'The importers were tested against sample files written from each format\'s published or observed layout, not against exports from every real app. Compare the count afterwards.',
        ],
      },
    },
  },
  {
    id: 'action.import_scan_google',
    kind: 'action',
    title: 'Scan Google Authenticator\'s code',
    page: 'import',
    what: 'Reads the export code Google Authenticator shows under Transfer codes, Export codes, with the computer\'s camera or from a picture, and reads the one-time codes in it.',
    how: [
      'Hold the phone up to the camera, or choose **Choose a picture** for a screenshot of the code. A screenshot chosen with **Choose a file** works too.',
      'Google splits a long list across several codes. After the first, choose **Scan the next code** for each of the rest, then import them all together.',
      'The codes are read on this computer. Nothing is sent anywhere.',
    ],
    caveats: 'The codes hold the secret keys of your accounts. Delete any screenshot of them once the import is done.',
    on: {
      android: {
        what: 'Scans the export code Google Authenticator shows under Transfer codes, Export codes, and reads the one-time codes in it.',
        how: [
          'Google splits a long list across several codes. After the first, tap **Scan the next code** for each of the rest, then import them all together.',
          'A screenshot of the code works too: choose it with **Choose a file**.',
        ],
      },
    },
    related: ['page.import', 'feature.codes'],
    words: 'google authenticator qr migration transfer export otp',
  },

  // ---- export ----

  {
    id: 'page.export',
    kind: 'page',
    title: 'Export',
    page: 'export',
    what: 'Writes the whole vault to a file: either still encrypted, or in plain text for another app to read.',
    how: [
      '**Stays encrypted** keeps every password protected. [[group.export.plain|Plain text]] writes them out for anyone to read, and asks first.',
      'Apart from the WM Pass vault, which is a copy of the whole file, every export leaves out the trash. Attachments are kept in their own files beside the vault, and no export includes them.',
    ],
    when: 'To move to another app, to keep a copy you can open without WM Pass, or to print a paper copy to lock away. For regular copies you can restore, use [[page.backup]].',
    caveats: 'Exporting does not ask for your master password. Anyone at your unlocked vault could export it, which is one more reason to keep [[setting.lock_after]] short.',
    related: ['feature.import_export', 'page.import', 'page.backup', 'setting.lock_after'],
    docs: '/guides/import-export/',
    words: 'backup csv json kdbx keepass bitwarden print paper html save copy',
  },
  {
    id: 'action.export_wmvault',
    kind: 'action',
    title: 'WM Pass vault',
    page: 'export',
    what: 'Saves a copy of the vault file itself, still encrypted. It opens in WM Pass with the same passwords, key files and security keys as this vault.',
    how: 'Any changes still waiting to be saved are written first, so the copy is up to date.',
    caveats: [
      'Attachments are not in it, since they live in their own files beside the vault. The trash and the hidden shelf are, as they are part of the file.',
      'The copy keeps every way in this vault has now, duress passwords included. Changing your password later does not change the copy.',
    ],
    related: ['page.backup', 'concept.vault_file', 'action.rekey'],
    words: 'wmvault copy encrypted file',
  },
  {
    id: 'action.export_kdbx',
    kind: 'action',
    title: 'KeePass database',
    page: 'export',
    what: 'Writes a new KeePass database (KDBX 4) protected by a password you choose. KeePass, KeePassXC and other apps that read KDBX can open it.',
    how: [
      'The database uses Argon2id to protect its password. Before it is saved, WM Pass reads it back and checks that every login came through intact.',
      'Items of other kinds keep their fields, with a WM Pass kind field saying what they were.',
    ],
    caveats: [
      'Items on the hidden shelf are left out.',
      'Folders are not kept: every item goes into one group. Each login keeps one web address, and attachments and passkeys are not included.',
      'Anyone with the file can try passwords against it for as long as they like, so give it a long one.',
    ],
    related: ['page.import', 'page.shelf'],
    words: 'keepass keepassxc kdbx encrypted',
  },
  {
    id: 'group.export.plain',
    kind: 'concept',
    title: 'Plain text',
    page: 'export',
    what: 'These exports write out every password, note and one-time code secret as readable text. Anyone who gets the file can read all of it.',
    how: 'WM Pass asks before writing one, and saves it where you choose. Items on the hidden shelf are included.',
    caveats: [
      'Keep the file off shared drives, and out of folders that sync to the cloud, where old versions can be kept.',
      'Delete it as soon as you are done, and empty the recycle bin. A deleted file can stay readable on the disk for a while.',
    ],
    related: ['action.export_bitwarden', 'action.export_csv', 'action.export_html', 'action.export_pdf', 'page.shelf'],
    words: 'unencrypted plaintext readable warning',
  },
  {
    id: 'action.export_bitwarden',
    kind: 'action',
    title: 'Bitwarden JSON',
    page: 'export',
    what: 'Writes Bitwarden\'s unencrypted JSON, which Bitwarden and many other password managers can import.',
    how: 'Kinds Bitwarden does not have become secure notes, with their values as custom fields.',
    caveats: 'It is [[group.export.plain|plain text]]: anyone who opens the file can read every password.',
    related: ['group.export.plain'],
    words: 'bitwarden json move',
  },
  {
    id: 'action.export_csv',
    kind: 'action',
    title: 'CSV',
    page: 'export',
    what: 'Writes a spreadsheet file with one row per item, in WM Pass\'s own columns. WM Pass can import it back.',
    how: 'The columns are kind, folder, title, username, password, web addresses, one-time code, notes, tags and other fields. Custom fields, and the fields of kinds other than logins, go in the last column.',
    caveats: 'It is [[group.export.plain|plain text]]: anyone who opens the file can read every password.',
    related: ['group.export.plain', 'page.import'],
    words: 'csv spreadsheet excel',
  },
  {
    id: 'action.export_html',
    kind: 'action',
    title: 'Printable page',
    page: 'export',
    what: 'Writes a web page listing every item and its values, made to be printed and locked away.',
    how: 'Open it in a browser and print it. The page carries a warning at the top that anyone holding it can read everything on it.',
    caveats: [
      'One-time codes appear as their setup links, which hold the secret.',
      'The file is [[group.export.plain|plain text]] until you delete it, and the paper can be read by anyone who finds it.',
    ],
    related: ['group.export.plain', 'action.emergency_kit', 'action.export_pdf'],
    words: 'print paper html offline copy',
  },
  {
    id: 'action.export_pdf',
    kind: 'action',
    title: 'PDF or printer',
    page: 'export',
    what: 'Hands the printable page to the system\'s print dialog, which saves it as a PDF or sends it to a printer. No file is written first.',
    how: 'Choose a printer, or save as a PDF from the dialog and pick where the file goes. The page carries a warning at the top that anyone holding it can read everything on it. It is never shown in the window, and it is taken out again once the dialog closes.',
    caveats: [
      'One-time codes appear as their setup links, which hold the secret.',
      'Like the other [[group.export.plain|plain-text]] exports, the PDF is not encrypted: anyone who opens it can read everything until you delete it, and the paper can be read by anyone who finds it.',
    ],
    related: ['group.export.plain', 'action.export_html'],
    words: 'pdf print paper printer save',
    on: {
      android: {
        what: 'Hands the printable page to Android\'s print screen, which saves it as a PDF or sends it to a printer.',
        how: 'Choose **Save as PDF** there and pick where the file goes, or choose a printer. The page carries a warning at the top that anyone holding it can read everything on it.',
      },
    },
  },
] as const satisfies readonly Topic[];
