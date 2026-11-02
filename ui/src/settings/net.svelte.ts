// Network features: whether they may run, and what the last breach check found.

import { host } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';

/** Whether anything may use the network right now. */
export function online() {
  return prefs.v.network_allowed;
}

/** Whether this shell can make these calls at all. */
export function networkExists() {
  return host().network === true;
}

class BreachCounts {
  /** Entry id to times seen. Kept in memory only, so a locked vault says nothing. */
  counts = $state<Record<string, number>>({});
}

export const breachCounts = new BreachCounts();

pass.onWipe.push(() => {
  breachCounts.counts = {};
});
