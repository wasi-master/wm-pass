// Diagrams for the guide. Rows of plain boxes, top to bottom, and arrows between neighbours or
// between one row and the next. Each platform lays them out in its own fonts. Labels stay short:
// a box says what it is, its sub line says one fact about it.

import type { Figure } from './schema.ts';

export const FIGURES: readonly Figure[] = [
  {
    id: 'fig.vault',
    caption: 'Every way in opens the same random vault key. Changing a password rewraps that key; rekeying makes a new one.',
    rows: [
      [
        { id: 'password', label: 'Password' },
        { id: 'file', label: 'Password and key file' },
        { id: 'hardware', label: 'Password and security key', icon: 'security_key' },
      ],
      [{ id: 'key', label: 'Vault key', sub: 'Random, made with the vault', tone: 'good' }],
      [{ id: 'items', label: 'Your items', sub: 'Encrypted with the vault key', icon: 'inventory_2' }],
    ],
    edges: [
      { from: 'password', to: 'key' },
      { from: 'file', to: 'key' },
      { from: 'hardware', to: 'key' },
      { from: 'key', to: 'items', label: 'opens' },
    ],
  },
  {
    id: 'fig.lock',
    caption: 'Locking forgets the key. Getting back in takes the password or another way in.',
    rows: [
      [
        { id: 'idle', label: 'No activity', sub: 'for the time you chose' },
        { id: 'sleep', label: 'Sleep or screen off', sub: 'if that is on' },
        { id: 'you', label: 'You lock it' },
      ],
      [{ id: 'locked', label: 'Locked', sub: 'the key is forgotten', tone: 'good' }],
      [{ id: 'door', label: 'The door', icon: 'lock' }],
    ],
    edges: [
      { from: 'idle', to: 'locked' },
      { from: 'sleep', to: 'locked' },
      { from: 'you', to: 'locked' },
      { from: 'locked', to: 'door' },
    ],
  },
  {
    id: 'fig.clipboard',
    caption: 'A copied secret is cleared after the time you chose, but only if it is still on the clipboard.',
    rows: [
      [
        { id: 'copy', label: 'You copy a password' },
        { id: 'clip', label: 'Clipboard', sub: 'kept out of its history', icon: 'content_paste' },
        { id: 'paste', label: 'You paste it' },
      ],
      [
        { id: 'clear', label: 'Cleared', sub: 'when the time is up', tone: 'good' },
        { id: 'other', label: 'Left alone', sub: 'if you copied something else', tone: 'muted' },
      ],
    ],
    edges: [
      { from: 'copy', to: 'clip' },
      { from: 'clip', to: 'paste' },
      { from: 'clip', to: 'clear' },
      { from: 'clip', to: 'other', dashed: true },
    ],
  },
  {
    id: 'fig.duress',
    caption: 'Every password looks the same at the door. What happens depends on which one is typed.',
    rows: [
      [{ id: 'door', label: 'The door', icon: 'lock' }],
      [
        { id: 'real', label: 'Your password', sub: 'opens your vault', tone: 'good' },
        { id: 'decoy', label: 'Decoy password', sub: 'opens the decoy' },
        { id: 'lockout', label: 'Lockout password', sub: 'refuses all for an hour', tone: 'warn' },
        { id: 'wipe', label: 'Wipe password', sub: 'erases this copy', tone: 'danger' },
      ],
    ],
    edges: [
      { from: 'door', to: 'real' },
      { from: 'door', to: 'decoy' },
      { from: 'door', to: 'lockout' },
      { from: 'door', to: 'wipe' },
    ],
  },
  {
    id: 'fig.kit',
    caption: 'Here, any two of three. Fewer shares than that number reveal nothing at all.',
    rows: [
      [
        { id: 'a', label: 'Share 1', icon: 'person' },
        { id: 'b', label: 'Share 2', icon: 'person' },
        { id: 'c', label: 'Share 3', sub: 'not needed', icon: 'person', tone: 'muted' },
      ],
      [{ id: 'join', label: 'Any two together', sub: 'with a copy of the vault file' }],
      [{ id: 'open', label: 'A new password', sub: 'opens the vault', tone: 'good' }],
    ],
    edges: [
      { from: 'a', to: 'join' },
      { from: 'b', to: 'join' },
      { from: 'c', to: 'join', dashed: true },
      { from: 'join', to: 'open' },
    ],
  },
  {
    id: 'fig.shelf',
    caption: 'Items on the shelf stay out of sight until you open it with your password. It closes when the vault locks.',
    rows: [
      [{ id: 'vault', label: 'Your vault' }],
      [
        { id: 'lists', label: 'Lists, search, autofill', sub: 'everything else' },
        { id: 'shelf', label: 'Hidden shelf', sub: 'closed', icon: 'visibility_lock', tone: 'muted' },
      ],
      [{ id: 'password', label: 'Master password', sub: 'opens it until you lock' }],
    ],
    edges: [
      { from: 'vault', to: 'lists' },
      { from: 'vault', to: 'shelf', dashed: true },
      { from: 'password', to: 'shelf' },
    ],
  },
  {
    id: 'fig.backup',
    caption: 'A backup is the encrypted vault file itself, so only your password opens it, wherever it is kept.',
    rows: [
      [{ id: 'vault', label: 'Vault file', sub: 'already encrypted', icon: 'draft' }],
      [{ id: 'place', label: 'A place', sub: 'folder, WebDAV, S3, cloud drive', icon: 'cloud' }],
      [{ id: 'check', label: 'Read back and checked', tone: 'good' }],
      [{ id: 'prune', label: 'Oldest copies removed', sub: 'keeping as many as you chose' }],
    ],
    edges: [
      { from: 'vault', to: 'place', label: 'copied' },
      { from: 'place', to: 'check' },
      { from: 'check', to: 'prune' },
    ],
  },
  {
    id: 'fig.sync',
    caption: 'Each device writes its own encrypted file to the shared place and merges the others into its vault.',
    rows: [
      [
        { id: 'a', label: 'This device', icon: 'computer' },
        { id: 'b', label: 'Another device', icon: 'devices' },
      ],
      [{ id: 'place', label: 'Shared place', sub: 'one file per device', icon: 'cloud' }],
      [{ id: 'merge', label: 'Merged', sub: 'field by field', tone: 'good' }],
    ],
    edges: [
      { from: 'a', to: 'place' },
      { from: 'b', to: 'place' },
      { from: 'place', to: 'merge' },
    ],
  },
  {
    id: 'fig.lan',
    caption: 'Pairing happens once, on the same network. After that the two devices know each other by their keys.',
    rows: [
      [
        { id: 'phone', label: 'Phone', icon: 'devices' },
        { id: 'computer', label: 'Computer', icon: 'computer' },
      ],
      [{ id: 'code', label: 'The same code on both', sub: 'you compare it' }],
      [{ id: 'paired', label: 'Paired', sub: 'later syncs need no code', tone: 'good' }],
    ],
    edges: [
      { from: 'phone', to: 'code' },
      { from: 'computer', to: 'code' },
      { from: 'code', to: 'paired' },
    ],
  },
  {
    id: 'fig.vaults',
    caption: 'A nested vault is a separate file opened from inside yours. A shared vault has its own key, which each member holds.',
    rows: [
      [{ id: 'main', label: 'Your vault' }],
      [
        { id: 'nested', label: 'Nested vault', sub: 'a separate file' },
        { id: 'shared', label: 'Shared vault', sub: 'its own key' },
      ],
      [{ id: 'members', label: 'Members', sub: 'each with their own copy of the key', icon: 'group' }],
    ],
    edges: [
      { from: 'main', to: 'nested' },
      { from: 'main', to: 'shared' },
      { from: 'shared', to: 'members' },
    ],
  },
  {
    id: 'fig.bridge',
    caption: 'The extension and the app talk over an encrypted link, set up once by comparing a fingerprint.',
    rows: [
      [
        { id: 'page', label: 'Web page' },
        { id: 'extension', label: 'Extension', icon: 'extension' },
      ],
      [{ id: 'relay', label: 'Relay program', sub: 'carries sealed messages, holds no keys' }],
      [{ id: 'app', label: 'WM Pass', sub: 'answers only for the site you are on', icon: 'computer' }],
    ],
    edges: [
      { from: 'page', to: 'extension' },
      { from: 'extension', to: 'relay' },
      { from: 'relay', to: 'app' },
    ],
  },
  {
    id: 'fig.ssh',
    caption: 'The key signs inside WM Pass and never leaves the vault. When the vault locks, the agent has nothing to offer.',
    rows: [
      [{ id: 'client', label: 'ssh or git', icon: 'terminal' }],
      [{ id: 'socket', label: 'Agent socket', sub: 'SSH_AUTH_SOCK' }],
      [{ id: 'ask', label: 'WM Pass asks you', sub: 'if that is on' }],
      [{ id: 'sign', label: 'Signed', sub: 'with a key from the vault', tone: 'good' }],
    ],
    edges: [
      { from: 'client', to: 'socket' },
      { from: 'socket', to: 'ask' },
      { from: 'ask', to: 'sign' },
    ],
  },
  {
    id: 'fig.autotype',
    caption: 'Auto-type presses the keys for you. Typed text passes through the system, where other programs can see it.',
    rows: [
      [{ id: 'palette', label: 'Quick search', sub: 'Shift+Enter on a login' }],
      [{ id: 'queue', label: 'System input', sub: 'other programs can see it', tone: 'warn' }],
      [{ id: 'window', label: 'The window in front', sub: 'username, Tab, password' }],
    ],
    edges: [
      { from: 'palette', to: 'queue' },
      { from: 'queue', to: 'window' },
    ],
  },
  {
    id: 'fig.breach',
    caption: 'Have I Been Pwned never sees your password or even its full hash. The match is made on this device.',
    rows: [
      [{ id: 'password', label: 'Your password' }],
      [{ id: 'hash', label: 'SHA-1 hash', sub: 'made on this device' }],
      [
        { id: 'prefix', label: 'First 5 characters', sub: 'sent' },
        { id: 'rest', label: 'The rest', sub: 'stays here', tone: 'muted' },
      ],
      [{ id: 'answer', label: 'Leaked hashes that start the same', sub: 'sent back, hundreds of them' }],
      [{ id: 'match', label: 'Compared here', tone: 'good' }],
    ],
    edges: [
      { from: 'password', to: 'hash' },
      { from: 'hash', to: 'prefix' },
      { from: 'hash', to: 'rest', dashed: true },
      { from: 'prefix', to: 'answer' },
      { from: 'answer', to: 'match' },
    ],
  },
  {
    id: 'fig.alias',
    caption: 'The site only ever sees the alias. Turn the alias off and its mail stops, without touching your real address.',
    rows: [
      [{ id: 'site', label: 'A website' }],
      [{ id: 'alias', label: 'An alias', sub: 'made for that site' }],
      [{ id: 'service', label: 'Forwarding service', sub: 'SimpleLogin, addy.io and others' }],
      [{ id: 'inbox', label: 'Your real inbox', icon: 'mail' }],
    ],
    edges: [
      { from: 'site', to: 'alias', label: 'sends mail to' },
      { from: 'alias', to: 'service' },
      { from: 'service', to: 'inbox', label: 'forwards' },
    ],
  },
];
