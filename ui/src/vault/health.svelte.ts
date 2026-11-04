// The health report with the owner's choices: items ignored per check, and local addresses
// left out. Every screen that shows the report asks for it here, so they agree.

import { pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
import type { HealthView } from '../engine/types';
import { breachCounts } from '../settings/net.svelte';

/** Each check's name, as the Health page and its settings call it. */
export const CHECK_NAMES: Record<string, string> = {
  breached: 'In a known breach',
  reused: 'Used more than once',
  weak: 'Easy to guess',
  insecure: 'Unencrypted sites',
  expiring: 'Expiring soon',
  old: 'Not changed in a year',
  twofactor: 'No second factor',
  duplicates: 'Duplicate items',
};

/** Items left out of one health check, by check, as the person chose them. */
export function ignoredChecks(): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const pair of prefs.listOf('health_ignored')) {
    const at = pair.indexOf(':');
    if (at <= 0) continue;
    (out[pair.slice(0, at)] ??= []).push(pair.slice(at + 1));
  }
  return out;
}

export function ignoreHealth(check: string, id: string, ignore: boolean) {
  const all = new Set(prefs.listOf('health_ignored'));
  if (ignore) all.add(`${check}:${id}`);
  else all.delete(`${check}:${id}`);
  prefs.setList('health_ignored', [...all].sort());
}

class HealthMemory {
  /** The last report, so coming back to Health finds the list where it was instead of empty. */
  last = $state<HealthView | null>(null);
  /** Groups left open, so coming back from an item finds them as they were. */
  open = $state<string[]>([]);
}

export const healthMemory = new HealthMemory();

pass.onWipe.push(() => {
  healthMemory.last = null;
});

export async function healthNow(): Promise<HealthView> {
  const report = await pass.health({ ...breachCounts.counts }, ignoredChecks(), prefs.v.health_skip_local);
  healthMemory.last = report;
  return report;
}
