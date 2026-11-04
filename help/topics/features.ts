// How WM Pass works: the ideas behind the settings, each pointing at the pages and rows that control it.

import type { Topic } from '../schema.ts';

// ---- text more than one platform shares ----

const SEARCH_WORDS = [
  'Type part of a name. Words of four letters or more also match with a letter or two missing, so "gthub" still finds GitHub.',
  'Search looks at names, websites, tags, usernames, the holder and issuer of cards, names and emails on identities, database hosts and users, Wi-Fi names, products, providers, and the names of fields you added. It never looks inside a password or another secret.',
  'These words narrow a search, and can be combined:',
  '- "tag:work", "kind:card", "site:github" and "folder:bank" keep to one tag, kind, website or folder. "kind:" takes login, otp, note, card, identity, recovery, wifi, ssh, api, database, license or document.',
  '- "has:otp", "has:passkey", "has:attachment", "has:alias", "has:notes", "has:link", "has:history" and "has:reminder" keep the items that have one.',
  '- "weak", "reused", "breached", "old" and "duplicate" keep the items the health check flags, and "pinned" and "favorite" keep those.',
  '- "modified<30d" keeps items changed in the last 30 days, and "modified>1y" those not changed for a year. "created<" and "created>" work the same way. Use d, w, m or y.',
  '- A word starting with a minus, such as "-old", leaves out items that match it. Quotes keep words together.',
  '- "in:notes" looks inside notes as well, which search otherwise leaves alone.',
  'Items in the trash and on the hidden shelf stay out of results. "trashed" brings the trash in, and "hidden" brings in the shelf while it is open.',
];

const LOCK_SAVES = 'Before it locks, WM Pass saves any change still waiting and leaves a nested vault you were in. The key is then wiped from memory, and the hidden shelf closes.';

const NETWORK_SENDS = [
  'What each feature sends:',
  '- Breach checks: the first five characters of each password\'s SHA-1 hash to Have I Been Pwned, and an email address to the service you pick.',
  '- Email aliases: your API key and the note you write, to the forwarding service.',
  '- Disposable inbox: a request for a throwaway address, to a public mail service.',
  '- Username checker: the name, to each site it asks.',
  '- Backups and sync: the encrypted vault file, to the place you chose.',
  '- Accounts: sign-in and sync with Bitwarden, Vaultwarden or your own server. Your password itself is never sent.',
  '- Sync on your own network: encrypted traffic to your paired devices, with nothing in between.',
];

const PLACE_GROUPS = [
  '- Accounts. With Google Drive, Dropbox, OneDrive and Box you sign in on the service\'s own page in your browser, and WM Pass never sees that password. With pCloud, MEGA, iCloud Drive and Proton Drive you sign in inside WM Pass.',
  '- Servers and storage, which covers WebDAV, S3, SFTP, FTP, SMB, NFS, and Synology or QNAP NAS boxes. WebDAV and S3 fill in the address for many services, such as Nextcloud, Backblaze B2 or Cloudflare R2.',
  '- Somewhere else, which means a Git repository on GitHub, GitLab or Gitea, or a mailbox over IMAP.',
];

const PLACE_VERIFY = 'Each backup is written, read back and compared before any older copy is removed, so a failed run can leave one copy too many, never one too few.';

const PLACE_CAVEATS = [
  'Some kinds can send a password across the network as text if you let them: plain FTP, IMAP without TLS, and NFS without Kerberos. The dialog says so where it applies.',
  'A public Git repository lets anyone download the encrypted file, so WM Pass refuses one unless you allow it.',
  'An SFTP server\'s key, and a NAS\'s own certificate if you accept it, are remembered on the first connection. If either changes later, WM Pass refuses to connect.',
  'Google Drive, Dropbox, OneDrive and Box appear only in builds that were registered with each service.',
];

const SYNC_MERGE = [
  'Merging goes field by field, and the newest value of each field wins. If two devices changed the same password, the value that loses goes into that item\'s history, so a conflict never destroys a password.',
  'File sync runs about 20 seconds after you change something, so a burst of edits makes one trip, and whenever you choose **Sync now**. Attachments are carried across once each, already encrypted.',
];

const ACCOUNT_KDBX = 'A KeePass file stays where it is, and WM Pass writes back to it on each save, keeping one earlier copy beside its own files. Syncthing or a cloud app can carry the file.';

const ACCOUNT_CAVEATS = [
  'A KeePass file is only as strong as its own settings. WM Pass writes it with an experimental KDBX 4 writer, so it reads every save back and compares it, and keeps the previous file.',
  'Inside an account, Settings, Security leaves out the master password, ways in, duress and emergency groups. Those belong to a WM Pass vault.',
  'Removing an account forgets it on this device, with its offline copy. Nothing on the server or in the file changes.',
];

const VAULTS_HOW = [
  'Both are under Settings, Sync and accounts, Nested and shared vaults.',
  '**A nested vault** is its own encrypted file, made with its own password. It opens from inside this vault without asking again.',
  '**A shared vault** lives at a [[feature.places|place]] that everyone sharing it can reach, such as a cloud folder. Each person has their own key to it, so nobody needs anyone else\'s password.',
  'To join someone\'s shared vault, choose **Join someone\'s shared vault** and make a code to send them. Read the short fingerprint it shows out to them, so they can check nobody swapped the code. Their invitation comes back to the same list, where you accept it and pick the place.',
  'The owner adds people from **People** by pasting their code, once the fingerprint they read out matches. Removing someone moves the vault to a new key, so they cannot open anything written from then on.',
  'Open one from the list and it works like your own vault. To leave, lock, or come back to this page and choose the row under **You are inside**.',
];

const VAULTS_CAVEATS = [
  'Anyone who can open the outer vault can open a nested one too, since it opens without asking again.',
  'Everyone with the key to a shared vault can read and change everything in it. Someone you remove keeps any copy they already had, so change the passwords they could see.',
  'Inside a nested or shared vault, Settings, Security shows only the locking choices. Ways in, duress and the emergency kit belong to the vault you opened at the door.',
];

const CODES_HOW = [
  'A code can live in a login, under **One-time code** in the editor, or on its own as a **Code** item. Give it the setup key a site shows, or the otpauth link behind its QR code.',
  'Time-based codes change on a fixed period, usually 30 seconds, and the ring beside each one shows how long it has left. In its last ten seconds the next code shows too, so a late copy still works.',
  'Counter-based codes are not shown until you ask, since showing one uses it up. Steam Guard codes work as well.',
];

const AUTOFILL_ANDROID_CAVEATS = [
  'Items in the trash or on the hidden shelf are never offered.',
  'A password typed by WM Keyboard passes through the keyboard\'s process, as anything you type does. The autofill service keeps it out of there.',
];

const BRIDGE_TALK = [
  'From then on the extension asks the app for the logins that match the page in front of you, and for the one you choose to fill. It cannot read the rest of the vault.',
  'The two talk over an encrypted channel, through a small relay program that holds no keys. Each side commits to its part of the code before it sees the other\'s, so someone in the middle gets one chance in a million.',
];

const BRIDGE_CAVEATS = [
  'The extension\'s own key is sealed by a browser key that scripts cannot read out, which stops a copied browser profile from connecting. It does not stop code running as the extension itself.',
  'The app\'s question names the extension and where it says it comes from. Anything on the computer could claim the same, so refuse a request you did not start.',
  'Passkeys, and the copy buttons in the toolbar popup, need the extension\'s own vault.',
];

export const features = [
  // ---- the vault ----

  {
    id: 'concept.vault_file',
    kind: 'concept',
    title: 'The vault file',
    what: 'Everything in a vault lives in one encrypted file, a .wmvault file. Without a way in, it is random-looking bytes of a fixed shape.',
    why: 'Because the file holds nothing readable, it can be copied anywhere: a backup folder, a cloud drive, a server you do not trust. Whoever holds a copy still needs a way in.',
    how: [
      'The items are encrypted under a random vault key. Each way in, such as a password, a password with a key file, or a password with a security key, opens a slot that holds a copy of that key. Changing a password rewraps the key and leaves the items as they are.',
      'The file has room for eight ways in. Unused slots are random bytes, so nobody can tell from the file how many ways in it has, or whether one of them is a duress password.',
      'A password is turned into a key with Argon2id, using at least 64 MiB of memory, and WM Pass refuses a file that asks for less. That makes every guess slow for someone trying passwords against a stolen copy.',
      'The contents are padded and sealed in chunks, with a fresh key on every save, so the file\'s size says little and a cut or rearranged file fails to open. Each save also raises a counter inside the file, so an older copy can be told from a newer one.',
      'Attachments are separate files, each under its own key, which is kept inside its item. Deleting the item for good destroys that key, and the attachment with it.',
      'Decoy vaults sit beside the real one as files of the same shape. One password is tried against all of them in the time one file takes, so how long unlocking takes gives nothing away.',
    ],
    caveats: [
      'Anyone with a copy of the file and any of its ways in can read the whole vault. [[action.rekey|Rekeying]] protects what you write afterwards, not copies made before.',
      'A vault saved by a newer version of WM Pass may open read-only in an older one, so that saving cannot drop fields the older version does not know.',
      'The format is published on the WM Pass website. No outside audit of the core has been done yet.',
    ],
    figure: 'fig.vault',
    related: ['concept.master_password', 'group.security.methods', 'action.rekey', 'feature.places', 'page.hardware'],
    docs: '/security/format/',
    words: 'wmvault file format encryption argon2 key slot header',
    on: {
      extension: {
        caveats: [
          'Anyone with a copy of the file and any of its ways in can read the whole vault. [[action.rekey|Rekeying]] protects what you write afterwards, not copies made before.',
          'The extension\'s own vault cannot use a security key. Connect to the desktop app to use one.',
          'A vault saved by a newer version of WM Pass may open read-only in an older one, so that saving cannot drop fields the older version does not know.',
          'The format is published on the WM Pass website. No outside audit of the core has been done yet.',
        ],
      },
    },
  },
  {
    id: 'concept.master_password',
    kind: 'concept',
    title: 'The master password',
    what: 'The password that opens your vault. It unlocks the key to everything in it, and nobody can reset it.',
    why: 'There is no account and no server that holds a copy, so there is no reset link. That is also what keeps anyone else, WM Pass included, from opening your vault.',
    how: [
      'When you make a vault, the door shows how hard the password would be to guess, and **Create vault** stays off until it is strong enough. **Generate a passphrase** suggests five random words.',
      'Unlocking turns the password into a key slowly on purpose, so that every guess is slow for anyone trying passwords against a copy of the file.',
      'After five wrong tries in a row, the door makes you wait: 30 seconds at first, doubling each time up to 15 minutes.',
      'You can add more passwords that open the same vault, and require a key file or a security key alongside one. See [[group.security.methods]].',
    ],
    when: 'A good one is long, random, and used nowhere else. Four or five random words work well, and are easier to type than a jumble of symbols. Keep a written copy somewhere safe until you know it by heart.',
    caveats: [
      'Forget it and the vault is gone, unless you have another way in or made an [[action.emergency_kit|emergency kit]].',
      'The waits apply at this door only. Someone who copies the file can guess elsewhere, slowed only by the key derivation, which is why the password\'s strength matters.',
      'A new password does not change what older backups open with. See [[action.change_password]] and [[action.rekey]].',
    ],
    related: ['action.change_password', 'action.rekey', 'group.security.methods', 'feature.emergency_kit', 'concept.vault_file', 'screen.door'],
    docs: '/start/first-vault/',
    words: 'master password passphrase forgot reset strength',
  },
  {
    id: 'concept.item_kinds',
    kind: 'concept',
    title: 'Kinds of item',
    what: 'A vault holds twelve kinds of item. Each has the fields that kind of thing needs, and all of them share the same extras.',
    how: [
      '- A login holds a username and password, with the websites it is for, a one-time code, passkeys, an email alias and security questions.',
      '- A code is a one-time code on its own, for an account whose password is kept elsewhere.',
      '- A note is text kept secret until you open it.',
      '- A card has the name on it, its number, expiry, security code, PIN, issuer and billing address.',
      '- An identity has a full name, email, phone, address and date of birth.',
      '- A recovery item keeps a recovery phrase or a list of backup codes, and the account it is for.',
      '- A Wi-Fi item has the network name, its security, the password, and whether the network is hidden.',
      '- An SSH key has the private and public key, fingerprint and passphrase, and says whether the SSH agent on a computer may offer it.',
      '- An API key has its provider, key, secret, scopes and expiry date.',
      '- A database item has the engine, host, port, database name, user and password, whether to use TLS, and a connection string.',
      '- A license has the product, license key, who it is licensed to, order number and expiry date.',
      '- A document has its type and number, with the scans as attachments.',
      'Every kind can also have a folder, tags, a color and icon, notes, fields of your own, attachments, links to other items, a reminder and a self-destruct date. Old values of its secret fields are kept in its history.',
      'Secret fields, such as passwords, card numbers, keys and notes, stay hidden until you choose to see them, and search leaves them alone.',
    ],
    when: 'Pick the kind that matches the thing, so its fields and copy buttons fit. Use **Code** when only the one-time code belongs in WM Pass; when the password is here too, keep the code in the login, so one item has both.',
    caveats: 'Which kinds the add menu offers, and in what order, is up to you under [[page.add]].',
    related: ['page.add', 'screen.editor', 'concept.search', 'feature.codes'],
    words: 'types kinds login card note identity wifi ssh api database license document recovery code',
  },
  {
    id: 'concept.search',
    kind: 'concept',
    title: 'Searching the vault',
    what: 'The search box finds items across the whole vault by their name, websites, username and other plain fields. A few extra words narrow it down.',
    how: [
      ...SEARCH_WORDS,
      'Results are in the list\'s order: by name, or newest first when the list is sorted by last changed. Pinned items come first. Arrow keys move through them, Enter opens the chosen one, and Escape clears the search.',
    ],
    when: 'Press {{key:search}} from anywhere in the window to jump to the box. If nothing matches, **Add it as a login** starts a new login with that name.',
    caveats: [
      'The word "breached" finds only what a breach check found since the vault was last unlocked. See [[feature.breaches]].',
      'Search leaves passwords and other secrets alone on purpose: a search box must not become a way to test what a password is.',
    ],
    related: ['screen.list', 'concept.item_kinds', 'feature.health', 'feature.shelf', 'setting.sort'],
    words: 'find filter query tag kind has search words',
    on: {
      android: {
        how: [
          ...SEARCH_WORDS,
          'Best matches come first, with pinned items at the top. Before you type, the search page offers words to start from under **Look for**, such as **Weak**, **Has code** and **Changed recently**, with the items you opened last below them.',
        ],
        when: 'Tap the search bar on the Vault tab. If nothing matches, the search page offers to add what you typed as a new login.',
      },
    },
  },

  // ---- protection ----

  {
    id: 'feature.locking',
    kind: 'feature',
    title: 'Locking',
    what: 'Locking closes the vault and wipes its key from memory. Until you unlock it again, WM Pass can show, copy or fill nothing from it.',
    why: 'An open vault is readable by anyone at the keyboard. Locking often, and by itself, keeps that window short.',
    how: [
      LOCK_SAVES,
      'It locks when you choose **Lock** in the sidebar or the tray, or press {{key:lock}}. It also locks after the time set under [[setting.lock_after]], when the computer sleeps if [[setting.lock_on_sleep]] is on, and when the window is hidden if [[setting.lock_on_hide]] is on.',
      'While it is locked, quick search shows nothing, the SSH agent has no keys to offer, and a connected browser extension is told the vault is locked. Scheduled backups still run, since they only copy the encrypted file.',
      'Getting back in takes the master password or another way in you set up. On Linux, the system keyring can hold the key if you turn on [[action.keyring_unlock]].',
    ],
    when: 'Lock whenever you step away. The timer is a safety net for the times you forget.',
    caveats: [
      'Locking does not protect a vault from a program that was already running on the computer while it was open.',
      'Locking does not empty the clipboard. A secret copied before the lock keeps its own timer. See [[feature.clipboard]].',
    ],
    figure: 'fig.lock',
    related: ['setting.lock_after', 'setting.lock_on_sleep', 'setting.lock_on_hide', 'action.keyring_unlock', 'action.panic_now', 'page.security'],
    docs: '/security/threat-model/',
    words: 'lock auto-lock timeout sleep idle',
    on: {
      extension: {
        how: [
          LOCK_SAVES,
          'It locks when you choose **Lock**, after the time set under [[setting.lock_after]], and when the computer\'s screen locks if [[setting.lock_on_sleep]] is on.',
          'The key of the extension\'s own vault stays in the browser\'s memory while it is open. It survives the browser pausing the extension\'s background, but not the browser closing. A nested vault locks when the background stops.',
        ],
      },
      android: {
        how: [
          LOCK_SAVES,
          'It locks when you tap the lock button, this long after you leave the app under [[setting.lock_after]], when the screen goes off if [[setting.lock_screen_off]] is on, and when a panic app signals it.',
          'Getting back in takes the master password, or your fingerprint if you turned that on. The fingerprint key lives in the phone\'s secure hardware, and a fingerprint added to the phone later makes it unusable, so you type the password again.',
        ],
        related: ['setting.lock_after', 'setting.lock_screen_off', 'setting.panic_action', 'page.security'],
      },
    },
  },
  {
    id: 'feature.clipboard',
    kind: 'feature',
    title: 'Copying secrets',
    what: 'When you copy a password, a code or another secret, WM Pass marks it as sensitive and empties the clipboard again after a while.',
    why: 'Anything on the clipboard can be read by other programs and pasted by mistake into the wrong place. Marking a copy and clearing it soon after narrows both.',
    how: [
      'A copied secret is marked so that clipboard history, and clipboard managers that respect the mark, leave it out. On Windows it is kept out of the cloud clipboard too.',
      'A saved secret goes from the vault to the clipboard inside the app, so the window never holds it as text.',
      'The message that says something was copied shows how long it stays, with a **Clear now** button.',
      'When the time set under [[setting.clipboard_seconds]] is up, WM Pass empties the clipboard only if your secret is still on it. Anything you copied since is left alone.',
      'Usernames, websites and other plain fields are copied as they are, and not cleared.',
    ],
    when: 'Where you can, fill instead of copying: the browser extension and auto-type do not use the clipboard.',
    caveats: [
      'A program reading the clipboard during those seconds still sees the secret.',
      'Locking the vault does not clear the clipboard early.',
    ],
    figure: 'fig.clipboard',
    related: ['setting.clipboard_seconds', 'feature.autofill', 'feature.autotype', 'screen.item'],
    words: 'clipboard copy paste clear history',
    on: {
      extension: {
        what: 'When you copy a password, a code or another secret, the extension empties the clipboard again after a while.',
        how: [
          'The extension cannot mark a copy as sensitive, so clipboard history may keep it.',
          'The message that says something was copied shows how long it stays, with a **Clear now** button.',
          'When the time set under [[setting.clipboard_seconds]] is up, the extension empties the clipboard, whatever is on it by then. The browser runs its timers no sooner than 30 seconds, so a shorter time waits that long.',
          'Usernames, websites and other plain fields are copied as they are, and not cleared.',
        ],
        when: 'Where you can, fill instead of copying: filling a page does not use the clipboard.',
      },
      android: {
        how: [
          'A copied secret is marked as sensitive, so Android keeps it out of clipboard previews and history where it honors the mark.',
          'A small bar counts down until it is cleared, with **Clear now**.',
          'When the time set under [[setting.clipboard_seconds]] is up, WM Pass empties the clipboard, unless you have copied something else since.',
          'If WM Keyboard is your keyboard, its clipboard history keeps the copy only until then: when WM Pass empties the clipboard, the keyboard drops it from its history too.',
          'Usernames, websites and other plain fields are copied as they are, and not cleared.',
        ],
        when: 'Where you can, fill instead of copying: the autofill service does not use the clipboard.',
      },
    },
  },
  {
    id: 'feature.shelf',
    kind: 'feature',
    title: 'The hidden shelf',
    what: 'A place for items you do not want seen by accident. Items on the shelf stay out of every list and search until you open the shelf with your master password.',
    why: 'Some items should not show when you open the vault in front of someone. The shelf keeps them out of the way without a second vault.',
    how: [
      'Right-click an item, or open its **More** menu, and choose **Move to hidden shelf**. Several selected items go at once with **Put on the hidden shelf**.',
      'To see them, open Settings, Hidden shelf, or add **Hidden shelf** to the sidebar under [[page.navigation]], and type your master password. It is asked even though the vault is already open.',
      'While the shelf is open, its items open and edit as usual. Choose **Close the shelf**, or lock the vault, and they go out of sight again.',
      'While it is closed, its items are out of the lists, search and quick search, and nothing can open them, even a window that had one open before.',
      'Open or closed, shelf items are never offered for filling, never shown on the Codes screen, and never offered by the SSH agent.',
    ],
    when: 'For a few items you would rather keep out of sight. For a whole separate set of items, a [[feature.vaults|nested vault]] or a [[action.decoy_vault|decoy vault]] fits better.',
    caveats: [
      'The shelf hides items. It does not encrypt them separately: they are in the same vault file, under the same key, and anyone who has the master password can open the shelf.',
      'Shelf items still count towards the health score.',
      'Backups and sync carry shelf items like any other, since they copy the whole file.',
    ],
    figure: 'fig.shelf',
    related: ['page.shelf', 'screen.shelf', 'feature.duress', 'feature.vaults'],
    docs: '/guides/duress/',
    words: 'hide hidden private shelf out of sight',
    on: {
      android: {
        how: [
          'Open an item and, under **More** at the bottom, choose **Move to hidden shelf**.',
          'To see them, open Settings, Hidden shelf, and type your master password. It is asked even though the vault is already open.',
          'While the shelf is open, its items open and edit as usual. Tap **Close the shelf**, or lock the vault, and they go out of sight again.',
          'Open or closed, shelf items are never offered for autofill and never shown on the Codes tab.',
        ],
      },
    },
  },
  {
    id: 'feature.duress',
    kind: 'feature',
    title: 'Duress and decoys',
    what: 'Passwords that do something other than open your vault, for when someone makes you unlock it. At the door they look exactly like any other password.',
    why: 'Refusing is not always safe. A duress password lets you appear to give in while protecting what matters.',
    how: [
      'Each way into the vault fills a slot in the file, and the slot says what happens when its password is typed: open the vault, wipe this device\'s copy, or lock hard. Slots are sealed and unused ones are random, so nobody can read from the file which kind a password is, or how many there are.',
      '- A [[action.wipe_password]] erases the vault on this device and shows "Wrong password." WM Pass also forgets the backup places, accounts and paired devices it knew here.',
      '- A [[action.lockout_password]] refuses every password for an hour, the right one included. Nothing is lost.',
      '- A [[action.decoy_vault]] is a separate vault with its own password. Fill it with a few believable logins, so it holds up to a look.',
      '- [[action.panic_now]] does what a lockout password does, from inside the open vault.',
      'Two more things help: the [[feature.shelf|hidden shelf]] keeps chosen items out of sight, and a self-destruct date, set from an item\'s **More** menu, removes an item at a time you choose.',
    ],
    when: 'Set these up only if you might face someone you cannot refuse, and keep backups somewhere they cannot reach. Those backups are how you get the vault back after a wipe.',
    caveats: [
      'These help against a quick look under pressure. They do not defeat forensics: flash storage does not reliably erase, and a disk image taken earlier, or a sync provider\'s file history, still holds what it held.',
      'A wipe touches only this device. Backups, synced copies and other devices keep the vault.',
      'An item that has self-destructed still exists in any copy made before then.',
      'Someone with an older copy of your files to compare against may notice a decoy.',
    ],
    figure: 'fig.duress',
    related: ['action.wipe_password', 'action.lockout_password', 'action.decoy_vault', 'action.panic_now', 'feature.shelf', 'page.security'],
    docs: '/guides/duress/',
    words: 'duress panic coercion forced decoy wipe hidden vault self-destruct',
    on: {
      android: {
        how: [
          'Each way into the vault fills a slot in the file, and the slot says what happens when its password is typed: open the vault, wipe this device\'s copy, or lock hard. Slots are sealed and unused ones are random, so nobody can read from the file which kind a password is, or how many there are.',
          '- A [[action.wipe_password]] erases the vault on this phone and shows "Wrong password." WM Pass also forgets the backup places, accounts and paired devices it knew here.',
          '- A [[action.lockout_password]] refuses every password for an hour, the right one included. Nothing is lost.',
          '- A [[action.decoy_vault]] is a separate vault with its own password. Fill it with a few believable logins, so it holds up to a look.',
          '- A panic app such as Ripple can tell WM Pass to lock, or to wipe, as set under [[setting.panic_action]].',
          'Two more things help: the [[feature.shelf|hidden shelf]] keeps chosen items out of sight, and **Self-destruct**, under **More** on an item, removes it at a time you choose.',
        ],
        related: ['action.wipe_password', 'action.lockout_password', 'action.decoy_vault', 'setting.panic_action', 'feature.shelf', 'page.security'],
      },
    },
  },
  {
    id: 'feature.emergency_kit',
    kind: 'feature',
    title: 'Emergency access',
    what: 'A way for people you trust to open your vault together if you cannot, without any one of them being able to do it alone.',
    why: 'Nothing can recover a forgotten master password, and nobody can reach your vault if something happens to you. An emergency kit gives that power to a group, and to no single person.',
    how: [
      'The kit adds a recovery key to the vault as its own way in, then splits that key into shares. You choose how many people, from two to seven, and how many of them it takes.',
      'Fewer than that number learn nothing at all from their shares, even if they put them together.',
      'WM Pass saves the kit as a file. Print it, give each person their share, then delete the file.',
      'To use it, enough of them take a copy of the vault file to WM Pass, choose **Add another vault or account** at the door, or **I already have a vault** on a new device, then **Recover with emergency shares**. They paste the shares, choose a new password, and the vault opens.',
    ],
    when: 'When there are people you would trust with the whole vault today. Choose how many it takes, such as any three of five, so one lost share or one absent person does not block the rest.',
    caveats: [
      'Nothing enforces a waiting period. People holding enough shares can open the vault at once, so any wait is a promise between you.',
      'They also need a copy of the vault file, so make sure they can get one, from a backup for example.',
      '[[action.rekey|Rekeying]] stops the shares working. Make a new kit afterwards.',
      'Someone holding enough shares and an old copy of the file can still read that old copy after you rekey.',
    ],
    figure: 'fig.kit',
    related: ['action.emergency_kit', 'action.rekey', 'concept.master_password', 'screen.door'],
    docs: '/guides/emergency-access/',
    words: 'emergency shamir trustee inheritance family recovery shares',
  },
  {
    id: 'feature.network',
    kind: 'feature',
    title: 'Working offline',
    what: 'WM Pass works fully without a network, and nothing leaves the device until you turn on **Allow network features**. Each feature that needs the network says what it sends, and to whom, before it runs.',
    why: 'A password manager that talks to the internet is one more thing to trust. Staying local by default means the only copies of your vault are the ones you make.',
    how: [
      'One switch, [[setting.network_allowed]], stands in front of every feature that reaches the internet. While it is off, a page that needs it says what it would send and offers to turn it on.',
      'Setting up a backup place on a server, or adding a Bitwarden or server account, turns it on, since that is your yes to reaching it.',
      'With it on, network features run when you use them. In the background, only what you set up runs: scheduled backups, file sync after a change, and the update check when the app starts, if you turned it on.',
      'Web requests go over https. Plain http is refused, except to this device and to Tailscale addresses, where nothing readable crosses a network. SFTP, FTP, SMB, NFS and IMAP connections encrypt or not as the server and the place\'s settings decide.',
      'Site icons come with the app, so showing them never tells anyone which sites are in your vault.',
      ...NETWORK_SENDS,
      '- Updates: a request to GitHub for the latest release.',
    ],
    when: 'Leave it off if you use WM Pass on one device and keep backups in a folder. Turn it on when you want one of the features above.',
    caveats: [
      'Backups to a folder and opening a KeePass file need no network, so they work with this off.',
    ],
    related: ['setting.network_allowed', 'feature.breaches', 'feature.aliases', 'feature.places', 'feature.accounts', 'setting.update_check'],
    words: 'offline internet network privacy online',
    on: {
      extension: {
        how: [
          'One switch, [[setting.network_allowed]], stands in front of everything that reaches the internet. While it is off, a page that needs it says what it would send and offers to turn it on.',
          'Setting up a backup place on a server turns it on, since that is your yes to reaching it.',
          'Web requests go over https. Plain http is refused, except to this device and to Tailscale addresses, where nothing readable crosses a network.',
          'Site icons come with the extension, so showing them never tells anyone which sites are in your vault.',
        ],
        caveats: [
          'The extension makes these calls from its background, where its permission to reach any site lets it talk to services a web page cannot. The vault page only sees the answers.',
          'The extension\'s backups and sync go to places on servers, so they need this on.',
        ],
      },
      android: {
        how: [
          'One switch stands in front of every feature that reaches the internet. While it is off, a page that needs it says what it would send, with **Allow network features** to turn it on.',
          'Setting up a backup place on a server, or adding a Bitwarden or server account, turns it on, since that is your yes to reaching it.',
          'With it on, network features run when you use them. In the background, only what you set up runs, such as scheduled backups.',
          'Web requests go over https. Plain http is refused, except to this device and to Tailscale addresses, where nothing readable crosses a network. SFTP, FTP, SMB, NFS and IMAP connections encrypt or not as the server and the place\'s settings decide.',
          'Site icons come with the app, so showing them never tells anyone which sites are in your vault.',
          ...NETWORK_SENDS,
        ],
        caveats: [
          'Backups to a folder and opening a KeePass file need no network, so they work with this off.',
          'The offline build of the app has no permission to reach a network at all, so these features are not in it. It cannot sync over your own network either, since Android counts that as internet access.',
        ],
      },
    },
  },
  {
    id: 'feature.breaches',
    kind: 'feature',
    title: 'Breach checks',
    what: 'Checks whether your passwords, or the email addresses in your vault, appear in published leaks. It runs only when you ask.',
    why: 'A password that has leaked anywhere is on the lists attackers try first, however strong it looks. Knowing which ones have leaked tells you what to change first.',
    how: [
      '**Check every password** goes through each item with a password, with Have I Been Pwned or XposedOrNot. WM Pass hashes the password and sends only the start of the hash: five characters of SHA-1 to Have I Been Pwned, or ten of Keccak-512 to XposedOrNot. The service answers with every leaked hash that starts the same way, and the comparison happens on your device, so the service cannot tell which password you asked about.',
      'The check keeps running when you leave the page, and **Stop** ends it early.',
      'Passwords it finds lead the list on the [[screen.health]], with how many times each was seen.',
      'Under **Your email addresses**, each address used in your items can be checked with one of three services:',
      '- **LeakCheck** names the services an address appeared in. The address goes to LeakCheck.',
      '- **Have I Been Pwned** needs an API key of your own. The address and the key go to Have I Been Pwned.',
      '- **Passwords in COMB** asks Proxynova about the address, then compares the passwords it lists with your own, on your device. Only yes or no comes out: the leaked text is never shown or kept.',
    ],
    when: 'After you import a vault, and again from time to time.',
    caveats: [
      'It needs network features to be allowed.',
      'Results are kept in memory only, so they are gone once the vault locks. Run the check again to see them.',
      'A check finds only leaks the service knows about. A password it does not find can still be weak or reused.',
    ],
    figure: 'fig.breach',
    related: ['page.breaches', 'screen.health', 'feature.health', 'setting.network_allowed'],
    words: 'breach leak pwned hibp have i been pwned leakcheck comb',
    on: {
      extension: {
        caveats: [
          'Results are kept in memory only, so they are gone once the vault locks.',
          'A check finds only leaks the service knows about. A password it does not find can still be weak or reused.',
        ],
      },
    },
  },

  // ---- copies and other devices ----

  {
    id: 'feature.places',
    kind: 'feature',
    title: 'Places for copies',
    what: 'A place is somewhere outside this device where WM Pass keeps copies of your encrypted vault: a folder, a cloud account, a server or a mailbox. The same places serve backups, file sync and shared vaults.',
    why: 'A vault kept on one device is lost with it. A copy somewhere else is how you get it back, and because only the encrypted file goes there, the place cannot read it.',
    how: [
      'Add one under Settings, Backups or Settings, Sync and accounts, with **Add a place**. Places come in four groups:',
      '- A folder on this device. Syncthing, Nextcloud or a cloud provider\'s own app can carry it on from there.',
      ...PLACE_GROUPS,
      'What WM Pass needs to reach a place is kept on this computer, not in the vault, sealed under a key in the system\'s keychain, or in a file only you can read where there is no keychain. For pCloud, MEGA and Proton Drive it keeps only the session the sign-in hands back. iCloud Drive keeps your password too, and asks for a code from one of your devices about once a month.',
      PLACE_VERIFY,
    ],
    when: 'Set up at least one place before you rely on the vault. A lost device with no backup is a lost vault.',
    caveats: [
      'A place on a server needs network features to be allowed, and adding one allows them.',
      ...PLACE_CAVEATS,
    ],
    figure: 'fig.backup',
    related: ['page.backup', 'page.sync', 'feature.sync', 'feature.vaults', 'setting.network_allowed'],
    docs: '/guides/backup-sync/',
    words: 'place backup destination cloud webdav s3 sftp ftp smb nfs nas dropbox google drive onedrive icloud git imap',
    on: {
      extension: {
        how: [
          'Add one under Settings, Backups or Settings, Sync and accounts, with **Add a place**. The extension offers:',
          '- Accounts. With Google Drive, Dropbox, OneDrive and Box you sign in on the service\'s own page, and WM Pass never sees that password. With pCloud, MEGA and Proton Drive you sign in inside WM Pass.',
          '- Servers and storage, which covers WebDAV, S3, and Synology or QNAP NAS boxes. WebDAV and S3 fill in the address for many services.',
          '- Somewhere else, which here means a Git repository on GitHub, GitLab or Gitea.',
          'A browser cannot open the raw connections that SFTP, FTP, SMB, NFS or IMAP need, and iCloud\'s sign-in needs headers a page may not send. The extension reaches those through the desktop app while it is connected; the web vault cannot.',
          PLACE_VERIFY,
        ],
        caveats: [
          'Every place here is on a server, so it needs [[setting.network_allowed]], and adding one turns it on.',
          'A NAS needs a certificate from a known authority here: the extension cannot accept the NAS\'s own.',
          'A public Git repository lets anyone download the encrypted file, so WM Pass refuses one unless you allow it.',
          'Google Drive, Dropbox, OneDrive and Box appear only in builds that were registered with each service.',
        ],
      },
      android: {
        how: [
          'Add one under Settings, Backups or Settings, Sync and accounts, with **Add a place**. Places come in four groups:',
          '- A folder on this phone, which can be one that a cloud app or Syncthing carries on from there.',
          ...PLACE_GROUPS,
          'What WM Pass needs to reach a place is kept on this phone under a key in its secure hardware, not in the vault, so scheduled backups can run while the vault is locked. For pCloud, MEGA and Proton Drive it keeps only the session the sign-in hands back.',
          PLACE_VERIFY,
        ],
      },
    },
  },
  {
    id: 'feature.sync',
    kind: 'feature',
    title: 'Keeping devices in sync',
    what: 'Sync keeps the same vault on several devices. Changes made on any of them meet up and merge, so nothing typed on one device is lost on another.',
    why: 'Copies of a vault on several devices drift apart as you edit. Sync brings them together without one overwriting another.',
    how: [
      'There are three ways, and you can use more than one:',
      '- Through a folder or server. Turn on sync for any [[feature.places|place]] under Settings, Sync and accounts. Each device leaves its own encrypted copy there and merges everyone else\'s. A device only ever writes its own file, so two syncing at once cannot undo each other.',
      '- On your own network, where two paired devices talk directly. See [[feature.lan]].',
      '- Through an account, when a Bitwarden, Vaultwarden or WM Pass server account syncs with its server. See [[feature.accounts]].',
      ...SYNC_MERGE,
    ],
    when: 'Pick whichever your devices can all reach.',
    caveats: [
      'File sync runs for the vault you opened at the door, not inside a nested vault or an account.',
      'A file at the same place that is under another key, such as someone else\'s vault, is left alone.',
      'Merging an older copy is harmless: newer values still win.',
    ],
    figure: 'fig.sync',
    related: ['page.sync', 'feature.places', 'feature.lan', 'feature.accounts', 'feature.vaults'],
    docs: '/guides/backup-sync/',
    words: 'sync merge conflict devices phone computer',
    on: {
      extension: {
        how: [
          'There are two ways here:',
          '- Through a server. Turn on sync for any [[feature.places|place]] under Settings, Sync and accounts. Each device leaves its own encrypted copy there and merges everyone else\'s. A device only ever writes its own file, so two syncing at once cannot undo each other.',
          '- Through the desktop app. Connected to it, the extension uses the vault open there and has nothing of its own to sync. See [[feature.bridge]].',
          ...SYNC_MERGE,
        ],
      },
    },
  },
  {
    id: 'feature.lan',
    kind: 'feature',
    title: 'Syncing over your own network',
    shells: ['desktop', 'android'],
    what: 'Two of your devices on the same network can sync directly, with no server or cloud in between. You pair them once, and after that either can start a sync.',
    why: 'Some people want their vault on a phone and a computer without it ever touching someone else\'s server.',
    how: [
      'Pair under Settings, Sync and accounts, with **Pair a device**. Either show a code on one device and paste its text on the other, or compare six digits on both screens and accept on both.',
      'A shown code holds a one-off secret and stops working after two minutes. Six digits work without a camera: each side commits to its part before seeing the other\'s, so someone in the middle gets one chance in a million, and a failed try shows on both screens.',
      'Pairing stores the other device\'s public key, and later connections from any other key are refused.',
      'To sync, choose **Wait for one** on one device and **Sync now** on the other. Before anything is sent, each side proves it holds the vault\'s key, so a paired device with another vault, or one still on the key from before a [[action.rekey|rekey]], gets nothing.',
      'They swap only the items that differ, and merge them as file sync does. An attachment goes across only if the vault refers to it and the other side asked for it.',
    ],
    when: 'When your devices are often on the same Wi-Fi and you would rather not use a cloud account.',
    caveats: [
      'It counts as network use, so network features must be allowed.',
      'Devices find each other by an advert that names nothing. A device that connects can still learn this one\'s public key, a fixed identifier on that network that opens nothing.',
      'A device you forget needs pairing again before it can sync.',
    ],
    figure: 'fig.lan',
    related: ['page.sync', 'feature.sync', 'page.receive', 'setting.network_allowed'],
    docs: '/security/lan/',
    words: 'lan wifi local network pair peer direct',
    on: {
      android: {
        how: [
          'Pair under Settings, Sync and accounts, with **Add a device**. It asks whether the other device already has this vault: one with none gets a copy and is paired, one that has it is only paired. Either scan a code that the other device shows, or compare six digits on both screens and accept on both.',
          'A shown code holds a one-off secret and stops working after two minutes. Six digits work without a camera: each side commits to its part before seeing the other\'s, so someone in the middle gets one chance in a million, and a failed try shows on both screens.',
          'Pairing stores the other device\'s public key, and later connections from any other key are refused.',
          'To sync, tap **Wait for one** on one device and **Sync now** on the other. Before anything is sent, each side proves it holds the vault\'s key, so a paired device with another vault, or one still on the key from before a [[action.rekey|rekey]], gets nothing.',
          'They swap only the items that differ, and merge them as file sync does. An attachment goes across only if the vault refers to it and the other side asked for it.',
        ],
      },
    },
  },
  {
    id: 'feature.accounts',
    kind: 'feature',
    title: 'Bitwarden and KeePass accounts',
    what: 'WM Pass can open a vault that lives somewhere else: a KeePass file where it is, or an account on Bitwarden, Vaultwarden or a WM Pass server. It opens in place of your own vault, in the same screens.',
    why: 'Moving everything at once is not always possible. A work account on a Bitwarden server, or a KeePass file shared with others, can stay where it is while you use WM Pass for it.',
    how: [
      'Add one under Settings, Sync and accounts, with **Add an account**, or at the door with **Add another vault or account**. Choose **Bitwarden**, **Bitwarden EU**, **Own server** or **KeePass file**.',
      'An account opens in place of your vault, so adding one under Settings offers **Sign in now**: the vault locks and the door opens with the account chosen and its password asked for. Added at the door, it comes chosen there. Later, it appears at the door beside your own vault; choose it and type its own password.',
      'For a server account, your password is never sent: a key made from it proves who you are, as with any Bitwarden app. The first sign-in from a new device may need a code Bitwarden emails you, or a two-step code, typed beside the password.',
      'Changes go up to the server after each edit. If it cannot be reached they wait, and WM Pass keeps an offline copy, encrypted as the server\'s is.',
      ACCOUNT_KDBX,
    ],
    when: 'For a vault you share with people who use Bitwarden or KeePass, or one an employer runs.',
    caveats: ['Bitwarden does not publish its API, so a change on its side can stop this working.', ...ACCOUNT_CAVEATS],
    related: ['page.sync', 'screen.door', 'feature.sync', 'setting.network_allowed'],
    docs: '/guides/backup-sync/',
    words: 'account bitwarden vaultwarden keepass kdbx server sign in',
    on: {
      extension: {
        how: [
          'Add one under Settings, Sync and accounts, with **Add an account**, or at the door with **Add another vault or account**. The extension can open a **KeePass file** only: signing in to a server is in the desktop and phone apps.',
          'Added under Settings, it offers **Sign in now**, which locks the vault and opens the door with the file chosen. Added at the door, it comes chosen there. Later, it appears at the door beside your own vault; choose it and type its own password.',
          'The extension keeps its own copy of the file you choose, since a browser cannot reach the original again. Changes stay in that copy, not in the original file.',
        ],
        caveats: ACCOUNT_CAVEATS,
      },
      android: {
        how: [
          'Add one under Settings, Sync and accounts, with **Add an account**, or at the door. Choose Bitwarden, a server of your own, or a KeePass file.',
          'It then appears at the door beside your own vault. Choose it there and type its own password.',
          'For a server account, your password is never sent: a key made from it proves who you are, as with any Bitwarden app. The first sign-in from a new device may need a code Bitwarden emails you, or a two-step code, typed beside the password.',
          'Changes go up to the server after each edit. If it cannot be reached they wait, and WM Pass keeps an offline copy, encrypted as the server\'s is.',
          ACCOUNT_KDBX,
        ],
      },
    },
  },
  {
    id: 'feature.vaults',
    kind: 'feature',
    title: 'Vaults inside a vault',
    what: 'A vault can hold other vaults: nested ones, kept apart for your own use, and shared ones, which you and other people each open with your own key.',
    why: 'Some passwords belong to one part of your life, such as work, or to a group, such as a family. A separate vault keeps them apart, and a shared one lets several people keep the same logins up to date.',
    how: VAULTS_HOW,
    when: 'A nested vault for a part of your life you want kept apart. A shared vault for family or a team, without a server.',
    caveats: VAULTS_CAVEATS,
    figure: 'fig.vaults',
    related: ['page.vaults', 'feature.places', 'feature.sync', 'feature.shelf'],
    docs: '/guides/backup-sync/',
    words: 'nested shared family team vault inside separate',
    on: {
      extension: {
        caveats: [...VAULTS_CAVEATS, 'A nested or shared vault locks when the browser stops the extension\'s background, which it can do at any quiet moment.'],
      },
    },
  },
  {
    id: 'feature.import_export',
    kind: 'feature',
    title: 'Moving passwords in and out',
    what: 'Import brings items in from another password manager or authenticator app. Export writes your vault out again, encrypted or as plain text.',
    why: 'Your passwords should never be stuck in one app, this one included.',
    how: [
      'Under Settings, Import, choose your old app\'s export file. WM Pass shows how many items it found, a few of their names, and any it could not read, before anything is added.',
      'Password managers: 1Password, Apple Passwords, Avast, Avira, Bitwarden, Buttercup, Chrome and other Chromium browsers, Dashlane, Enpass, Firefox, Kaspersky, KeePass and KeePassXC, Keeper, LastPass, mSecure, NordPass, Password Safe, Proton Pass, Psono, RoboForm, SafeInCloud, Sticky Password and Zoho Vault. Authenticators: Aegis, 2FAS, andOTP, Authenticator Pro, Bitwarden Authenticator, Ente Auth, FreeOTP+, LastPass Authenticator, Proton Authenticator, Raivo, and Google Authenticator, whose export QR code you can choose as a screenshot.',
      'A file with a password, such as a KeePass or SafeInCloud database, asks for it first. Items already in the vault are skipped, and the result says how many.',
      'Under Settings, Export, a **WM Pass vault** or a **KeePass database** stays encrypted. **Bitwarden JSON**, **CSV** and a **Printable page** are plain text that anyone who opens them can read.',
    ],
    when: 'Import once when you move to WM Pass, then compare the count with your old app. Export when you leave, or for a printed copy kept somewhere safe.',
    caveats: [
      'Most exports from other apps are plain text. Delete the file once WM Pass has read it.',
      'The importers were tested against sample files written from each format\'s published or observed layout, not against exports from every real app. Compare the count afterwards.',
      'Some apps leave things out of an export: LastPass leaves out attachments and one-time code secrets, and Bitwarden\'s CSV leaves out cards, identities and passkeys. Use the fuller format where an app has one.',
      'A plain-text export from WM Pass holds every password. Keep it off shared storage and delete it when you are done.',
    ],
    related: ['page.import', 'page.export', 'feature.codes', 'concept.vault_file'],
    docs: '/guides/import-export/',
    words: 'import export migrate move csv kdbx json bitwarden 1password lastpass chrome keepass',
    on: {
      android: {
        how: [
          'Under Settings, Import, choose your old app\'s export file. WM Pass shows how many items it found, a few of their names, and any it could not read, before anything is added.',
          'Password managers: 1Password, Apple Passwords, Avast, Avira, Bitwarden, Buttercup, Chrome and other Chromium browsers, Dashlane, Enpass, Firefox, Kaspersky, KeePass and KeePassXC, Keeper, LastPass, mSecure, NordPass, Password Safe, Proton Pass, Psono, RoboForm, SafeInCloud, Sticky Password and Zoho Vault. Authenticators: Aegis, 2FAS, andOTP, Authenticator Pro, Bitwarden Authenticator, Ente Auth, FreeOTP+, LastPass Authenticator, Proton Authenticator and Raivo. For Google Authenticator, tap **Scan Google Authenticator\'s code** and scan each of its export codes in turn.',
          'A file with a password, such as a KeePass or SafeInCloud database, asks for it first. Items already in the vault are skipped, and the result says how many.',
          'Under Settings, Export, a **WM Pass vault** or a **KeePass database** stays encrypted. **Bitwarden JSON**, **CSV** and a **Printable page** are plain text that anyone who opens them can read.',
        ],
      },
    },
  },

  // ---- tools ----

  {
    id: 'feature.codes',
    kind: 'feature',
    title: 'One-time codes',
    what: 'WM Pass works as an authenticator. It keeps the setup keys sites give you for two-step sign-in, and shows the short codes they ask for.',
    why: 'A code beside its password means one app to open when you sign in, and one backup that covers both.',
    how: [
      ...CODES_HOW,
      'The [[screen.codes]] lists them all. Click one to copy it; the copy clears from the clipboard as a password does.',
      'Bring codes over from Aegis, 2FAS, andOTP, Ente Auth, FreeOTP+, Raivo or Google Authenticator under Settings, Import.',
    ],
    when: 'Whenever a site offers two-step sign-in with an authenticator app. The [[screen.health]] lists logins for sites that offer it where you have no code saved.',
    caveats: [
      'A code saved in the same vault as its password means that someone who opens the vault has both.',
      'Codes on the hidden shelf do not appear on the Codes screen.',
    ],
    related: ['screen.codes', 'page.codes', 'page.import', 'feature.health', 'concept.item_kinds'],
    words: 'totp hotp 2fa two-factor two-step authenticator otp mfa steam',
    on: {
      android: {
        how: [
          ...CODES_HOW,
          'The Codes tab lists them all. Tap one to copy it, and hold it to open its item. The add button there can scan a site\'s QR code.',
          'The Codes widget, if you turn on [[setting.widget_access]], and a paired watch, if you allow it under [[setting.wear_access]], can show codes too. Both only while the vault is open.',
          'Bring codes over from Aegis, 2FAS, andOTP, Ente Auth, FreeOTP+, Raivo or Google Authenticator under Settings, Import.',
        ],
        caveats: [
          'A code saved in the same vault as its password means that someone who opens the vault has both.',
          'Codes on the hidden shelf do not appear on the Codes tab.',
        ],
      },
    },
  },
  {
    id: 'feature.generator',
    kind: 'feature',
    title: 'Generated passwords',
    what: 'The generator makes passwords, passphrases and PINs from your device\'s own randomness, with every choice drawn evenly, so no result is likelier than another.',
    why: 'People are poor at picking random passwords. A generated one is as strong as its length and characters say, with no pattern to guess.',
    how: [
      '**Random** draws characters from the sets you pick, at a length from 8 to 64. **Avoid look-alikes** leaves out characters that are easy to misread, such as I, l, 1, O and 0.',
      '**Words** joins 3 to 12 words from the EFF\'s list of 7,776, with a separator, and capitals and a number if you want them. Words are easier to read aloud and to type by hand.',
      '**Pronounceable** alternates consonants and vowels, **PIN** is digits only, and **Pattern** follows a template you type.',
      'The bits figure measures every password the settings could make, not the one on screen, so it is the honest number. Time to crack assumes 10,000 guesses a second against a slow hash.',
      '**Presets** set several choices at once: **Strong**, **No symbols**, **Memorable** and **PIN**.',
    ],
    when: 'For every new password. If a site refuses one, try the **No symbols** preset or a shorter length.',
    caveats: [
      'A generated value is kept nowhere until you copy it or save it as a login. A copy follows the clipboard rules for secrets.',
      'Changes on the Generator screen last until you leave it. The starting choices are under [[page.generator]].',
    ],
    related: ['screen.generator', 'page.generator', 'setting.gen_mode', 'setting.gen_length', 'setting.editor_generate'],
    words: 'password generator random passphrase words diceware pin entropy bits',
  },
  {
    id: 'feature.health',
    kind: 'feature',
    title: 'Password health',
    what: 'The health check looks through your vault for passwords that are weak, reused, old or leaked, and other things worth fixing, and sums it up as a score out of 100.',
    why: 'One weak or reused password can undo the rest. Seeing them in one list tells you where to start.',
    how: [
      'All of it is worked out on your device, over every item not in the trash:',
      '- *In a known breach* lists passwords a [[feature.breaches|breach check]] found in published leaks.',
      '- *Used more than once* lists shared passwords, since one leak opens every site that uses it.',
      '- *Easy to guess* lists passwords an attacker with a password list would find quickly. A password built from the site\'s name or your username scores lower.',
      '- *Unencrypted sites* lists logins saved with a plain http address.',
      '- *Expiring soon* lists items whose reminder or expiry date is close.',
      '- *Not changed in a year* lists passwords older than that, or older than the item\'s own reminder allows.',
      '- *No second factor* lists sites known to offer one-time codes where you have no code or passkey saved. The list of sites comes with the app.',
      '- *Duplicate items* finds the same site, username and password saved more than once. Merging them keeps both histories.',
      'The score is the share of logins with a password that are none of weak, reused, leaked or old.',
    ],
    when: 'After an import, and whenever the score drops. The screen lists the most serious problems first.',
    caveats: [
      'Breach results last only until the vault locks, so that group is empty until you run a check again.',
      'Items on the hidden shelf count towards the score.',
      'Only logins with a password count towards the score.',
    ],
    related: ['screen.health', 'page.health', 'feature.breaches', 'setting.health_score', 'page.compare'],
    words: 'health score weak reused old breached audit check',
  },
  {
    id: 'feature.aliases',
    kind: 'feature',
    title: 'Aliases and forwarding',
    what: 'An alias is an email address you give one site in place of your real one. Mail to it still reaches you, and you can see who passed it on, or stop it.',
    why: 'One address everywhere lets sites link your accounts, and a single leak fills your inbox with spam. A different address for each site keeps them apart.',
    how: [
      'From your own address: a plus tag, such as you+shop@gmail.com, works with Gmail, Fastmail and many others, and Gmail also ignores dots in the name. Both are made on your device, with no service and no network.',
      'A forwarding service makes new addresses that forward to you: SimpleLogin, addy.io, Firefox Relay, or DuckDuckGo, which is experimental. Add the service\'s API key on the [[page.aliases]] page, and WM Pass keeps it in the vault as an API key item.',
      'In the editor, **Use an alias** beside a login\'s username gives that login an address of its own.',
      'Each alias from a service can have its forwarding switched off, without touching your inbox.',
    ],
    when: 'A plus tag is enough to see who leaked or sold your address. Use a forwarding service when a site should never learn your real address at all.',
    caveats: [
      'A site can strip a plus tag or the dots and reach your real address. Only a forwarding service hides it.',
      'The service you pick receives your API key and the note you write. It needs network features to be allowed.',
      'DuckDuckGo has no public API, so WM Pass uses the private one its own extension calls. It may stop working.',
    ],
    figure: 'fig.alias',
    related: ['page.aliases', 'page.inbox', 'screen.editor', 'setting.network_allowed'],
    words: 'alias masked email forward hide simplelogin addy relay duckduckgo plus',
    on: {
      extension: {
        caveats: [
          'A site can strip a plus tag or the dots and reach your real address. Only a forwarding service hides it.',
        ],
      },
    },
  },
  {
    id: 'feature.usernames',
    kind: 'feature',
    title: 'Checking usernames',
    what: 'The username checker asks a list of sites whether a name is already taken, so you can pick one that is free where you need it.',
    how: [
      'Type a name on the [[page.usernames]] page and choose **Check**. Every site is asked at once and answers **Free**, **Taken** or **No clear answer**.',
      'Sites that refuse anonymous lookups, or give the same answer for every name, get no answer, because a guess is worse than none. X, Threads and Reddit are among them, and the page links their profiles for you to open yourself.',
    ],
    caveats: [
      'Every site checked learns the name you looked up.',
      'It needs network features to be allowed.',
    ],
    related: ['page.usernames', 'setting.network_allowed', 'feature.network'],
    words: 'username handle availability taken free',
    on: {
      extension: {
        caveats: 'TikTok and Instagram only answer a browser that runs their scripts. The extension asks the desktop app to check those two, while it is connected.',
      },
    },
  },

  // ---- filling ----

  {
    id: 'feature.autofill',
    kind: 'feature',
    title: 'Filling logins',
    shells: ['extension', 'android'],
    what: 'WM Pass fills a saved login into the page or app in front of you, without the clipboard. Nothing fills until you choose a login.',
    why: 'Offering a login only where it was saved means a look-alike site gets nothing, and filling skips the clipboard altogether.',
    how: [
      'Click into a login field and a small key appears at its edge. Click the key, then choose a login. It shows for username and code fields too.',
      'The menu lists only logins saved for that site. It sits where the page\'s own scripts cannot read or press it, and it ignores a click that comes too soon, or lands while the page is faded or covered, which is how some pages try to steal a click.',
      'If the form is inside a frame from another site, the menu says so before you choose.',
      'The toolbar popup lists the same logins, each one a click from filling. Its shortcut starts as Ctrl+Shift+L, or Cmd+Shift+L on a Mac.',
      'After you sign in somewhere new, or with a changed password, a bar offers to save or update the login. **Offer to save logins** on the [[page.desktop-link]] page turns it off.',
      'Connected to the desktop app, the extension fills from the vault open there. See [[feature.bridge]].',
    ],
    when: 'Fill rather than copy wherever you can.',
    caveats: [
      'Nothing fills when a page loads. Every fill starts from your click.',
      'Items in the trash or on the hidden shelf are never offered.',
      'A login saved without a website, or for another site, is not offered. Search for it in the popup and copy it instead.',
    ],
    related: ['screen.popup', 'feature.bridge', 'feature.passkeys', 'page.desktop-link', 'feature.clipboard'],
    docs: '/apps/extension/',
    words: 'autofill fill form login browser suggestions',
    on: {
      android: {
        how: [
          'Set WM Pass as the phone\'s autofill service, from **Autofill service** under Settings, Security. A login field in another app or a browser then offers your saved logins.',
          'Apps are matched by their package name and signing certificate, never by name alone. Browsers are matched by web address, but only browsers WM Pass knows.',
          'A suggestion fills only when you tap it. A locked vault asks you to unlock first, and a web page shown inside another app asks before filling and names what it is filling.',
          '[[setting.autofill_inline]] shows logins in the keyboard\'s suggestion row, and [[setting.autofill_save]] offers to save after you sign in with a new or changed password.',
          'WM Keyboard\'s WM Pass tool is another way to fill, once you allow it under [[setting.keyboard_access]].',
        ],
        caveats: AUTOFILL_ANDROID_CAVEATS,
        related: ['setting.autofill_inline', 'setting.autofill_save', 'setting.keyboard_access', 'feature.passkeys', 'feature.clipboard'],
        docs: '/apps/android/',
      },
    },
  },
  {
    id: 'feature.passkeys',
    kind: 'feature',
    title: 'Passkeys',
    shells: ['extension', 'android'],
    what: 'A passkey signs you in to a site without a password, using a key kept in your vault. WM Pass can create passkeys and sign in with them.',
    why: 'A passkey belongs to the site that made it, so a look-alike site cannot ask for it, and there is no password to reuse or leak.',
    how: [
      'When a site asks to create or use a passkey, a WM Pass window asks you first, naming the site. Choose the account, then **Sign in** or **Save passkey**.',
      'If you type your master password in that window, the site is told you were verified. Some sites require it.',
      'If you have no passkey for that site, the browser\'s own handling carries on as usual.',
      'Passkeys are kept in a login item, which lists each one with its site and when it was made.',
    ],
    caveats: [
      'Passkeys work with the extension\'s own vault. They are not used through the desktop app.',
      'Only sites served over https can use them, and a new passkey is made only if the site accepts the kind WM Pass makes (ES256). Otherwise the browser takes over.',
    ],
    related: ['feature.autofill', 'concept.item_kinds', 'feature.health'],
    docs: '/apps/extension/',
    words: 'passkey webauthn fido passwordless',
    on: {
      android: {
        how: [
          'On Android 14 and later, choose WM Pass as a passkey provider in the phone\'s settings. Apps and browsers that ask for a passkey can then use one from your vault.',
          'A locked vault offers only a way to unlock it, since the keys stay in the vault.',
          'An app other than a browser WM Pass knows gets a confirmation that names it, in place of a check against the site\'s published list of apps.',
          'The site is told you were verified only after a real fingerprint or device check.',
          'Passkeys are kept in a login item, which lists each one with its site and when it was made.',
        ],
        caveats: 'Passkeys from WM Pass need Android 14 or later.',
        docs: '/apps/android/',
      },
    },
  },
  {
    id: 'feature.bridge',
    kind: 'feature',
    title: 'Connecting the extension',
    shells: ['desktop', 'extension'],
    what: 'The browser extension can use the vault that is open in the desktop app, so you unlock once on the computer and fill in the browser.',
    why: 'Without it, the extension needs a vault of its own, with its own password to type and its own copy to keep in step.',
    how: [
      'In the app, open Settings, Browser extension. Turn on [[setting.bridge_enabled]], then choose **Set up browsers**, which registers WM Pass with Chrome, Edge, Brave, Vivaldi, Chromium and Firefox.',
      'In the extension, open Settings, Desktop app, and choose **Connect**. Both show a six-digit code. If they match, say so on both.',
      ...BRIDGE_TALK,
      'With [[setting.bridge_ask_every_site]] on, the app asks once for each site before the first fill there.',
    ],
    when: 'When you use the desktop app and a browser on the same computer.',
    caveats: [...BRIDGE_CAVEATS, 'Forget an extension under **Connected extensions** to cut it off, including a conversation it already has open.'],
    figure: 'fig.bridge',
    related: ['page.browser', 'page.desktop-link', 'setting.bridge_enabled', 'setting.bridge_ask_every_site', 'feature.autofill'],
    docs: '/apps/extension/',
    words: 'browser extension desktop app connect pair native messaging chrome firefox',
    on: {
      extension: {
        how: [
          'In the desktop app, open Settings, Browser extension. Turn on **Let the extension connect**, then choose **Set up browsers**.',
          'Here, open Settings, Desktop app, and choose **Connect**. Both show a six-digit code. If they match, say so on both.',
          ...BRIDGE_TALK,
          'Unless you turned it off in the app, the app asks once for each site before the first fill there.',
        ],
        caveats: [...BRIDGE_CAVEATS, '**Disconnect** on the Desktop app page ends the connection from this side.'],
      },
    },
  },
  {
    id: 'feature.autotype',
    kind: 'feature',
    title: 'Typing logins with auto-type',
    shells: ['desktop'],
    what: 'Auto-type types a login\'s username, a Tab and its password into the window you were using, as if you typed them. It starts from quick search.',
    why: 'Some programs and remote desktops cannot be filled and will not take a paste. Typing reaches them, and skips the clipboard.',
    how: [
      'Turn on [[setting.auto_type_enabled]] under Settings, Desktop.',
      'Open [[screen.palette]], find the login, and press Shift+Enter. WM Pass hides its windows, waits a moment for the window behind to take focus, then types.',
      'It types the username, presses Tab, then types the password. It does not press Enter, so you can look before you sign in.',
      'The password is read and typed by the app itself, outside the window\'s page.',
    ],
    when: 'For apps the browser extension cannot reach, such as a desktop program or a remote session.',
    caveats: [
      'Typed text goes through the system\'s input queue, where other programs can see it.',
      'It types into whichever window has the focus. If another window comes to the front in that moment, that window gets the password.',
      'macOS asks for Accessibility permission the first time. On Linux it works under X11; Wayland desktops usually refuse it.',
      'With [[setting.lock_on_hide]] on, the vault locks once the main window is hidden, and quick search then has nothing to type.',
    ],
    figure: 'fig.autotype',
    related: ['setting.auto_type_enabled', 'screen.palette', 'setting.global_palette', 'feature.clipboard'],
    docs: '/apps/desktop/',
    words: 'auto-type autotype type keystrokes keyboard',
  },
  {
    id: 'feature.ssh',
    kind: 'feature',
    title: 'The SSH agent',
    shells: ['desktop'],
    what: 'While the vault is open, WM Pass can answer ssh and git for the SSH keys in it, so you sign in to servers with no key file on disk.',
    why: 'A private key file in your home folder can be copied by any program. Kept in the vault, the key is encrypted on disk and used only while the vault is open.',
    how: [
      'Turn on [[setting.ssh_agent]] under Settings, SSH agent. The page then shows a line to paste into your shell, which points SSH_AUTH_SOCK at WM Pass. It shows the exact line for your system.',
      'The agent offers every SSH key item with **Offer to the SSH agent** turned on. Programs can list those keys and ask for a signature; they cannot add or remove keys through it.',
      'With [[setting.ssh_confirm]] on, a window asks each time a program wants a key to sign something, naming the key. Choose **Allow** or **Refuse**.',
      'When the vault locks, the agent has nothing to offer until you unlock again.',
      'In an SSH key item, **Generate a key** makes a new Ed25519 key. Only Ed25519 keys can sign through the agent. A key with a passphrase is opened with the passphrase saved in its item.',
    ],
    when: 'When you use ssh, or git over SSH, on this computer and would rather your keys lived in the vault.',
    caveats: [
      'Any program running under your account can reach the agent. **Ask before each use** is what stops one from signing quietly, and its window names the key, not the program.',
      'Keys on the hidden shelf or in the trash are never offered.',
    ],
    figure: 'fig.ssh',
    related: ['page.ssh', 'setting.ssh_agent', 'setting.ssh_confirm', 'concept.item_kinds'],
    docs: '/apps/desktop/',
    words: 'ssh agent git ed25519 key SSH_AUTH_SOCK sign',
  },

  // ---- customise ----

  {
    id: 'feature.themes',
    kind: 'feature',
    title: 'Themes and colors',
    what: 'A theme sets WM Pass\'s colors, how round its corners are, and its font. Pick one of the built-in themes, or make and share your own.',
    how: [
      'On the [[page.themes]] page, choose from WM Pass, High contrast, Catppuccin, Dracula, Nord, Solarized, Gruvbox, Tokyo Night and more. A theme can have a light side, a dark side or both, and [[setting.look]] picks which one you see.',
      'Choose **New theme**, or **Customize a copy…** from a theme\'s menu, to make one of your own. Saving a built-in theme makes a copy, so the original stays as it was.',
      '**Share…** gives a theme as a file or as a line of text to copy. **Import** takes either.',
      'Whatever a theme says, WM Pass keeps text readable against its background, so a careless or unkind theme cannot hide a warning.',
    ],
    caveats: [
      'A theme holds colors and a few settings. Nothing from your vault is in it.',
      'Themes use the same format on the phone and the computer, so a theme shared from one opens on the other.',
    ],
    related: ['page.themes', 'page.look', 'setting.look'],
    words: 'theme colors colors dark light dracula catppuccin nord solarized custom share',
  },
] as const satisfies readonly Topic[];
