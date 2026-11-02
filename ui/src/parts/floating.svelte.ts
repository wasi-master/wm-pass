// Things that float over the window: questions, menus and passing messages.

import { host, type MenuItem } from '../engine/host';

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
}

class Overlays {
  question = $state<Question | null>(null);
  toasts = $state<Toast[]>([]);
  menu = $state<{ items: MenuItem[]; x: number; y: number; resolve: (id: string | null) => void } | null>(null);
  private next = 1;

  ask(options: Omit<Question, 'resolve'>): Promise<boolean> {
    return new Promise((resolve) => {
      this.question = { ...options, resolve };
    });
  }

  answer(yes: boolean) {
    this.question?.resolve(yes);
    this.question = null;
  }

  toast(text: string, options: { action?: string; run?: () => void; ms?: number } = {}) {
    const id = this.next++;
    this.toasts = [...this.toasts.slice(-2), { id, text, action: options.action, run: options.run }];
    setTimeout(() => this.dismiss(id), options.ms ?? (options.action ? 6000 : 2600));
    return id;
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /** A menu at a point: the operating system's own if there is one. */
  async popup(items: MenuItem[], x: number, y: number): Promise<string | null> {
    const native = host().menu;
    if (native) return native(items);
    return new Promise((resolve) => {
      this.menu = { items, x, y, resolve };
    });
  }

  closeMenu(id: string | null) {
    this.menu?.resolve(id);
    this.menu = null;
  }
}

export const overlays = new Overlays();
