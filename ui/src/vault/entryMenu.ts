import type { MenuItem } from '../engine/host';
import { pass } from '../engine/pass.svelte';
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
  // A change the vault refuses, such as one opened read-only, says so rather than doing nothing.
  try {
    await run(choice, entry);
  } catch (failure: any) {
    overlays.toast(failure?.message ?? 'That could not be done.');
  }
}

async function run(choice: string, entry: Summary) {
  switch (choice) {
    case 'restore':
      await pass.change('entry.restore', { id: entry.id });
      // Gone from the trash list, so not left open there as a live item.
      if (ui.section === 'trash') ui.deselect(entry.id);
      overlays.toast(`“${entry.title || 'Untitled'}” is back.`);
      break;
    case 'purge': {
      const yes = await overlays.ask({
        title: `Delete “${entry.title || 'Untitled'}” for good?`,
        body: 'Its password, history and attachments go with it. A backup made before now would still hold them.',
        action: 'Delete for good',
        danger: true,
      });
      if (yes) {
        await pass.change('entry.purge', { id: entry.id });
        ui.deselect(entry.id);
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
      if (!entry.hidden) ui.deselect(entry.id);
      // It leaves the list either way, so it says where it went, as several at once do.
      overlays.toast(entry.hidden ? `“${entry.title || 'Untitled'}” is off the hidden shelf.` : `“${entry.title || 'Untitled'}” moved to the hidden shelf.`);
      break;
    default:
      await ui.perform(choice, entry);
  }
}
