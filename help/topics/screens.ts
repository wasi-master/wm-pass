// The screens: the door, the window and its panes, Codes, Generator, Health, trash, shelf, quick search and the popup.

import type { Topic } from '../schema.ts';

// ---- text more than one platform shares ----

const DOOR_TROUBLE = [
  'After five wrong passwords in a row, the door makes you wait before the next try: 30 seconds at first, doubling up to 15 minutes. The wait is shown on screen.',
  'A vault that needs a key file says so after you type the password. **Choose the key file** lets you pick it: the same file you added, unchanged.',
  'Forgot the password? Nothing can reset it. If you made an emergency kit, the people holding it can open the vault with **Recover with emergency shares**, found under **Add another vault or account**.',
];

const LIST_ROWS = [
  'Each row shows the item\'s icon, its name, small marks for a favorite, a one-time code, a passkey, an attachment or a self-destruct date, and a second line such as the username. What a row shows is set under [[page.list]].',
  'Several logins for one site fold into one row that says how many accounts there are, unless [[setting.group_sites]] is off. Open it to see them all.',
];

const ITEM_FIELDS = [
  'Below come its fields. A secret shows as dots; the eye button shows it, and it hides again after the time set under [[setting.reveal_seconds]] or when you leave the item. When the password is showing, its strength is shown under it.',
  'A one-time code counts down with a ring and shows the next code in its last ten seconds. A counter-based code shows **New code** in place of the code.',
  'Further down are the websites, passkeys, fields of your own, security questions, notes, an email alias, attachments, linked items, the reminder and the tags. With no tags yet, WM Pass may suggest one, guessed on your device.',
  'The history link at the bottom, such as **History, 2 earlier**, lists earlier values of secret fields, each hidden until you show it.',
];

const EDITOR_PARTS = [
  'The name field comes first. Leave it empty and the website\'s name is used.',
  'The fields that kind of item needs come next. For some kinds, the less used ones wait under **More details**, unless [[setting.editor_fold_details]] is off.',
  'Further sections open when you need them: **Websites** for a login, **One-time code** for a login or code, **Notes**, **More fields** for fields of your own, hidden or not, **Security questions** for a login, and **Organize** for the folder, tags, icon, color and favorite. A section that already holds something starts open; [[page.editor]] sets which empty ones do.',
];

const EDITOR_SECRETS = 'A secret that is already saved shows as **Saved. Click to change** and stays in the vault until you click it, so fixing a name never brings the password onto the screen.';

const GENERATOR_TYPES = [
  '**Type** picks what to make. **Random** has a length from 8 to 64 and the character sets a-z, A-Z, 0-9 and !@#, plus **Avoid look-alikes**. **Words** has the number of words, what goes between them, and **Capitals** and **Number**. **Pronounceable** has syllables, **PIN** has digits, and **Pattern** takes a template.',
  '**Presets** set several of these at once.',
  'Changes here last until you leave. The starting choices are under [[page.generator]].',
];

const HEALTH_PARTS = [
  'The score out of 100 is the share of your logins with a password that are not weak, reused, leaked or old. The bar under it changes color as the score rises.',
  'Below it, each kind of problem has its own group with a count. Open a group to see why it matters and which items have it, and click an item to open it.',
  'Problems are listed from the most serious down, so start at the top. [[page.health]] chooses which groups are listed and whether the score shows.',
  'For duplicate items, the button that merges each set asks **Which one stays?** The others fold into it: their passwords move to its history and their extra fields come along, so nothing is lost.',
];

const TRASH_PARTS = [
  'Moving an item to the trash takes it out of every list, search and fill, and keeps it for the time set under Settings, Trash. A message offers **Undo** straight after.',
  'Open a trashed item to see it, then **Restore** to put it back as it was, or **Delete for good**.',
  'After that time, WM Pass deletes it for good the next time the vault opens, with its history and attachments.',
];

const TRASH_CAVEATS = [
  'Deleting for good cannot be undone here. A backup made before then still holds the item.',
  'An item that self-destructs skips the trash.',
];

export const screens = [
  // ---- the door ----

  {
    id: 'screen.door',
    kind: 'screen',
    title: 'Unlock screen',
    what: 'The unlock screen, or door, is where a vault is made and opened. Everything else waits behind it.',
    how: [
      'On the first run it asks you to **Create your vault**. Type a master password, watch the meter under it, and confirm it. **Create vault** stays off until the password is strong enough and both boxes match. **Generate a passphrase** suggests five random words.',
      'After that it asks for your password. Type it and press Enter or **Unlock**.',
      'If you have accounts or KeePass files, they show as choices above the password: **This device** for your own vault, and one for each account.',
      'A vault that needs a security key asks you to plug it in and touch it. If you turned on [[action.keyring_unlock]], **Unlock from the system keyring** opens the vault without the password.',
      '**Add another vault or account**, or **I already have a vault** on a new device, offers four ways in: [[action.join_device|Get it from another device]], over this network; **Restore from a file**, for a .wmvault or .wmbackup file; **Open a KeePass file or sign in**; and **Recover with emergency shares**.',
    ],
    when: DOOR_TROUBLE,
    caveats: 'A duress password looks like a wrong one here, or opens its decoy vault as if it were the real one. That is on purpose. See [[feature.duress]].',
    related: ['concept.master_password', 'group.security.methods', 'feature.accounts', 'feature.emergency_kit', 'feature.locking'],
    words: 'unlock door login sign in password create vault first run restore',
    on: {
      extension: {
        how: [
          'On the first run it asks you to **Create your vault**. Type a master password, watch the meter under it, and confirm it. **Create vault** stays off until the password is strong enough and both boxes match. **Generate a passphrase** suggests five random words.',
          'After that it asks for your password. Type it and press Enter or **Unlock**.',
          'If you have KeePass files open in place, they show as choices above the password: **This device** for your own vault, and one for each file.',
          '**Add another vault or account**, or **I already have a vault**, offers **Use the vault in the desktop app**, which connects this browser to WM Pass on this computer; **Restore from a file**, for a .wmvault or .wmbackup file; **Open a KeePass file or sign in**; and **Recover with emergency shares**.',
          'Connected to the desktop app, the extension can fill from the vault open there without this screen. See [[feature.bridge]].',
        ],
      },
      android: {
        how: [
          'On the first run it asks you to make your vault: type a master password, then type it again. You cannot go on until the password is strong enough and the two match. **Generate a passphrase** suggests random words.',
          'After that it asks for your password. Type it and tap **Unlock**, or tap **Use fingerprint** if you turned fingerprint unlock on.',
          'If you have accounts or KeePass files, they show as chips above the password: one for your own vault on this phone, and one for each account.',
          'A vault that needs a security key asks you to hold it to the back of the phone, or plug it in and touch it.',
          'For a vault you already have, the door offers [[action.join_device|Get it from another device]], **Restore from a file**, **Restore from a backup location**, **Open a KeePass file or sign in**, and **Recover with emergency shares**.',
        ],
        when: [
          'After five wrong passwords in a row, the door makes you wait before the next try: 30 seconds at first, doubling up to 15 minutes. The wait is shown on screen.',
          'If fingerprint unlock fails, type your password. A fingerprint added to the phone since you turned it on stops it working until you type the password again.',
          'Forgot the password? Nothing can reset it. If you made an emergency kit, the people holding it can open the vault with **Recover with emergency shares**.',
        ],
      },
    },
  },

  // ---- the window ----

  {
    id: 'screen.window',
    kind: 'screen',
    title: 'Main window',
    what: 'Once the vault is open, the window has three panes side by side: the sidebar, the list of items, and the item you chose.',
    how: [
      'The [[screen.sidebar|sidebar]] on the left picks what the list shows: all items, favorites, a kind, a folder or a tag. It also leads to Codes, the Generator, Health, the trash and Settings.',
      'The [[screen.list]] in the middle shows what matches, with the search box on top.',
      'The [[screen.item]] on the right shows the item you clicked. Editing or adding an item opens the [[screen.editor]] in the same place.',
      'Drag the line between two panes to make one wider. {{key:toggle_sidebar}} hides the sidebar or brings it back.',
      'Codes, the Generator and Health take the place of the list and the item. Settings takes the whole window.',
      'In a narrow window, one pane shows at a time, with tabs along the bottom for all items, Codes, the Generator and Settings. Opening an item shows a back arrow.',
    ],
    when: [
      'Keyboard shortcuts work from anywhere in the window, and can be changed under [[page.shortcuts]]:',
      '- {{key:search}} search, {{key:add}} add an item, {{key:edit}} edit the open item.',
      '- {{key:copy_password}} copy its password, {{key:copy_username}} its username, {{key:copy_code}} its code, and {{key:open_site}} open its website.',
      '- {{key:codes}} Codes, {{key:generator}} the Generator, {{key:health}} Health, {{key:settings}} Settings.',
      '- {{key:lock}} lock, and {{key:help}} explains the screen you are on.',
    ],
    tour: 'tour.window',
    preview: 'preview.window',
    related: ['screen.sidebar', 'screen.list', 'screen.item', 'page.navigation', 'page.shortcuts', 'feature.locking'],
    words: 'window layout panes main screen home',
    on: {
      android: {
        title: 'Home screen',
        what: 'Once the vault is open, the home screen shows your items, with tabs along the bottom for the other parts of the app.',
        how: [
          'The tabs are Vault, Codes, Generator, Health and Tools. Which ones show, and in what order, is under [[page.navigation]]. See [[screen.sidebar]].',
          'On the Vault tab, the search bar sits on top, or at the bottom if you moved it there. It also holds the lock and settings buttons.',
          'Under it, chips filter the list: **All**, **Favorites**, and one for each kind of item you have. Below them is the [[screen.list]].',
          'The add button in the corner adds an item, on the Vault and Codes tabs. [[page.navigation]] sets what it opens, or hides it.',
          'Tapping an item opens the [[screen.item]], and **Back** returns here.',
        ],
        when: 'With a keyboard plugged in or paired, shortcuts work here too. They are listed under [[page.shortcuts]].',
        tour: 'tour.home',
      },
    },
  },
  {
    id: 'screen.sidebar',
    kind: 'screen',
    title: 'The sidebar',
    what: 'The column on the left of the window. It chooses what the list shows and leads to the other parts of WM Pass.',
    how: [
      'At the top are the places you chose under [[page.navigation]]: **All items**, **Favorites**, **Codes**, **Generator**, **Health**, **Trash**, and **Hidden shelf** if you add it. Counts beside them can be turned off.',
      '**Types** lists each kind of item you have, with how many.',
      '**Folders** lists your folders. The **+** beside the heading makes a new one, and right-clicking a folder offers **Rename…** and **Remove folder**. Removing a folder keeps what was in it, in no folder.',
      '**Tags** lists every tag in use. Tags are added from an item\'s **More** menu or in the editor.',
      'At the bottom are the vault\'s name, **Lock** and **Settings**.',
      'Drag its right edge to change its width. {{key:toggle_sidebar}} hides it or brings it back.',
    ],
    when: 'Turn off the parts you do not use under [[page.navigation]], so the rest is easier to scan.',
    preview: 'preview.sidebar',
    related: ['page.navigation', 'page.folders', 'screen.window', 'screen.list'],
    words: 'sidebar sections folders tags types navigation',
    on: {
      android: {
        title: 'Navigation bar',
        what: 'The bar along the bottom of the home screen. Each tab opens one part of WM Pass.',
        how: [
          '**Vault** lists your items. **Codes** shows one-time codes, **Generator** makes passwords, **Health** checks them, and **Tools** holds email aliases, the disposable inbox, the username checker and the password comparison.',
          'Which tabs show, in what order, and whether they have labels, is under [[page.navigation]].',
          'Folders and tags are not in the bar: set them on an item, manage folders under Settings, Folders, and find them in search with "folder:" or "tag:".',
        ],
        when: 'With only one tab chosen, the bar is hidden.',
      },
    },
  },
  {
    id: 'screen.list',
    kind: 'screen',
    title: 'Item list',
    what: 'The middle pane lists the items in the place you picked in the sidebar, or everything that matches a search.',
    how: [
      'The search box on top looks through the whole vault, whatever the sidebar shows. {{key:search}} jumps to it, Escape clears it, and [[concept.search]] lists the words that narrow it down.',
      'The sort button beside it changes the order, the layout (list, compact or grid) and the grouping, by site or by folder.',
      'The add button (+) offers the kinds of item set under [[page.add]]. {{key:add}} adds the first of them straight away.',
      ...LIST_ROWS,
      'Clicking a row opens the item, and [[setting.tap_action]] can make it copy something as well. Double-clicking does what [[setting.double_click]] says. The copy button on a row\'s right edge shows when you point at it.',
      'Right-click a row for **Copy password**, **Copy username**, **Copy code**, **Open site**, **Edit**, **Duplicate**, favorites, **Pin to the top**, **Move to hidden shelf** and **Move to trash**.',
      'Arrow keys move up and down the list. Hold Ctrl, or Cmd on a Mac, and click to choose several rows, or Shift-click to choose a run of them. The right pane then offers to add them to favorites, pin them, put them on the hidden shelf, or move them to the trash.',
    ],
    when: 'An empty vault offers **Add a login** and **Import**. A search that finds nothing offers **Add it as a login**, with what you typed as its name.',
    tour: 'tour.first_item',
    related: ['concept.search', 'page.list', 'page.add', 'screen.item', 'concept.item_kinds'],
    words: 'list items rows search sort layout multi select',
    on: {
      android: {
        what: 'The Vault tab lists your items, filtered by the chip you picked, or everything that matches a search.',
        how: [
          'Tap the search bar to search the whole vault. Before you type, it suggests words to start from and the items you opened last. [[concept.search]] lists the words that narrow it down.',
          'The chips under it filter the list by favorites or by kind.',
          ...LIST_ROWS,
          'Tap a row to open the item. Swipe a row right or left for a quick action, such as copying the password or moving it to the trash; [[page.list]] sets what each swipe does.',
          'Hold a row to start choosing several. The bar on top then moves them to a folder, tags them, changes their color, or moves them to the trash, with **Undo** after.',
          'The add button adds an item, of the kinds set under [[page.add]].',
        ],
        when: 'An empty vault offers to import from another app, or to add a first login or code. A search that finds nothing offers to add what you typed as a new login.',
      },
    },
  },
  {
    id: 'screen.item',
    kind: 'screen',
    title: 'Item view',
    what: 'The right pane shows the item you chose: its fields, ready to copy, and everything else it holds.',
    how: [
      'The bar on top has the star for favorites, **Edit** ({{key:edit}}), and **More**, which attaches a file, links another item, sets a reminder or a self-destruct date, moves it to a folder, changes its tags, color or icon, sends it to another device, duplicates or pins it, puts it on the hidden shelf, or moves it to the trash.',
      'Under the name, **Copy password** ({{key:copy_password}}), **Copy username** ({{key:copy_username}}) and **Open site** ({{key:open_site}}) do the most common things. [[page.entry]] chooses which of them show.',
      ...ITEM_FIELDS,
      'Click any field to copy it, unless you turned that off under [[setting.tap_copies]]. Each field also has its own copy button.',
      'An image attachment can be viewed. Any attachment can be saved as a copy, which asks first because the copy is not encrypted.',
    ],
    when: 'A copied secret is cleared from the clipboard after the time set under [[setting.clipboard_seconds]]. The message that says it was copied has **Clear now**.',
    caveats: [
      'An item set to self-destruct shows the date at the top, with **Keep it** to cancel.',
      'A trashed item shows **Restore** and **Delete for good** in place of the copy buttons.',
    ],
    related: ['page.entry', 'screen.editor', 'feature.clipboard', 'concept.item_kinds', 'feature.codes'],
    words: 'item detail entry fields copy reveal show history attachment',
    on: {
      android: {
        what: 'Tapping an item opens its page: its fields, ready to copy, and everything else it holds.',
        how: [
          'The bar on top has **Back**, the star for favorites, and **Edit**.',
          'Under the name, **Copy password**, **Copy username** and **Open site** do the most common things. [[page.entry]] chooses which of them show.',
          'Below come its fields. A secret shows as dots until you tap **Show**, and it hides again after the time set under [[setting.reveal_seconds]] or when you leave. Tap a field to copy it.',
          'A one-time code counts down with a ring and shows the next code in its last ten seconds. A counter-based code shows **New code** in place of the code.',
          'Further down are the websites, passkeys, fields of your own, security questions, notes, attachments and linked items, then **History** with earlier values of secret fields.',
          '**More**, at the bottom, adds a tag, attaches a file, links another item, sets a reminder, a folder, a color or a self-destruct date, pins it, puts it on the hidden shelf, duplicates it, sends it to another device, or moves it to the trash.',
        ],
        when: 'A copied secret is cleared from the clipboard after the time set under [[setting.clipboard_seconds]].',
      },
    },
  },
  {
    id: 'screen.editor',
    kind: 'screen',
    title: 'Item editor',
    what: 'Where a new item is filled in, or an existing one changed. Nothing is saved until you choose **Save**.',
    how: [
      ...EDITOR_PARTS,
      EDITOR_SECRETS,
      'A password field has a show button, and **Generate**, which opens the generator; **Use this** puts the result in the field. While you type a password, a meter under it shows how strong it is. A username field has **Use an alias**. An SSH key\'s private key field has **Generate a key**.',
      '**Save**, or Ctrl+S (Cmd+S on a Mac), saves and shows the item. **Close** leaves, and asks **Discard changes?** first if you changed anything.',
    ],
    when: '[[setting.editor_generate]] starts every new login with a generated password already in place.',
    tour: 'tour.first_item',
    related: ['page.editor', 'concept.item_kinds', 'feature.generator', 'feature.codes', 'feature.aliases'],
    words: 'editor edit new item add save fields',
    on: {
      android: {
        how: [
          ...EDITOR_PARTS,
          'A secret that is already saved shows as **Saved. Tap to change** and stays in the vault until you tap it, so fixing a name never brings the password onto the screen.',
          'A password field has **Show** and **Generate**, which opens the generator; **Use this** puts the result in the field. A username field has **Use an alias**. Under **One-time code**, **Scan a QR code** reads the code a site shows.',
          '**Save** saves and shows the item. **Close** leaves, and asks first if you changed anything.',
        ],
      },
    },
  },

  // ---- the other screens ----

  {
    id: 'screen.codes',
    kind: 'screen',
    title: 'Codes screen',
    what: 'Every one-time code in the vault on one screen, counting down, ready to copy.',
    how: [
      'Each code shows the site or issuer, the account, the code, and a ring for the time it has left. In its last ten seconds the next code shows under it.',
      'Click a code to copy it. Double-click to open its item.',
      'A counter-based code shows **Show the next code**, since each one shown is used up.',
      '[[page.codes]] sets the size of the codes, whether they are split in two groups, the next code, the ring, the order, and whether codes stay hidden until clicked. A hidden code shows for 15 seconds after you click it.',
    ],
    when: 'With no codes yet, **Add a code** starts one and **Bring them from another app** opens Settings, Import. {{key:codes}} opens this screen from anywhere.',
    caveats: 'A copied code is cleared from the clipboard like a password. Codes on the hidden shelf are not shown here.',
    tour: 'tour.codes',
    related: ['feature.codes', 'page.codes', 'page.import', 'concept.item_kinds'],
    words: 'codes totp 2fa authenticator one-time',
    on: {
      android: {
        how: [
          'Each code shows the site or issuer, the account, the code, and a ring for the time it has left. In its last ten seconds the next code shows under it.',
          'Tap a code to copy it. Hold it to open its item.',
          'A counter-based code shows **Show the next code**, since each one shown is used up.',
          'The add button adds a code, and can scan the QR code a site shows.',
          '[[page.codes]] sets the size of the codes, the next code, the ring, the order, and whether codes stay hidden until tapped. A hidden code shows for 15 seconds after you tap it.',
        ],
        when: 'With no codes yet, **Add a code** starts one and **Bring them from another app** opens Settings, Import.',
        caveats: 'A copied code is cleared from the clipboard like a password. Codes on the hidden shelf are not shown here.',
      },
    },
  },
  {
    id: 'screen.generator',
    kind: 'screen',
    title: 'Generator screen',
    what: 'Makes a new password, passphrase or PIN, and shows how strong it is.',
    how: [
      'The new value sits at the top. **Generate again**, the button beside it, makes another.',
      'Under it, a bar and a line show how long the password would take to crack, and the bits figure shows its strength as a number, if [[setting.gen_show_bits]] is on.',
      '**Copy** copies it, and the copy is cleared like any other secret. **Save as login** starts a new login with it as the password.',
      ...GENERATOR_TYPES,
    ],
    when: '{{key:generator}} opens this screen from anywhere. In the editor, **Generate** beside a password field opens the same choices.',
    caveats: 'A value is kept nowhere until you copy it or save it.',
    tour: 'tour.generator',
    related: ['feature.generator', 'page.generator', 'screen.editor'],
    words: 'generator password random passphrase pin new',
    on: {
      android: {
        how: [
          'The new value sits at the top. **Generate again** makes another.',
          'Under it, a bar and a line show how long the password would take to crack, and the bits figure shows its strength as a number, if [[setting.gen_show_bits]] is on.',
          '**Copy** copies it, and the copy is cleared like any other secret. **Save as login** starts a new login with it as the password.',
          ...GENERATOR_TYPES,
        ],
        when: 'In the editor, **Generate** beside a password field opens the same choices.',
      },
    },
  },
  {
    id: 'screen.health',
    kind: 'screen',
    title: 'Health screen',
    what: 'A score for your passwords, and a list of what to fix, worst first.',
    how: [
      ...HEALTH_PARTS,
      '**Check for breaches** opens Settings, Breaches, and **Compare two passwords** opens a side-by-side comparison.',
    ],
    when: '{{key:health}} opens this screen from anywhere. Look after an import, and whenever the score drops.',
    caveats: 'Leaked passwords show only after a breach check since the vault was last unlocked.',
    tour: 'tour.health',
    related: ['feature.health', 'page.health', 'feature.breaches', 'page.compare'],
    words: 'health score audit weak reused breached old duplicates',
    on: {
      android: {
        how: [
          ...HEALTH_PARTS,
          '**Check for breaches** opens Settings, Breaches, and **Compare two passwords** opens a side-by-side comparison.',
        ],
        when: 'Look after an import, and whenever the score drops.',
      },
    },
  },
  {
    id: 'screen.trash',
    kind: 'screen',
    title: 'Trash screen',
    what: 'Items you deleted, kept for a while so you can change your mind.',
    how: [
      'Open **Trash** in the sidebar to see them.',
      ...TRASH_PARTS,
      'Right-clicking a trashed item offers **Restore** and **Delete for good…**. **Empty** at the top deletes everything in the trash for good.',
      'Search finds trashed items when you add the word "trashed".',
    ],
    caveats: TRASH_CAVEATS,
    related: ['page.trash', 'screen.list', 'screen.item'],
    words: 'trash deleted bin restore undo empty',
    on: {
      android: {
        what: 'Settings, Trash holds items you deleted, kept for a while so you can change your mind.',
        how: [
          ...TRASH_PARTS,
          '**Empty the trash** deletes everything in it for good.',
          'Search finds trashed items with the **Trash** chip, or the word "trashed".',
        ],
      },
    },
  },
  {
    id: 'screen.shelf',
    kind: 'screen',
    title: 'Hidden shelf screen',
    what: 'Shows the items on the hidden shelf, once you open it with your master password.',
    how: [
      'While the shelf is closed, this screen asks for your master password, even though the vault is open. Type it and choose **Open the shelf**.',
      'Once open, its items list here and open like any other. **Close it** at the top puts them out of sight again, and so does locking.',
      'To take an item off the shelf, choose **Remove from hidden shelf** from its menu.',
      'Add **Hidden shelf** to the sidebar under [[page.navigation]] to reach this screen from there.',
    ],
    caveats: 'The shelf hides items. It does not encrypt them separately. See [[feature.shelf]].',
    related: ['feature.shelf', 'page.shelf', 'page.navigation'],
    words: 'hidden shelf private open',
    on: {
      android: {
        what: 'Settings, Hidden shelf shows the items on the shelf, once you open it with your master password.',
        how: [
          'While the shelf is closed, the page asks for your master password, even though the vault is open. Type it and tap **Open the shelf**.',
          'Once open, its items list here and open like any other. **Close the shelf** puts them out of sight again, and so does locking.',
          'To take an item off the shelf, choose **Remove from hidden shelf** under **More** on the item.',
        ],
        related: ['feature.shelf', 'page.shelf'],
      },
    },
  },
  {
    id: 'screen.palette',
    kind: 'screen',
    title: 'Quick search window',
    shells: ['desktop'],
    what: 'A small search box that opens over whatever you are doing, so you can copy a password without bringing up the main window.',
    how: [
      'Open it with the shortcut set under [[setting.global_palette]], from any program, or with **Quick Search** in the tray menu.',
      'Type part of a name. Up to eight matches show; the arrow keys move between them.',
      'Enter copies the password, or the code if the item has no password. Ctrl+Enter, or Cmd+Enter on a Mac, copies the username. With [[setting.auto_type_enabled]] on, Shift+Enter types the login into the window you were using.',
      'Copying closes the window, and so does Escape. It holds nothing of its own: it asks the main window\'s open vault each time.',
    ],
    when: 'For copying a password while you work in another program.',
    caveats: [
      'While the vault is locked it says "WM Pass is locked." and offers **Open WM Pass**.',
      'A copied password is cleared from the clipboard after the time set under [[setting.clipboard_seconds]].',
    ],
    tour: 'tour.palette',
    related: ['setting.global_palette', 'feature.autotype', 'page.desktop', 'concept.search'],
    docs: '/apps/desktop/',
    words: 'quick search palette global shortcut hotkey spotlight',
  },
  {
    id: 'screen.popup',
    kind: 'screen',
    title: 'Extension popup',
    shells: ['extension'],
    what: 'The small window that opens from the WM Pass button in the browser\'s toolbar. It lists the logins for the site you are on.',
    how: [
      'The top shows the site, and says **From the desktop app** when the logins come from the vault open there. The buttons beside it open WM Pass in a tab and lock the extension\'s own vault.',
      'Click a login to fill it into the page and close the popup. Only logins saved for this site can fill.',
      'With the extension\'s own vault open, **Search logins** finds any login by name, username or site, and each row has buttons to copy its username, password or code.',
      'With no vault yet, it offers **Open WM Pass**, to make one or connect the desktop app. A locked vault shows the unlock screen here.',
    ],
    when: 'The browser\'s shortcut for the popup starts as Ctrl+Shift+L, or Cmd+Shift+L on a Mac. Filling from the key in a login field does the same without opening it.',
    caveats: 'A fill through the desktop app may ask in the app first, once per site. If you refuse there, the popup says "Not filled."',
    related: ['feature.autofill', 'feature.bridge', 'screen.door', 'feature.passkeys'],
    docs: '/apps/extension/',
    words: 'popup toolbar extension fill browser',
  },
] as const satisfies readonly Topic[];
