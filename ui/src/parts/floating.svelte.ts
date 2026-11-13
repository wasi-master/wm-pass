// Things that float over the window: questions, menus and passing messages.

import { host, type MenuItem } from '../engine/host';
import { stopped } from '../engine/stopped.svelte';

export interface Question {
  title: string;
  body?: string;
  action: string;
  danger?: boolean;
  cancel?: string;
  resolve: (yes: boolean) => void;
}

export interface Toast {
  id: number;
  text: string;
  action?: string;
  run?: () => void;
  /** Stays when the vault locks. */
  keep?: boolean;
}

class Overlays {
  question = $state<Question | null>(null);
  toasts = $state<Toast[]>([]);
  menu = $state<{ items: MenuItem[]; x: number; y: number; resolve: (id: string | null) => void } | null>(null);
  private next = 1;

  ask(options: Omit<Question, 'resolve'>): Promise<boolean> {
    // Only one question shows at a time. One already up is put down as a no, or whoever asked it
    // would wait for an answer that never comes.
    this.question?.resolve(false);
    // After an internal error nothing more can be done in the vault: the message over the screen
    // is the only question.
    if (stopped.on) return Promise.resolve(false);
    return new Promise((resolve) => {
      this.question = { ...options, resolve };
    });
  }

  answer(yes: boolean) {
    this.question?.resolve(yes);
    this.question = null;
  }

  /** Each message's clock, so one being read or reached for can wait. */
  private clocks = new Map<number, { timer: ReturnType<typeof setTimeout> | null; ms: number }>();

  toast(text: string, options: { action?: string; run?: () => void; ms?: number; keep?: boolean } = {}) {
    const id = this.next++;
    // Every call fails while the message about an internal error is up, and each would say so.
    if (stopped.on) return id;
    this.toasts = [...this.toasts.slice(-2), { id, text, action: options.action, run: options.run, keep: options.keep }];
    const ms = options.ms ?? (options.action ? 6000 : 2600);
    this.clocks.set(id, { timer: setTimeout(() => this.dismiss(id), ms), ms });
    return id;
  }

  dismiss(id: number) {
    const clock = this.clocks.get(id);
    if (clock?.timer) clearTimeout(clock.timer);
    this.clocks.delete(id);
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /**
   * Holds a message while the pointer is on it or the keyboard is in it: an Undo reached with Tab,
   * or a long message read slowly, must not go from under the person. Let go, it has its full time
   * again.
   */
  hold(id: number, held: boolean) {
    const clock = this.clocks.get(id);
    if (!clock) return;
    if (clock.timer) clearTimeout(clock.timer);
    clock.timer = held ? null : setTimeout(() => this.dismiss(id), clock.ms);
  }

  /**
   * Takes away the messages whose button acts on the vault, such as Undo, when it locks: pressed
   * at the door, they would act on nothing.
   */
  forgetVault() {
    for (const t of this.toasts) if (t.action && !t.keep) this.dismiss(t.id);
  }

  /** A menu at a point: the operating system's own if there is one. */
  async popup(items: MenuItem[], x: number, y: number): Promise<string | null> {
    const native = host().menu;
    if (native) return native(items);
    this.menu?.resolve(null);
    // The menu takes the focus to its first choice. Closed, it hands it back to where it was, or
    // someone on the keyboard is left at the top of the page.
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return new Promise((resolve) => {
      this.menu = {
        items,
        x,
        y,
        resolve: (id) => {
          if (before?.isConnected) before.focus({ preventScroll: true });
          resolve(id);
        },
      };
    });
  }

  closeMenu(id: string | null) {
    this.menu?.resolve(id);
    this.menu = null;
  }
}

export const overlays = new Overlays();
