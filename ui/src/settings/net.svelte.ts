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
  /**
   * Entry id to times seen. Kept in memory only, and only while the vault is open: a list on
   * disk of which passwords have leaked would be a gift to anyone who took the computer.
   */
  counts = $state<Record<string, number>>({});
}

export const breachCounts = new BreachCounts();

export type BreachSource = 'hibp' | 'xon';

export function breachSourceName(source: string) {
  return source === 'xon' ? 'XposedOrNot' : 'Have I Been Pwned';
}

/** The last finished check: when, with which service, and what it found. */
export interface BreachLast {
  atMs: number;
  source: string;
  found: number;
  checked: number;
}

export function breachLast(): BreachLast | null {
  const parts = prefs.v.breach_last.split('|');
  if (parts.length < 4) return null;
  const atMs = Number(parts[0]);
  if (!Number.isFinite(atMs) || atMs <= 0) return null;
  return { atMs, source: parts[1], found: Number(parts[2]) || 0, checked: Number(parts[3]) || 0 };
}

/**
 * The password breach check, run for the whole app rather than for one page, so leaving the
 * page does not stop it. What it finds stays in memory; only when it ran, with which service
 * and how much it found are kept, so the page can say so.
 */
class BreachCheck {
  running = $state(false);
  done = $state(0);
  total = $state(0);
  source = $state<BreachSource>('hibp');
  problem = $state<string | null>(null);
  private stopping = false;

  /** Checks every password in `ids` with `source`. Does nothing while a check is running. */
  async start(ids: string[], source: BreachSource) {
    if (this.running || ids.length === 0) return;
    this.running = true;
    this.stopping = false;
    this.problem = null;
    this.source = source;
    this.total = ids.length;
    this.done = 0;
    const counts: Record<string, number> = {};
    let problem: string | null = null;
    try {
      for (const id of ids) {
        if (this.stopping) {
          problem = `Stopped after ${this.done} of ${ids.length}.`;
          break;
        }
        const reply = await pass.engine('entry.reveal', { id, field: 'password' }, { handle: pass.handle });
        const secret = reply.secret;
        try {
          const check = await pass.engine<{ count: number }>('breach.password', { source }, { handle: pass.handle, secret, online: online() });
          if (check.json.count > 0) counts[id] = check.json.count;
        } finally {
          secret?.fill(0);
        }
        this.done++;
      }
    } catch (failure: any) {
      problem =
        pass.state.kind !== 'unlocked'
          ? `The vault locked before the check finished. Checked ${this.done} of ${ids.length}.`
          : (failure?.message ?? 'The check stopped.');
    }
    const checked = this.done;
    if (checked > 0) {
      // What a part-finished check found still counts; the rest keep their earlier answer. A
      // vault that locked meanwhile keeps nothing.
      if (pass.state.kind === 'unlocked') breachCounts.counts = checked === ids.length ? counts : { ...breachCounts.counts, ...counts };
      prefs.set('breach_last', `${Date.now()}|${source}|${Object.keys(counts).length}|${checked}`);
    }
    this.problem = problem;
    this.running = false;
  }

  stop() {
    this.stopping = true;
  }
}

export const breachCheck = new BreachCheck();

$effect.root(() => {
  $effect(() => {
    if (pass.state.kind !== 'unlocked') breachCounts.counts = {};
  });
});

pass.onWipe.push(() => {
  breachCheck.stop();
  breachCounts.counts = {};
  prefs.set('breach_last', '');
});
