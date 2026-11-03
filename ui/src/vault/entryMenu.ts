import type { MenuItem } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { host } from '../engine/host';
import type { Summary } from '../engine/types';
import { overlays } from '../parts/floating.svelte';
import { ui } from './ui.svelte';

/** A row's right-click menu. */
export function entryMenu(entry: Summary): MenuItem[] {
  if (entry.trashed) {
    return [
      { id: 'restore', label: 'Restore' },
      { separator: true },
      { id: 'purge', label: 'Delete for good…', danger: true },
    ];
  }
  const items: MenuItem[] = [];
  if (entry.has_password) items.push({ id: 'password', label: 'Copy password' });
  if (ui.canDo('username', entry)) items.push({ id: 'username', label: 'Copy username' });
  if (entry.has_otp) items.push({ id: 'code', label: 'Copy code' });
  if (entry.site) items.push({ id: 'open_site', label: 'Open site' });
  if (items.length) items.push({ separator: true });
  items.push(
    { id: 'edit', label: 'Edit' },
    { id: 'duplicate', label: 'Duplicate' },
    { id: 'favourite', label: entry.favourite ? 'Remove from favorites' : 'Add to favorites' },
    { id: 'pin', label: entry.pinned ? 'Unpin' : 'Pin to the top' },
    { id: 'hide', label: entry.hidden ? 'Remove from hidden shelf' : 'Move to hidden shelf' },
    { separator: true },
    { id: 'trash', label: 'Move to trash', danger: true },
  );
  return items;
}

export async function runEntryMenu(choice: string, entry: Summary) {
  switch (choice) {
    case 'restore':
      await pass.change('entry.restore', { id: entry.id });
      break;
    case 'purge': {
      const yes = await overlays.ask({
        title: `Delete “${entry.title}” for good?`,
        body: 'Its password, history and attachments go with it. A backup made before now would still hold them.',
        action: 'Delete for good',
        danger: true,
      });
      if (yes) {
        await pass.change('entry.purge', { id: entry.id });
        if (ui.selected === entry.id) ui.selected = null;
      }
      break;
    }
    case 'duplicate': {
      const made = await pass.change<{ id: string }>('entry.duplicate', { id: entry.id });
      ui.open(made.json.id);
      break;
    }
    case 'pin':
      await pass.change('entry.common', { id: entry.id, pinned: !entry.pinned });
      break;
    case 'hide':
      await pass.change('entry.common', { id: entry.id, hidden: !entry.hidden });
      if (!entry.hidden && ui.selected === entry.id) ui.selected = null;
      break;
    default:
      await ui.perform(choice, entry);
  }
  void host;
}
