// Settings, Settings file, About and Guide.

import type { Topic } from '../schema.ts';

export const app = [
  // ---- file ----

  {
    id: 'page.file',
    kind: 'page',
    title: 'Settings file',
    page: 'file',
    what: 'Saves your look and layout to a file, loads them from one, or puts them back as they started.',
    why: 'Setting up a new computer or phone the way you like it takes a while. A settings file carries it over, and you can share one with someone who likes your setup.',
    how: [
      'The file holds every customization: appearance and themes, icons, the vault list, the sidebar, the editor, the item page, codes, the generator, health checks and keyboard shortcuts. Nothing from your vault is in it.',
      'A file saved on the phone works on the computer, and the other way round. Settings one of them does not have are skipped.',
      'Protection settings never travel in a settings file. Lock timers, clipboard clearing, screenshot blocking, the network switch, the tray and start-up choices, the browser extension\'s connection, the SSH agent, auto-type, update checks and the quick search shortcut are left out when you save, and ignored when you import. A file from someone else cannot weaken them.',
    ],
    caveats: [
      'How long the trash keeps items is saved in the vault itself, so it is not in a settings file either.',
      'What the guide has already shown you, such as the welcome and finished tours, stays on this device.',
    ],
    related: ['page.look', 'page.themes', 'page.security', 'action.trash_keep'],
    words: 'export import backup reset preferences json share transfer customization',
    on: {
      android: {
        how: [
          'The file holds every customization: appearance and themes, icons, the vault list, navigation, the editor, the item page, codes, the generator, health checks and keyboard shortcuts. Nothing from your vault is in it.',
          'A file saved on the phone works on the computer, and the other way round. Settings one of them does not have are skipped.',
          'Protection settings never travel in a settings file. Lock timers, locking when the screen goes off, clipboard clearing, screenshot blocking, the network switch, autofill, WM Keyboard and watch access, and what a panic signal does are left out when you save, and ignored when you import. A file from someone else cannot weaken them.',
        ],
      },
    },
  },
  {
    id: 'action.settings_save_file',
    kind: 'action',
    title: 'Save to a file',
    page: 'file',
    what: 'Saves every customization as a JSON file, wm-pass-settings.json, wherever you choose.',
    related: ['page.file', 'action.settings_import'],
  },
  {
    id: 'action.settings_copy_text',
    kind: 'action',
    title: 'Copy as text',
    page: 'file',
    what: 'Copies the same settings to the clipboard as text, to paste into a message or a note. The text holds no secrets, so the clipboard is not cleared afterwards.',
    related: ['page.file', 'action.settings_import'],
    on: {
      android: {
        title: 'Send to an app',
        what: 'Opens the share sheet with the same settings as text, to send to another app or another person.',
      },
    },
  },
  {
    id: 'action.settings_import',
    kind: 'action',
    title: 'Import',
    page: 'file',
    what: 'Applies settings from pasted text or from a file, and says how many settings changed.',
    how: [
      'Paste the text and choose **Import**, or choose **Choose a file** to read a saved one.',
      'Settings this version does not know, and values it cannot use, are skipped. Themes in the file are checked like any theme you import, then added to your own themes, replacing any of yours with the same name.',
    ],
    caveats: 'Protection settings in the file are ignored, whoever made it.',
    related: ['page.file', 'page.themes'],
    words: 'load apply paste',
  },
  {
    id: 'action.settings_reset',
    kind: 'action',
    title: 'Reset all customization',
    page: 'file',
    what: 'Puts every customization back to how it started.',
    how: 'Your own themes stay, and so do protection settings such as lock timers. Nothing in the vault changes.',
    related: ['page.file'],
    words: 'defaults restore factory',
  },

  // ---- about ----

  {
    id: 'page.about',
    kind: 'page',
    title: 'About',
    page: 'about',
    what: 'Which version of WM Pass this is, and its license: free and open source, under the MIT license.',
    related: ['setting.network_allowed', 'page.updates'],
    words: 'version license license open source mit',
    on: {
      extension: {
        what: 'The license WM Pass is under: free and open source, under the MIT license.',
      },
    },
  },
  {
    id: 'action.about_source_code',
    kind: 'action',
    title: 'Source code',
    page: 'about',
    shells: ['desktop', 'extension'],
    what: 'Opens WM Pass\'s source code on GitHub in your browser.',
    words: 'github repository open source',
  },

  // ---- guide ----

  {
    id: 'page.guide',
    kind: 'page',
    title: 'Guide',
    page: 'guide',
    what: 'Every topic in the guide in one place: the tours, the screens, how WM Pass works, and each settings page with what is on it.',
    how: [
      'Type in **Search the guide** to find a topic by its words. The search at the top of Settings finds guide topics too.',
      '**Start here** shows the welcome again or takes you round the window. **Tours** lists the other tours, and marks the ones you have finished.',
      'Under each settings page, **About the page** explains the page, and opening its line lists the topics on it.',
      'The same topics open from the **?** beside a setting or a page title, and {{key:help}} explains whatever you are looking at.',
      'At the bottom, **The guide itself** has two settings: [[setting.help_buttons]] and [[setting.help_hints]].',
    ],
    caveats: 'What the guide has shown you, such as the welcome and finished tours, is remembered on this device only. It is not part of a settings file.',
    related: ['setting.help_buttons', 'setting.help_hints', 'page.file'],
    words: 'help manual tour tutorial explain question',
    on: {
      android: {
        how: [
          'The same topics open from the **?** beside a setting or a page title.',
          'Two settings decide how the guide shows itself: [[setting.help_buttons]] and [[setting.help_hints]].',
        ],
      },
    },
  },
  {
    id: 'action.show_welcome',
    kind: 'action',
    title: 'Show the welcome again',
    page: 'guide',
    shells: ['desktop', 'extension'],
    what: 'Opens the short welcome from your first unlock again: how to find, add and copy things, and how to ask what anything does.',
    how: '**Show me around** in it starts the tour of the window.',
    related: ['action.guide_take_tour', 'page.guide'],
    words: 'onboarding introduction start',
  },
  {
    id: 'action.guide_take_tour',
    kind: 'action',
    title: 'Take the tour',
    page: 'guide',
    shells: ['desktop', 'extension'],
    what: 'Points out the parts of this window one at a time.',
    how: 'A narrow window gets its own version, since its parts are laid out differently.',
    tour: 'tour.window',
    related: ['screen.window', 'action.show_welcome'],
    words: 'tour walkthrough introduction window',
  },
  {
    id: 'setting.help_buttons',
    kind: 'setting',
    title: 'Help buttons',
    page: 'guide',
    pref: 'help_buttons',
    what: 'Whether a **?** shows beside settings, group headings and page titles. Each one opens the guide\'s page about that thing.',
    effects: [
      { value: 'always', label: 'Always', text: 'Every **?** is on show all the time.' },
      { value: 'hover', label: 'On hover', text: 'The **?** beside a setting or a group heading appears while the pointer is over it, or while it has keyboard focus. The one beside a page title stays.' },
      { value: 'off', label: 'Off', text: 'No **?** anywhere. Right-click a setting and choose **What is this?**, or press {{key:help}} for the page you are on.' },
    ],
    related: ['page.guide', 'setting.help_hints'],
    words: 'question mark help explain buttons',
    on: {
      android: {
        how: 'A touch screen has no hover, so the phone has only two choices.',
        effects: [
          { value: 'always', label: 'Always', text: 'Every setting and page title the guide explains has a **?**.' },
          { value: 'off', label: 'Off', text: 'No **?** anywhere. Settings, Guide still lists every topic.' },
        ],
      },
    },
  },
  {
    id: 'setting.help_hints',
    kind: 'setting',
    title: 'Tips on new pages',
    page: 'guide',
    pref: 'help_hints',
    what: 'Shows a one-line note at the top of Codes, Health and the Hidden shelf the first time you open each one.',
    how: 'The note says what the page is for. **More** opens its guide page, **Tour** starts its tour where it has one, and the cross, **Do not show this again**, hides it. After any of these, that note does not come back on this device.',
    related: ['page.guide', 'setting.help_buttons'],
    words: 'hints tips first time notes',
    on: {
      android: {
        how: 'The note says what the page is for, with a way to read more. Once you have followed it or closed it, that note does not come back on this device.',
      },
    },
  },
] as const satisfies readonly Topic[];
