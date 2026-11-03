// Settings, Tools: email aliases, the disposable inbox, the username checker and comparing two passwords.

import type { Topic } from '../schema.ts';

export const tools = [
  // ---- aliases ----

  {
    id: 'page.aliases',
    kind: 'page',
    title: 'Email aliases',
    page: 'aliases',
    what: 'Makes email addresses to give to sites instead of your real one. You can see which site passed an address on, and with a forwarding service, stop its mail without touching your inbox.',
    how: [
      'The first group works from the address you already have. It needs no service and no network.',
      'The second group uses a forwarding service you have an account with. Each alias it makes is a new address that forwards to your real one, and you can switch any of them off.',
    ],
    when: 'A plus tag is enough to see which site leaked or sold your address. A forwarding service is for when a site should never learn your real address at all.',
    figure: 'fig.alias',
    related: ['group.aliases.own', 'group.aliases.services', 'action.use_alias', 'feature.aliases'],
    words: 'masked email forwarding hide my email',
    on: {
      extension: {
        caveats: 'The browser extension does not make calls to forwarding services itself. Use the desktop app or the phone for those.',
      },
    },
  },

  // ---- with your own address ----

  {
    id: 'group.aliases.own',
    kind: 'concept',
    title: 'With your own address',
    page: 'aliases',
    what: 'Two ways to make your own address look different to each site. Mail sent to either still lands in your inbox.',
    how: [
      'A plus tag puts a word after your name: you+shop@gmail.com. Gmail, Fastmail and many other mail services deliver it to you.',
      'Gmail ignores dots in the name, so firstlast@gmail.com and first.la.st@gmail.com reach the same inbox.',
      'Both are worked out on this device. Nothing is sent anywhere, and they work with network features off.',
    ],
    caveats: 'Neither hides your real address. It is there to read in both, and a site that strips the tag or the dots has it as it is.',
    related: ['action.alias_plus', 'action.alias_dots', 'group.aliases.services'],
    words: 'plus addressing subaddressing gmail dots',
  },
  {
    id: 'action.alias_plus',
    kind: 'action',
    title: 'Plus tag',
    page: 'aliases',
    what: 'Adds the tag to your address after a "+", such as you+github@gmail.com.',
    how: [
      'The tag becomes lowercase letters, digits and dashes, and needs at least one letter or digit. A tag already in the address is replaced.',
      'Copy the result with the button beside it.',
    ],
    caveats: 'A site can remove the tag before it stores the address, and then it has your real one.',
    related: ['group.aliases.own', 'action.alias_dots'],
    words: 'plus addressing subaddress tag',
  },
  {
    id: 'action.alias_dots',
    kind: 'action',
    title: 'Gmail dots',
    page: 'aliases',
    what: 'Puts dots at random places in a Gmail address. Gmail ignores them, so the mail still reaches you.',
    how: 'Each click places the dots differently. Only gmail.com and googlemail.com addresses are accepted, because other services treat a dot as part of the name.',
    caveats: 'A site that removes the dots has your real address.',
    related: ['group.aliases.own', 'action.alias_plus'],
    words: 'gmail dot trick googlemail',
  },

  // ---- forwarding services ----

  {
    id: 'group.aliases.services',
    kind: 'concept',
    title: 'Forwarding services',
    page: 'aliases',
    what: 'Makes a new address at a service you have an account with. Mail to it is forwarded to your real inbox until you switch it off.',
    why: 'The site never sees your real address. If it leaks or sells the alias, you switch that one alias off and nothing else changes.',
    how: [
      'Choose the service, paste the API key from your account there, and click **Keep this key in the vault**. From then on the page can make aliases and list the ones you have.',
      'The key is saved as an API key item in your vault, named after the service, such as "SimpleLogin API key". For each call, WM Pass reads it from the vault and wipes its copy straight after.',
      'With a key kept, opening the page or choosing the service asks it for your aliases. DuckDuckGo is the exception: it cannot list them.',
      'The service you choose receives your API key with every call, and the note you write when you make an alias. Each service says exactly where its calls go.',
    ],
    caveats: [
      'Every mail to your aliases passes through the service, so it can read them. Choose one you trust with your mail.',
      'WM Pass finds the key by its item\'s name. Rename or delete that item and the page asks for a key again.',
      'Needs [[setting.network_allowed]].',
    ],
    related: ['concept.alias_simplelogin', 'concept.alias_addy', 'concept.alias_firefox_relay', 'concept.alias_duckduckgo', 'action.alias_keep_key', 'feature.aliases'],
    words: 'simplelogin addy anonaddy relay duckduckgo api key forwarding',
  },
  {
    id: 'concept.alias_simplelogin',
    kind: 'concept',
    title: 'SimpleLogin',
    page: 'aliases',
    what: 'Makes aliases at SimpleLogin with an API key you create under API Keys in your SimpleLogin account.',
    how: [
      'Every call goes to app.simplelogin.io with your API key. Making an alias also sends the note you wrote, which SimpleLogin saves with it.',
      'Each new alias is made of random words. Your aliases are listed on the page, and each one can be switched off.',
    ],
    related: ['group.aliases.services', 'action.alias_forwarding'],
  },
  {
    id: 'concept.alias_addy',
    kind: 'concept',
    title: 'addy.io',
    page: 'aliases',
    what: 'Makes aliases at addy.io with an API key you create in addy.io\'s settings.',
    how: [
      'Every call goes to app.addy.io with your API key. Making an alias also sends your note, which addy.io saves as the alias\'s description.',
      'Each new alias is made of random words on the anonaddy.me domain. The page lists up to 100 of your aliases, and each one can be switched off.',
    ],
    related: ['group.aliases.services', 'action.alias_forwarding'],
    words: 'anonaddy',
  },
  {
    id: 'concept.alias_firefox_relay',
    kind: 'concept',
    title: 'Firefox Relay',
    page: 'aliases',
    what: 'Makes Relay addresses with the API key from your Firefox Relay settings.',
    how: [
      'Every call goes to relay.firefox.com with your API key. Making an address also sends your note, which Relay saves as its description and as the site it was made for.',
      'Your Relay addresses are listed on the page, and each one can be switched off.',
    ],
    related: ['group.aliases.services', 'action.alias_forwarding'],
    words: 'mozilla relay mask',
  },
  {
    id: 'concept.alias_duckduckgo',
    kind: 'concept',
    title: 'DuckDuckGo',
    page: 'aliases',
    what: 'Makes a Duck address with the token from your DuckDuckGo Email Protection dashboard. It is experimental.',
    how: 'DuckDuckGo has no public API, so this uses the private one its own extension calls, at quack.duckduckgo.com. Only the token is sent: your note stays on this device. The new address ends in @duck.com.',
    caveats: [
      'It may stop working whenever DuckDuckGo changes that private API.',
      'Duck addresses cannot be listed or switched off here. Manage them from DuckDuckGo.',
    ],
    related: ['group.aliases.services'],
    words: 'duck email protection duck.com',
  },
  {
    id: 'action.alias_keep_key',
    kind: 'action',
    title: 'Keep this key in the vault',
    page: 'aliases',
    what: 'Saves the service\'s API key as an item in your vault, so the page can use it.',
    how: 'The item is an API key named after the service, such as "addy.io API key". It is encrypted, synced and backed up with the rest of the vault, and you can open it from the vault list like any other item.',
    caveats: 'Anyone with this key can make aliases in your account and switch them off. If you think it has leaked, revoke it at the service and keep a new one here.',
    related: ['group.aliases.services'],
    words: 'api key token save',
  },
  {
    id: 'action.alias_make',
    kind: 'action',
    title: 'Make a new alias',
    page: 'aliases',
    what: 'Asks the service for a new alias, with what you typed under "What is it for?" as its note, and copies the new address.',
    how: 'The address goes on the clipboard, and into the list on the page for services that can list them. A note such as the site\'s name helps you tell your aliases apart later.',
    related: ['group.aliases.services', 'action.use_alias'],
  },
  {
    id: 'action.alias_forwarding',
    kind: 'action',
    title: 'Forwarding on',
    page: 'aliases',
    what: 'The switch beside each alias. Off stops its mail reaching you; on starts it again.',
    how: 'The list reloads after each change, so it always shows what the service says.',
    caveats: 'SimpleLogin only offers a switch that flips the alias. If it was changed somewhere else since the page loaded, it can end up the other way: check the list after it reloads.',
    related: ['group.aliases.services'],
    words: 'disable alias stop mail',
  },
  {
    id: 'action.use_alias',
    kind: 'action',
    title: 'Use an alias',
    page: 'aliases',
    what: 'The button beside a login\'s username in the editor. It makes a new alias at a service you have set up and puts it in the field.',
    how: [
      'Every service with a key kept in the vault is offered. The item\'s name is sent as the alias\'s note, so you can tell later which site it is for.',
      'With no service set up yet, it offers **Set one up**, which opens Settings, Email aliases.',
    ],
    when: 'When you sign up somewhere new and want the site never to have your real address.',
    caveats: 'Needs [[setting.network_allowed]].',
    related: ['group.aliases.services', 'action.alias_make'],
    words: 'editor username new login',
  },

  // ---- disposable inbox ----

  {
    id: 'page.inbox',
    kind: 'page',
    title: 'Disposable inbox',
    page: 'inbox',
    shells: ['desktop', 'android'],
    what: 'Makes a throwaway address on a public mail service and shows the mail that arrives, for a sign-up that only needs one confirmation mail.',
    why: 'Some sites want an address before they show you anything. A throwaway one keeps your own address, and your aliases, out of their lists.',
    how: [
      'Choose mail.tm or Guerrilla Mail and click **Make an address**. Copy the address into the sign-up form, and what is sent to it appears under **Arrived**.',
      'When you leave the page, WM Pass forgets the address and the key to its mailbox. You cannot open that inbox again.',
    ],
    when: 'For a one-off, such as a download link or a trial. Never for an account you want to keep: a password reset would go to an inbox you can no longer open.',
    caveats: [
      'The service, and possibly others, can read what arrives. Treat anything sent there as public.',
      'WM Pass does not delete the mailbox when you leave. It stays at the service, under that service\'s own rules.',
      'Needs [[setting.network_allowed]].',
    ],
    related: ['concept.inbox_mailtm', 'concept.inbox_guerrilla', 'page.aliases'],
    words: 'temporary throwaway burner email disposable',
  },
  {
    id: 'concept.inbox_mailtm',
    kind: 'concept',
    title: 'mail.tm',
    page: 'inbox',
    shells: ['desktop', 'android'],
    what: 'A free mailbox service. WM Pass makes a new account there for each address.',
    how: [
      'WM Pass asks api.mail.tm which domains it offers, then creates a mailbox with a random name and a random password, and signs in to it. Nothing about you is in either.',
      'While the page is open, it asks api.mail.tm for new mail with the sign-in key. Opening a mail fetches its full text.',
    ],
    related: ['page.inbox', 'concept.inbox_guerrilla'],
  },
  {
    id: 'concept.inbox_guerrilla',
    kind: 'concept',
    title: 'Guerrilla Mail',
    page: 'inbox',
    shells: ['desktop', 'android'],
    what: 'A free throwaway mail service that hands out an address with no account.',
    how: [
      'WM Pass asks api.guerrillamail.com for an address and gets a session key with it, which it sends with each check for new mail.',
      'Its API wants an IP address and a program name with every call, so WM Pass sends 127.0.0.1 and "WM-Pass" instead of anything real.',
      'Opening a mail shows the opening lines that came with the list, not the whole message.',
    ],
    caveats: 'Those fixed values keep your real address out of the request itself. The service still sees where the connection comes from.',
    related: ['page.inbox', 'concept.inbox_mailtm'],
  },
  {
    id: 'group.inbox.arrived',
    kind: 'concept',
    title: 'Arrived',
    page: 'inbox',
    shells: ['desktop', 'android'],
    what: 'The mail sent to the address so far. Click one to read it below the list.',
    how: [
      'The page asks the service for new mail every 8 seconds while it is open.',
      'Mail is shown as plain text only, so no picture, tracking pixel or script in it ever loads or runs here.',
    ],
    related: ['page.inbox'],
  },

  // ---- usernames ----

  {
    id: 'page.usernames',
    kind: 'page',
    title: 'Username checker',
    page: 'usernames',
    what: 'Asks a list of sites, one at a time, whether a username is taken there.',
    why: 'If you want the same name everywhere, this saves trying it on each site by hand.',
    how: [
      'For most sites, WM Pass asks for that name\'s public profile and reads whether one comes back.',
      'TikTok and Instagram only answer a browser. Their pages load in a hidden, private browser view that runs their scripts, may only load pages on that site, and keeps no cookies or storage afterwards. WM Pass reads only the text the page shows.',
      'X, Threads, Reddit and a few others refuse, or give the same answer for every name. They are listed under **Look for yourself**, with a link to the profile.',
      'Each site you check learns the name you looked up. Nothing from your vault goes with it.',
    ],
    when: 'Before you settle on a handle for a new account.',
    caveats: [
      'A name can only be letters, digits, dots, dashes and underscores, up to 64 characters, since none of these sites allow others.',
      'Needs [[setting.network_allowed]].',
    ],
    related: ['action.username_check', 'group.usernames.links', 'feature.usernames'],
    words: 'handle availability taken free name',
    on: {
      extension: {
        caveats: [
          'The extension has no network of its own. While it is connected to the desktop app, the app makes every check for it, and only while network features are allowed there.',
          'If nothing is connected, open the desktop app and connect it under Settings, Desktop app.',
          'A name can only be letters, digits, dots, dashes and underscores, up to 64 characters.',
        ],
      },
    },
  },
  {
    id: 'action.username_check',
    kind: 'action',
    title: 'Check',
    page: 'usernames',
    what: 'Asks every site in turn about the name in the box. The answers fill in as they come.',
    how: [
      '- *Free* means the site showed no profile under that name.',
      '- *Taken* means somebody has it.',
      '- *No clear answer* means the site refused, took too long, or said something WM Pass could not read.',
    ],
    caveats: [
      'Free means no profile was found today. Signing up is the only way to be sure you can have the name.',
      'A site that limits how often it can be asked gives no clear answer until you try again later.',
    ],
    related: ['page.usernames', 'group.usernames.links'],
  },
  {
    id: 'group.usernames.links',
    kind: 'concept',
    title: 'Look for yourself',
    page: 'usernames',
    what: 'Sites whose answer WM Pass cannot read, each with a link to the profile the name would have. Click one to open it in your browser.',
    how: 'They need a sign-in, or show the same page for a free name as for a suspended one, so a guess would mislead. The links are made on this device; nothing reaches those sites until you open one.',
    related: ['page.usernames'],
  },

  // ---- compare ----

  {
    id: 'page.compare',
    kind: 'page',
    title: 'Compare two passwords',
    page: 'compare',
    what: 'Type two passwords to see which would take longer to guess, and by roughly how much.',
    how: [
      'Each password gets a strength bar and a time it would hold for, against someone making 10,000 guesses a second, as they could against a password stored with a slow hash.',
      'The line at the bottom says how many times harder the stronger one is to guess. When they are less than ten times apart, it says **About the same.**',
      'The estimate comes from zxcvbn, which looks for the patterns people use rather than only counting characters.',
      'Both passwords stay on this device. Nothing is saved or sent, and the copies made for the check are wiped straight after.',
    ],
    when: 'When you are choosing between two passwords you made up, or wondering whether one more word is worth it.',
    caveats: [
      'It cannot know whether a password has already leaked. [[page.breaches]] checks that.',
      'For a password the generator made, its strength in bits is the true figure. This estimate can only guess.',
    ],
    related: ['page.breaches', 'page.generator', 'feature.health'],
    words: 'strength entropy zxcvbn crack time stronger',
  },
] as const satisfies readonly Topic[];
