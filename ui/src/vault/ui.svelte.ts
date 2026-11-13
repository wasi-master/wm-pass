// What the window is showing, and what a click on any of it does.

import { host } from '../engine/host';
import { consumeText, pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
import { secondsText, siteLine } from '../engine/text';
import type { Summary } from '../engine/types';
import { overlays } from '../parts/floating.svelte';

export interface Editing {
  id: string | null;
  kind: string;
  presetPassword?: string;
  presetNote?: string;
  presetOtp?: string;
  presetTitle?: string;
  /** A new item made while a folder or a tag is on show starts in it, so it is in the list it was added from. */
  presetFolder?: string;
  presetTags?: string[];
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
  /** A tag being renamed, deleted or given a symbol on every item. */
  managingTag = $state<string | null>(null);
  /** The last tag renamed or deleted, for a form open with it to follow: `to` is null when deleted. */
  tagChanged = $state<{ from: string; to: string | null } | null>(null);
  /** Asked to stop by anything that would lose an edit. */
  editorDirty = false;

  get narrow() {
    return this.width < 720;
  }

  /**
   * Forgets what the vault that just locked had on show. The next vault to open, which may be
   * another one, would otherwise find the old search with its results, an item chosen, or the
   * editor open on a form whose edits went with the lock.
   */
  reset() {
    this.selected = null;
    this.multi = [];
    this.editing = null;
    this.editorDirty = false;
    this.query = '';
    this.searchResults = null;
    this.showDetail = false;
    this.renamingFolder = null;
    this.managingTag = null;
    this.tagChanged = null;
    // A folder or a tag belongs to the vault that had it.
    if (/^(folder|tag):/.test(this.section)) this.section = 'all';
  }

  /** True when an edit is in the way: the person is asked first, and `then` runs if they let it go. */
  private blocked(then: () => void) {
    if (!this.editing || !this.editorDirty) return false;
    void this.guard(then);
    return true;
  }

  go(section: string) {
    if (this.section === section) return;
    if (this.blocked(() => this.go(section))) return;
    this.section = section;
    this.multi = [];
    this.editing = null;
    this.showDetail = false;
    if (!section.startsWith('settings')) prefs.set('last_tab', section);
  }

  open(id: string) {
    if (this.blocked(() => this.open(id))) return;
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
    if (this.blocked(() => this.add(kind, preset))) return;
    if (!this.isListSection(this.section)) this.section = 'all';
    const place: Partial<Editing> = this.section.startsWith('folder:')
      ? { presetFolder: this.section.slice(7) }
      : this.section.startsWith('tag:')
        ? { presetTags: [this.section.slice(4)] }
        : {};
    this.selected = null;
    this.multi = [];
    this.editing = { id: null, kind, ...place, ...preset };
    this.showDetail = true;
  }

  edit(id: string, kind: string) {
    if (this.editing?.id === id) return;
    if (this.blocked(() => this.edit(id, kind))) return;
    this.selected = id;
    // The editor is for the one item; the choice of several would come back when it closed.
    this.multi = [];
    this.editing = { id, kind };
    this.showDetail = true;
  }

  async guard(run: () => void) {
    if (this.editing && this.editorDirty) {
      const yes = await overlays.ask({ title: 'Discard changes?', body: 'Your edits to this item will be lost.', action: 'Discard', danger: true });
      if (!yes) return;
      this.editorDirty = false;
      // Let go of the edit itself too: run() may be a step to where the person already is, which
      // changes nothing and would leave the editor open with the edits it was told to drop.
      this.editing = null;
    }
    run();
  }

  // ---- acting on an entry ----

  private get clearAfter() {
    return prefs.v.clipboard_seconds;
  }

  private copied(what: string) {
    const seconds = this.clearAfter;
    overlays.toast(seconds > 0 ? `${what} copied. Clears in ${secondsText(seconds)}.` : `${what} copied.`, {
      action: 'Clear now',
      run: () => void host().clearClipboard(),
      ms: 3200,
      // Clearing the clipboard needs no vault, so the button still works after a lock.
      keep: true,
    });
  }

  async copySecret(what: string, method: string, args: unknown) {
    try {
      await pass.copySecret(method, args, this.clearAfter);
      this.copied(what);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be copied.');
    }
  }

  async copyPlain(what: string, value: string) {
    try {
      await host().copyText(value, false, 0);
      overlays.toast(`${what} copied.`);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be copied.');
    }
  }

  /** A code already on screen, such as a counter-based one: what is shown is what is copied. */
  async copyShownCode(code: string) {
    try {
      await host().copyText(code, true, this.clearAfter);
      this.copied('Code');
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be copied.');
    }
  }

  copyPassword(id: string) {
    return this.copySecret('Password', 'entry.reveal', { id, field: 'password' });
  }

  async copyCode(id: string) {
    // A counter-based code is used up once given out: each copy moves the counter on, as the
    // item page's New code button does, or the second copy would be refused by the site.
    const detail = await pass.detail(id).catch(() => null);
    if (detail?.otp && detail.otp.period === null) {
      try {
        const reply = await pass.change('otp.hotp_next', { id });
        return this.copyShownCode(consumeText(reply.secret));
      } catch (failure: any) {
        overlays.toast(failure?.message ?? 'That could not be copied.');
        return;
      }
    }
    return this.copySecret('Code', 'otp.code', { id });
  }

  async copyUsername(id: string) {
    const detail = await pass.detail(id).catch((failure: any) => {
      overlays.toast(failure?.message ?? 'That could not be copied.');
      return null;
    });
    if (!detail) return;
    const field = detail.fields.find((f) => f.key === 'username' || f.key === 'user');
    if (field?.value) await this.copyPlain('Username', String(field.value));
    else overlays.toast('This item has no username.');
  }

  async openSite(id: string) {
    try {
      const detail = await pass.detail(id);
      for (const site of detail.sites) {
        const line = siteLine(site);
        if (line?.openable) {
          const url = line.openable.includes('://') ? line.openable : `https://${line.openable}`;
          await host().openUrl(url);
          return;
        }
      }
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That site could not be opened.');
      return;
    }
    overlays.toast('This item has no website.');
  }

  canDo(action: string, entry: Summary) {
    // An item in the trash is only looked at, from its page: a double-click set to Edit would
    // otherwise open the editor on it, which the page itself does not offer.
    if (entry.trashed && action !== 'open') return false;
    switch (action) {
      case 'password':
        return entry.has_password;
      case 'code':
        return entry.has_otp;
      case 'username':
        // A login's line under its title is its username, so one with none offers no copy of it.
        return (entry.kind === 'login' && entry.subtitle !== '') || entry.kind === 'database';
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
        return pass.change('entry.common', { id: entry.id, favourite: !entry.favourite }).catch((failure: any) => {
          overlays.toast(failure?.message ?? 'That could not be changed.');
        });
      case 'trash':
        return this.trash(entry.id, entry.title);
      default:
        return undefined;
    }
  }

  /** Lets go of an item that has gone from the list. A narrow window goes back to the list too. */
  deselect(id: string) {
    if (this.selected !== id) return;
    this.selected = null;
    if (!this.editing) this.showDetail = false;
  }

  async trash(id: string, title: string) {
    try {
      await pass.change('entry.trash', { id });
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be moved to the trash.');
      return;
    }
    this.deselect(id);
    overlays.toast(`“${title || 'Untitled'}” moved to the trash.`, {
      action: 'Undo',
      run: () => void pass.change('entry.restore', { id }).catch((failure: any) => overlays.toast(failure?.message ?? 'It could not be restored.')),
    });
  }

  async bulk(op: string, args: Record<string, unknown> = {}, ids: string[] = this.multi) {
    await pass.change('entries.bulk', { ids, op, ...args });
  }
}

export const ui = new Ui();
