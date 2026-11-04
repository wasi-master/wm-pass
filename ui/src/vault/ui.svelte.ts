// What the window is showing, and what a click on any of it does.

import { host } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
import { siteLine } from '../engine/text';
import type { Summary } from '../engine/types';
import { overlays } from '../parts/floating.svelte';

export interface Editing {
  id: string | null;
  kind: string;
  presetPassword?: string;
  presetNote?: string;
  presetOtp?: string;
  presetTitle?: string;
}

class Ui {
  /** all, favourites, trash, shelf, codes, generator, health, settings, kind:x, folder:x or tag:x. */
  section = $state('all');
  selected = $state<string | null>(null);
  /** More than one row chosen with the keyboard held. */
  multi = $state<string[]>([]);
  editing = $state<Editing | null>(null);
  query = $state('');
  searchResults = $state.raw<Summary[] | null>(null);
  width = $state(1000);
  showDetail = $state(false);
  searchFocus = $state(0);
  renamingFolder = $state<{ id: string; name: string } | null>(null);
  /** Asked to stop by anything that would lose an edit. */
  editorDirty = false;

  get narrow() {
    return this.width < 720;
  }

  go(section: string) {
    if (this.section === section) return;
    this.section = section;
    this.multi = [];
    this.editing = null;
    this.showDetail = false;
    if (!section.startsWith('settings')) prefs.set('last_tab', section);
  }

  open(id: string) {
    if (this.section !== 'all' && !this.isListSection(this.section)) this.section = 'all';
    this.selected = id;
    this.multi = [];
    this.editing = null;
    this.showDetail = true;
  }

  isListSection(section: string) {
    return ['all', 'favourites', 'trash', 'shelf'].includes(section) || /^(kind|folder|tag):/.test(section);
  }

  add(kind: string, preset: Partial<Editing> = {}) {
    if (!this.isListSection(this.section)) this.section = 'all';
    this.selected = null;
    this.multi = [];
    this.editing = { id: null, kind, ...preset };
    this.showDetail = true;
  }

  edit(id: string, kind: string) {
    this.selected = id;
    this.editing = { id, kind };
    this.showDetail = true;
  }

  async guard(run: () => void) {
    if (this.editing && this.editorDirty) {
      const yes = await overlays.ask({ title: 'Discard changes?', body: 'Your edits to this item will be lost.', action: 'Discard', danger: true });
      if (!yes) return;
      this.editorDirty = false;
    }
    run();
  }

  // ---- acting on an entry ----

  private get clearAfter() {
    return prefs.v.clipboard_seconds;
  }

  private copied(what: string) {
    const seconds = this.clearAfter;
    overlays.toast(seconds > 0 ? `${what} copied. Clears in ${seconds} seconds.` : `${what} copied.`, {
      action: 'Clear now',
      run: () => void host().clearClipboard(),
      ms: 3200,
    });
  }

  async copySecret(what: string, method: string, args: unknown) {
    try {
      await pass.copySecret(method, args, this.clearAfter);
      this.copied(what);
    } catch (failure: any) {
      overlays.toast(failure.message ?? 'That could not be copied.');
    }
  }

  async copyPlain(what: string, value: string) {
    await host().copyText(value, false, 0);
    overlays.toast(`${what} copied.`);
  }

  copyPassword(id: string) {
    return this.copySecret('Password', 'entry.reveal', { id, field: 'password' });
  }

  copyCode(id: string) {
    return this.copySecret('Code', 'otp.code', { id });
  }

  async copyUsername(id: string) {
    const detail = await pass.detail(id);
    const field = detail.fields.find((f) => f.key === 'username' || f.key === 'user');
    if (field?.value) await this.copyPlain('Username', String(field.value));
    else overlays.toast('This item has no username.');
  }

  async openSite(id: string) {
    const detail = await pass.detail(id);
    for (const site of detail.sites) {
      const line = siteLine(site);
      if (line?.openable) {
        const url = line.openable.includes('://') ? line.openable : `https://${line.openable}`;
        await host().openUrl(url);
        return;
      }
    }
    overlays.toast('This item has no website.');
  }

  canDo(action: string, entry: Summary) {
    switch (action) {
      case 'password':
        return entry.has_password;
      case 'code':
        return entry.has_otp;
      case 'username':
        return entry.kind === 'login' || entry.kind === 'database';
      case 'open_site':
        return !!entry.site;
      case 'none':
        return false;
      default:
        return true;
    }
  }

  /**
   * What a copy button or a click on a row does for this item: the action chosen, or for a copy
   * the item cannot do, the next that it can. A code with no password copies the code, and a
   * login with neither its username.
   */
  effective(action: string, entry: Summary): string | null {
    if (this.canDo(action, entry)) return action;
    if (action !== 'password' && action !== 'code' && action !== 'username') return null;
    return ['password', 'code', 'username'].find((a) => this.canDo(a, entry)) ?? null;
  }

  perform(action: string, entry: Summary) {
    switch (action) {
      case 'password':
        return this.copyPassword(entry.id);
      case 'username':
        return this.copyUsername(entry.id);
      case 'code':
        return this.copyCode(entry.id);
      case 'open_site':
        return this.openSite(entry.id);
      case 'edit':
        return this.edit(entry.id, entry.kind);
      case 'open':
        return this.open(entry.id);
      case 'favourite':
        return pass.change('entry.common', { id: entry.id, favourite: !entry.favourite });
      case 'trash':
        return this.trash(entry.id, entry.title);
      default:
        return undefined;
    }
  }

  async trash(id: string, title: string) {
    await pass.change('entry.trash', { id });
    if (this.selected === id) this.selected = null;
    overlays.toast(`“${title}” moved to the trash.`, {
      action: 'Undo',
      run: () => void pass.change('entry.restore', { id }),
    });
  }

  async bulk(op: string, args: Record<string, unknown> = {}, ids: string[] = this.multi) {
    await pass.change('entries.bulk', { ids, op, ...args });
  }
}

export const ui = new Ui();
