import type { IconName } from '../engine/icons.svelte';

/** Every place the sidebar can list. Which ones it does, and in what order, is a setting. */
export const TAB_INFO: Record<string, { label: string; icon: IconName }> = {
  all: { label: 'All items', icon: 'inventory_2' },
  favourites: { label: 'Favourites', icon: 'star' },
  codes: { label: 'Codes', icon: 'pin' },
  generator: { label: 'Generator', icon: 'casino' },
  health: { label: 'Health', icon: 'ecg_heart' },
  trash: { label: 'Trash', icon: 'delete' },
  shelf: { label: 'Hidden shelf', icon: 'visibility_lock' },
};
