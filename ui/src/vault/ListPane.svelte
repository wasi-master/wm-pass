<script lang="ts">
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Tile from '../parts/Tile.svelte';
  import Btn from '../parts/Btn.svelte';
  import { tick, untrack } from 'svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { kindInSentence, kindName, kindPlural } from '../engine/face';
  import type { Summary } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { host } from '../engine/host';
  import { byName, chordLabel } from '../engine/text';
  import { ui } from './ui.svelte';
  import { TAB_INFO } from './tabs';
  import { entryMenu, runEntryMenu } from './entryMenu';

  type Item =
    | { type: 'heading'; text: string }
    | { type: 'one'; entry: Summary; inside?: boolean }
    | { type: 'stack'; site: string; entries: Summary[] };

  let openSites = $state<string[]>([]);
  let trashed = $state.raw<Summary[]>([]);
  let shelf = $state.raw<Summary[]>([]);
  let shelfOpen = $state(false);
  let searchInput = $state<HTMLInputElement | HTMLTextAreaElement>();
  let listBody = $state<HTMLElement>();
  let searchTimer: ReturnType<typeof setTimeout>;

  $effect(() => {
    void pass.revision;
    // A reply for a section since left is dropped: a slow trash list must not land on the shelf's.
    const section = ui.section;
    if (section === 'trash') {
      void pass.trashed().then(
        (list) => {
          if (ui.section === section) trashed = list;
        },
        () => undefined,
      );
    }
    if (section === 'shelf') {
      void pass
        .info()
        .then(async (info) => {
          const found = info.shelf_open ? (await pass.search('hidden')).filter((e) => e.hidden) : [];
          if (ui.section !== section) return;
          shelfOpen = info.shelf_open;
          shelf = found;
        })
        .catch(() => undefined);
    }
  });

  // Only a fresh ask for the search box takes focus, not the list coming back into view.
  let focusAsked = untrack(() => ui.searchFocus);
  $effect(() => {
    const asked = ui.searchFocus;
    if (asked !== focusAsked && searchInput) {
      focusAsked = asked;
      searchInput.focus();
      (searchInput as HTMLInputElement).select?.();
    }
  });

  function typed() {
    clearTimeout(searchTimer);
    const query = ui.query.trim();
    if (!query) {
      ui.searchResults = null;
      return;
    }
    searchTimer = setTimeout(() => void searchFor(query), 60);
  }

  /** What the results on show were asked for. */
  let resultsFor = '';
  async function searchFor(query: string) {
    const found = await pass.search(query, prefs.v.sort === 'changed' ? 'modified' : 'title').catch(() => null);
    // A slow reply for what was typed before is dropped, and so is one for a search since cleared.
    if (found && ui.query.trim() === query) {
      ui.searchResults = found;
      resultsFor = query;
    }
  }

  function clearSearch() {
    clearTimeout(searchTimer);
    ui.query = '';
    ui.searchResults = null;
    // The clear button goes with the text, and the focus would go with it.
    searchInput?.focus();
  }

  // Asked again after a change, and after a new sort, which the results come back in.
  $effect(() => {
    void pass.revision;
    void prefs.v.sort;
    if (ui.query.trim()) typed();
  });
  $effect(() => () => clearTimeout(searchTimer));

  const base = $derived.by(() => {
    const section = ui.section;
    const all = pass.entries;
    if (section === 'favourites') return all.filter((e) => e.favourite);
    if (section === 'trash') return trashed;
    if (section === 'shelf') return shelf;
    if (section.startsWith('kind:')) return all.filter((e) => e.kind === section.slice(5));
    if (section.startsWith('folder:')) return all.filter((e) => e.folder === section.slice(7));
    if (section.startsWith('tag:')) return all.filter((e) => e.tags.includes(section.slice(4)));
    return all;
  });

  // A search looks within the section shown. The trash and the shelf are left out of the vault's
  // own search, so their items are matched here.
  const searched = $derived.by((): Summary[] | null => {
    const section = ui.section;
    // Matched here as it is typed, so a search of the vault that fails or is still on its way
    // does not leave the whole trash on show as if it matched.
    if ((section === 'trash' || section === 'shelf') && ui.query.trim()) {
      const words = ui.query.trim().toLowerCase().split(/\s+/).filter(Boolean);
      return base.filter((e) => {
        const text = `${e.title} ${e.subtitle} ${e.site ?? ''}`.toLowerCase();
        return words.every((w) => text.includes(w));
      });
    }
    const results = ui.searchResults;
    if (!results) return null;
    if (section === 'all') return results;
    const here = new Set(base.map((e) => e.id));
    return results.filter((e) => here.has(e.id));
  });

  const heading = $derived.by(() => {
    const section = ui.section;
    if (section.startsWith('kind:')) return kindPlural(section.slice(5));
    if (section.startsWith('folder:')) return pass.folders.find((f) => f.id === section.slice(7))?.name ?? 'Folder';
    if (section.startsWith('tag:')) return section.slice(4);
    return TAB_INFO[section]?.label ?? '';
  });

  function sorted(list: Summary[]) {
    const byTitle = (a: Summary, b: Summary) => byName(a.title, b.title);
    switch (prefs.v.sort) {
      case 'changed':
        return [...list].sort((a, b) => b.modified_ms - a.modified_ms);
      case 'kind':
        return [...list].sort((a, b) => byName(kindName(a.kind), kindName(b.kind)) || byTitle(a, b));
      case 'site':
        // Items with no site last, whatever script the sites are written in.
        return [...list].sort((a, b) => Number(!a.site) - Number(!b.site) || byName(a.site ?? '', b.site ?? '') || byTitle(a, b));
      default:
        return list;
    }
  }

  // Sorted once for each change, rather than once for the grid and again for the arrow keys.
  const sortedBase = $derived(sorted(base));

  /** Pinned first, then the rest by name, with several logins on one site folded together. */
  const items = $derived.by((): Item[] => {
    const results = searched;
    if (results) return results.map((entry) => ({ type: 'one', entry }));
    const list = sortedBase;
    const out: Item[] = [];
    const headings = prefs.v.list_headings;
    const pinned = prefs.v.pinned_on_top ? list.filter((e) => e.pinned) : [];
    const rest = prefs.v.pinned_on_top ? list.filter((e) => !e.pinned) : list;
    if (pinned.length) {
      if (headings) out.push({ type: 'heading', text: 'Pinned' });
      pinned.forEach((entry) => out.push({ type: 'one', entry }));
    }
    if (!rest.length) return out;
    if (prefs.v.grouping === 'folder') {
      const names = new Map(pass.folders.map((f) => [f.id, f.name]));
      const groups = new Map<string, Summary[]>();
      for (const entry of rest) {
        const name = (entry.folder && names.get(entry.folder)) || 'No folder';
        // Added to in place: a copy of the group for each item would be ten thousand copies.
        const group = groups.get(name);
        if (group) group.push(entry);
        else groups.set(name, [entry]);
      }
      // By name as a person reads it, not capitals first, with what is in no folder at the end.
      const order = [...groups.keys()].sort((a, b) => Number(a === 'No folder') - Number(b === 'No folder') || byName(a, b));
      for (const name of order) {
        if (headings) out.push({ type: 'heading', text: name });
        groups.get(name)!.forEach((entry) => out.push({ type: 'one', entry }));
      }
      return out;
    }
    if (prefs.v.grouping === 'types') {
      // The phone opens on types, then folders; here the sidebar has both, so the list keeps
      // one part for each type.
      const groups = new Map<string, Summary[]>();
      for (const entry of rest) {
        const group = groups.get(entry.kind);
        if (group) group.push(entry);
        else groups.set(entry.kind, [entry]);
      }
      for (const kind of [...groups.keys()].sort((a, b) => byName(kindPlural(a), kindPlural(b)))) {
        if (headings) out.push({ type: 'heading', text: kindPlural(kind) });
        groups.get(kind)!.forEach((entry) => out.push({ type: 'one', entry }));
      }
      return out;
    }
    if (pinned.length && headings) out.push({ type: 'heading', text: 'All items' });
    const bySite = new Map<string, Summary[]>();
    if (prefs.v.group_sites) {
      for (const entry of rest) {
        if (entry.kind !== 'login' || !entry.site) continue;
        const group = bySite.get(entry.site);
        if (group) group.push(entry);
        else bySite.set(entry.site, [entry]);
      }
    }
    const done = new Set<string>();
    for (const entry of rest) {
      const group = entry.site ? bySite.get(entry.site) : undefined;
      if (group && group.length > 1 && entry.kind === 'login') {
        if (!done.has(entry.site!)) {
          done.add(entry.site!);
          out.push({ type: 'stack', site: entry.site!, entries: group });
          // Under their stack, set in, so they read as its accounts rather than more of the list.
          if (openSites.includes(entry.site!)) group.forEach((e) => out.push({ type: 'one', entry: e, inside: true }));
        }
      } else {
        out.push({ type: 'one', entry });
      }
    }
    return out;
  });

  // In the order on screen: the grid lays out every item as sorted, with nothing folded or pinned
  // ahead, so the arrows and Shift-click follow the grid there.
  const flatEntries = $derived(
    prefs.v.layout === 'grid' && !searched ? sortedBase : items.flatMap((item) => (item.type === 'one' ? [item.entry] : [])),
  );

  /**
   * Rows drawn so far. A long list draws a page at a time as it scrolls, rather than every one of
   * ten thousand items at once, and again for each letter typed in the search.
   */
  const PAGE = 200;
  let limit = $state(PAGE);
  $effect(() => {
    void ui.section;
    void ui.query;
    limit = PAGE;
  });
  /** Far enough to include the row chosen, wherever the arrows, End or a link took it. */
  function reach<T>(list: T[], idOf: (row: T) => string | null) {
    const selected = ui.selected;
    const at = selected ? list.findIndex((row) => idOf(row) === selected) : -1;
    return Math.max(limit, at + 1 + 20);
  }
  /**
   * What the arrow keys step through, in the order on screen: the items, and a site's stack of
   * logins, which Enter or the right arrow opens and the left arrow closes.
   */
  const navKeys = $derived(
    prefs.v.layout === 'grid' && !searched
      ? sortedBase.map((e) => e.id)
      : items.flatMap((item) => (item.type === 'one' ? [item.entry.id] : item.type === 'stack' ? [`s:${item.site}`] : [])),
  );
  /** The stack the arrows are on, when they are on one rather than on an item. */
  let cursorStack = $state<string | null>(null);
  $effect(() => {
    void ui.selected;
    void ui.section;
    cursorStack = null;
  });
  const stackId = (site: string) => `stack-${encodeURIComponent(site)}`;
  function toggleStack(site: string, open = !openSites.includes(site)) {
    if (open && !openSites.includes(site)) openSites = [...openSites, site];
    if (!open) openSites = openSites.filter((s) => s !== site);
  }

  /** Parts of the list under one heading, so a screen reader hears which part a row is in. */
  const parts = $derived.by(() => {
    const out: { heading: string | null; items: Item[] }[] = [];
    for (const item of itemsShown) {
      if (item.type === 'heading') out.push({ heading: item.text, items: [] });
      else if (out.length) out[out.length - 1].items.push(item);
      else out.push({ heading: null, items: [item] });
    }
    return out;
  });

  const gridShown = $derived(prefs.v.layout === 'grid' && !searched ? sortedBase.slice(0, reach(sortedBase, (e) => e.id)) : []);
  const itemsShown = $derived(items.slice(0, reach(items, (item) => (item.type === 'one' ? item.entry.id : null))));
  const moreToDraw = $derived(prefs.v.layout === 'grid' && !searched ? gridShown.length < sortedBase.length : itemsShown.length < items.length);

  /** Draws the next page when the end of what is drawn comes near. Made anew for each page, so a short page still asks for the next. */
  function more(node: HTMLElement) {
    const watcher = new IntersectionObserver(
      (seen) => {
        if (seen.some((s) => s.isIntersecting)) limit += PAGE;
      },
      { root: node.closest('.list-body'), rootMargin: '800px 0px' },
    );
    watcher.observe(node);
    return { destroy: () => watcher.disconnect() };
  }

  let anchor: string | null = null;
  /** The far end of a range chosen with Shift and the arrows, which the next one moves on from. */
  let rangeEnd = $state<string | null>(null);

  function clicked(event: MouseEvent, entry: Summary) {
    cursorStack = null;
    rangeEnd = null;
    const ids = flatEntries.map((e) => e.id);
    // From the row last clicked or moved to, as long as it is still in the list on screen.
    const start = anchor && ids.includes(anchor) ? anchor : ui.selected && ids.includes(ui.selected) ? ui.selected : null;
    if (event.shiftKey && start) {
      const from = ids.indexOf(start);
      const to = ids.indexOf(entry.id);
      if (from < 0 || to < 0) return;
      const range = ids.slice(Math.min(from, to), Math.max(from, to) + 1);
      // A range of one is simply that item, shown, as with the modifier key below.
      if (range.length === 1) ui.open(range[0]);
      else ui.multi = range;
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      // An item open from another section or before a search is not one the person can see here.
      const current = ui.multi.length ? ui.multi : ui.selected && ids.includes(ui.selected) ? [ui.selected] : [];
      const next = current.includes(entry.id) ? current.filter((id) => id !== entry.id) : [...current, entry.id];
      anchor = entry.id;
      // Down to one, it is simply the item shown; down to none, nothing is.
      if (next.length === 1) ui.open(next[0]);
      else if (next.length === 0) {
        ui.multi = [];
        ui.deselect(entry.id);
      } else ui.multi = next;
      return;
    }
    anchor = entry.id;
    void ui.guard(() => {
      ui.multi = [];
      const action = prefs.v.tap_action === 'open' ? null : ui.effective(prefs.v.tap_action, entry);
      if (action) void ui.perform(action, entry);
      ui.open(entry.id);
    });
  }

  // A set, so each row asks in one step: a range of thousands would otherwise be searched once per row.
  const chosen = $derived(new Set(ui.multi));
  function isSelected(entry: Summary) {
    return chosen.size ? chosen.has(entry.id) : ui.selected === entry.id;
  }

  async function menu(event: MouseEvent, entry: Summary) {
    event.preventDefault();
    // The event the menu key sends after its press, on a row clicked earlier: its menu is up already.
    if (Date.now() - keyMenuAt < 1000) return;
    const choice = await overlays.popup(entryMenu(entry), event.clientX, event.clientY);
    if (choice) await runEntryMenu(choice, entry);
  }

  // Shift+F10 or the menu key, on the list: the menu of the row the arrows reached, under it.
  let keyMenuAt = 0;
  async function keyMenu(entry: Summary) {
    keyMenuAt = Date.now();
    const box = document.getElementById(`option-${entry.id}`)?.getBoundingClientRect() ?? listBody?.getBoundingClientRect();
    const choice = await overlays.popup(entryMenu(entry), (box?.left ?? 0) + 16, box?.bottom ?? 0);
    if (choice) await runEntryMenu(choice, entry);
  }

  /** The browser's own menu that follows the key is not wanted over the row's. */
  function listContextMenu(event: MouseEvent) {
    if (event.target === listBody && Date.now() - keyMenuAt < 1000) event.preventDefault();
  }

  async function addMenu(event: MouseEvent) {
    const kinds = prefs.listOf('add_kinds');
    // With every kind taken off the menu in Settings, the button still adds a login, as the shortcut does.
    if (kinds.length <= 1) {
      ui.add(kinds[0] ?? 'login');
      return;
    }
    const target = event.currentTarget as HTMLElement;
    const box = target.getBoundingClientRect();
    const choice = await overlays.popup(kinds.map((k) => ({ id: k, label: kindName(k) })), box.left, box.bottom + 2);
    if (choice) void ui.guard(() => ui.add(choice));
  }

  async function sortMenu(event: MouseEvent) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const v = prefs.v;
    const choice = await overlays.popup(
      [
        { id: 'sort:name', label: 'Sort by name', checked: v.sort === 'name' },
        { id: 'sort:changed', label: 'Sort by last changed', checked: v.sort === 'changed' },
        { id: 'sort:kind', label: 'Sort by type', checked: v.sort === 'kind' },
        { id: 'sort:site', label: 'Sort by website', checked: v.sort === 'site' },
        { separator: true },
        { id: 'layout:list', label: 'List', checked: v.layout === 'list' },
        { id: 'layout:compact', label: 'Compact', checked: v.layout === 'compact' },
        { id: 'layout:grid', label: 'Grid', checked: v.layout === 'grid' },
        { separator: true },
        { id: 'group:smart', label: 'Group by site', checked: v.grouping === 'smart' },
        { id: 'group:folder', label: 'Group by folder', checked: v.grouping === 'folder' },
        { id: 'group:types', label: 'Group by type', checked: v.grouping === 'types' },
      ],
      box.left,
      box.bottom + 2,
    );
    if (!choice) return;
    const [kind, value] = choice.split(':');
    if (kind === 'sort') prefs.set('sort', value as never);
    if (kind === 'layout') prefs.set('layout', value as never);
    if (kind === 'group') prefs.set('grouping', value as never);
  }

  function dbl(entry: Summary) {
    const action = ui.effective(prefs.v.double_click, entry);
    if (action) void ui.perform(action, entry);
  }

  function keyed(event: KeyboardEvent) {
    // Keys that an input method is using to compose a word, such as Enter to accept it, are its own.
    if (event.isComposing || event.keyCode === 229) return;
    const ids = flatEntries.map((e) => e.id);
    const keys = navKeys;
    if (!keys.length) return;
    // Shift and an arrow choose a range from the row the arrows were on, as Shift-click does.
    if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && event.shiftKey && document.activeElement !== searchInput && ids.length) {
      event.preventDefault();
      const from = anchor && ids.includes(anchor) ? anchor : ui.selected && ids.includes(ui.selected) ? ui.selected : ids[0];
      const end = rangeEnd && ids.includes(rangeEnd) && ui.multi.includes(rangeEnd) ? rangeEnd : from;
      const next = ids[Math.max(0, Math.min(ids.length - 1, ids.indexOf(end) + (event.key === 'ArrowDown' ? 1 : -1)))];
      const a = ids.indexOf(from);
      const b = ids.indexOf(next);
      const range = ids.slice(Math.min(a, b), Math.max(a, b) + 1);
      cursorStack = null;
      anchor = from;
      if (range.length === 1) {
        rangeEnd = null;
        ui.open(range[0]);
      } else {
        void ui.guard(() => {
          ui.multi = range;
          rangeEnd = next;
        });
      }
      void showSelected();
      return;
    }
    const at = keys.indexOf(cursorStack !== null ? `s:${cursorStack}` : (ui.selected ?? ''));
    const step = (key: string | undefined) => {
      if (!key) return;
      rangeEnd = null;
      if (key.startsWith('s:')) {
        cursorStack = key.slice(2);
      } else {
        cursorStack = null;
        anchor = key;
        ui.open(key);
      }
      void showSelected();
    };
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      step(keys[Math.min(keys.length - 1, at + 1)]);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      step(keys[Math.max(0, at - 1)]);
    } else if ((event.key === 'Home' || event.key === 'End') && document.activeElement !== searchInput) {
      // In the search box these keys move through what is typed.
      event.preventDefault();
      step(event.key === 'Home' ? keys[0] : keys[keys.length - 1]);
    } else if (cursorStack !== null && (event.target === listBody || (event.target as HTMLElement).getAttribute?.('role') === 'option') && (event.key === 'ArrowRight' || event.key === 'ArrowLeft' || event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      toggleStack(cursorStack, event.key === 'ArrowRight' ? true : event.key === 'ArrowLeft' ? false : undefined);
    } else if (!ids.length) {
      return;
    } else if (event.key === 'Enter' && document.activeElement === searchInput && ui.section !== 'trash' && ui.section !== 'shelf' && ui.query.trim() && resultsFor !== ui.query.trim()) {
      // Enter straight after typing opens the first match for what was typed, not for the letters before it.
      event.preventDefault();
      clearTimeout(searchTimer);
      const query = ui.query.trim();
      void searchFor(query).then(() => {
        const first = flatEntries[0];
        if (first && ui.query.trim() === query) ui.open(first.id);
      });
    } else if (event.key === 'Enter' && document.activeElement === searchInput && ids.length) {
      event.preventDefault();
      ui.open(ids[at >= 0 ? at : 0]);
    } else if ((event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) && document.activeElement !== searchInput) {
      const entry = flatEntries.find((e) => e.id === ui.selected);
      if (!entry) return;
      event.preventDefault();
      void keyMenu(entry);
    } else if (event.key === 'Enter' && document.activeElement !== searchInput) {
      // On the list, Enter is a click on the row the arrows reached. A row clicked earlier keeps
      // the focus while the arrows move, so the key cannot go by which row has it; and a button
      // inside the list, such as a stack or a copy button, keeps its own Enter.
      const target = event.target as HTMLElement;
      if (target !== listBody && target.getAttribute('role') !== 'option') return;
      const entry = flatEntries.find((e) => e.id === ui.selected);
      if (!entry) return;
      event.preventDefault();
      clicked(event as unknown as MouseEvent, entry);
    }
  }

  // The row chosen with the arrow keys stays in sight in a long list.
  async function showSelected() {
    await tick();
    const id = activeOption;
    (id ? document.getElementById(id) : null)?.scrollIntoView({ block: 'nearest' });
  }

  async function emptyTrash() {
    const yes = await overlays.ask({
      title: 'Empty the trash?',
      body: trashed.length === 1 ? 'This item and its attachments will be deleted for good.' : `These ${trashed.length} items and their attachments will be deleted for good.`,
      action: trashed.length === 1 ? 'Delete it for good' : 'Delete them for good',
      danger: true,
    });
    if (!yes) return;
    const ids = trashed.map((e) => e.id);
    try {
      await ui.bulk('purge', {}, ids);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The trash could not be emptied.');
      return;
    }
    // Several chosen there are gone with the rest: Restore would act on nothing.
    if (ui.multi.some((id) => ids.includes(id))) ui.multi = [];
    if (ui.selected && ids.includes(ui.selected)) ui.deselect(ui.selected);
  }

  // The focus stays on the list while the arrows move, so a screen reader is told which row they reached.
  const activeOption = $derived(
    cursorStack !== null
      ? stackId(cursorStack)
      : rangeEnd && ui.multi.includes(rangeEnd)
        ? `option-${rangeEnd}`
        : ui.selected && flatEntries.some((e) => e.id === ui.selected)
          ? `option-${ui.selected}`
          : undefined,
  );

  // How many the search found, said once it settles rather than for every letter.
  let resultsSaid = $state('');
  $effect(() => {
    const query = ui.query.trim();
    const count = searched?.length ?? null;
    if (!query || count === null) {
      resultsSaid = '';
      return;
    }
    const timer = setTimeout(() => (resultsSaid = count === 0 ? `Nothing matches “${query}”` : count === 1 ? '1 match' : `${count} matches`), 400);
    return () => clearTimeout(timer);
  });

  const addKind = $derived(ui.section.startsWith('kind:') ? ui.section.slice(5) : 'login');

  const searchHint = $derived(`Search${ui.section === 'all' || !heading ? '' : ` ${heading}`}${prefs.shortcuts.search ? `  ${chordLabel(prefs.shortcuts.search, host().os)}` : ''}`);
</script>

{#snippet badges(entry: Summary)}
  {#if prefs.v.row_badges}
    <span class="badges">
      {#if entry.favourite}<span class="star" style="display:flex"><Icon name="star_fill" size={13} /><span class="sr-only">, favorite</span></span>{/if}
      {#if entry.has_otp}<Icon name="pin" size={13} /><span class="sr-only">, has a one-time code</span>{/if}
      {#if entry.has_passkey}<Icon name="key_vertical" size={13} /><span class="sr-only">, has a passkey</span>{/if}
      {#if entry.attachments > 0}<Icon name="attach_file" size={13} /><span class="sr-only">, has attachments</span>{/if}
      {#if entry.destroy_at_ms}<span class="timer" style="display:flex"><Icon name="timer" size={13} /><span class="sr-only">, deletes itself</span></span>{/if}
    </span>
  {/if}
{/snippet}

{#snippet row(entry: Summary, inside = false)}
  {@const compact = prefs.v.layout === 'compact'}
  {@const button = ui.effective(prefs.v.row_button, entry)}
  {@const sub = prefs.v.row_subtitle ? entry.subtitle : ''}
  <!-- Enter is the list's own, for the row the arrows reached. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="row"
    class:compact
    class:inside
    role="option"
    id="option-{entry.id}"
    tabindex="-1"
    aria-selected={isSelected(entry)}
    onclick={(e) => clicked(e, entry)}
    ondblclick={() => dbl(entry)}
    oncontextmenu={(e) => menu(e, entry)}
  >
    {#if prefs.v.row_icons}
      <Tile title={entry.title} site={entry.site} kind={entry.kind} label={entry.color} icon={entry.icon} size={compact ? 20 : 32} />
    {/if}
    <span class="grow">
      <!-- Compact keeps what each row says, on one line: the detail beside the name. -->
      <span class="gap" style="gap:4px">
        <span class="title ell" style="flex:0 1 auto">{entry.title || 'Untitled'}</span>
        {#if compact && sub}<span class="sub ell" style="flex:0 1 auto;padding-left:4px">{sub}</span>{/if}
        {@render badges(entry)}
      </span>
      {#if !compact && sub}<span class="sub ell" style="display:block">{sub}</span>{/if}
    </span>
    {#if button && !entry.trashed}
      <span class="hover-only">
        <IconBtn icon="content_copy" size={16} label="{button === 'password' ? 'Copy password' : button === 'username' ? 'Copy username' : button === 'code' ? 'Copy code' : 'Copy'} for {entry.title || 'Untitled'}" onclick={(e) => { e.stopPropagation(); void ui.perform(button, entry); }} />
      </span>
    {/if}
  </div>
{/snippet}

{#snippet rows(list: Item[])}
  {#each list as item (item.type === 'one' ? item.entry.id : item.type === 'stack' ? `s:${item.site}` : `h:${item.text}`)}
    {#if item.type === 'stack'}
      {@const open = openSites.includes(item.site)}
      {@const name = new Set(item.entries.map((e) => e.title)).size === 1 && item.entries[0].title ? item.entries[0].title : item.site}
      <!-- One of the list's options, reached with the arrows like the items: Enter or the right
           arrow opens it, the left arrow closes it. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row"
        class:compact={prefs.v.layout === 'compact'}
        class:cursor={cursorStack === item.site}
        role="option"
        id={stackId(item.site)}
        tabindex="-1"
        aria-selected="false"
        onclick={() => {
          cursorStack = item.site;
          toggleStack(item.site);
        }}
      >
        <Tile title={name} site={item.site} size={prefs.v.layout === 'compact' ? 20 : 32} />
        {#if prefs.v.layout === 'compact'}
          <span class="grow gap" style="gap:4px">
            <span class="title ell">{name}</span>
            <span class="sub" style="flex:none;padding-left:4px">{item.entries.length} accounts</span><span class="sr-only">, {open ? 'open' : 'closed'}</span>
          </span>
        {:else}
          <span class="grow">
            <span class="title ell" style="display:block">{name}</span>
            <span class="sub">{item.entries.length} accounts</span><span class="sr-only">, {open ? 'open' : 'closed'}</span>
          </span>
        {/if}
        <span class="dimmer" style="display:flex;transform:rotate({open ? 180 : 0}deg);transition:transform var(--t-med)"><Icon name="expand_more" size={18} /></span>
      </div>
    {:else if item.type === 'one'}
      {@render row(item.entry, item.inside)}
    {/if}
  {/each}
{/snippet}

<section class="list-pane" style={ui.narrow ? 'flex:1' : `width:${prefs.v.list_width}px`} aria-label="Items">
  <div class="list-head" data-tauri-drag-region>
    <div class="grow" data-help-anchor="list.search">
      <Field bind:value={ui.query} bind:ref={searchInput} placeholder={searchHint} label="Search" oninput={typed} onkeydown={keyed}>
        {#snippet lead()}<Icon name="search" size={15} />{/snippet}
        {#snippet trail()}
          {#if ui.query}
            <IconBtn icon="close" label="Clear search" size={14} onclick={clearSearch} />
          {/if}
        {/snippet}
      </Field>
    </div>
    <IconBtn icon="sort" label="Sort and layout" menu onclick={sortMenu} />
    <span data-help-anchor="list.add" style="display:flex"><IconBtn icon="add" label="Add an item" menu={prefs.listOf('add_kinds').length > 1} onclick={addMenu} /></span>
  </div>

  {#if ui.section === 'trash' && trashed.length > 0 && !ui.query}
    <div class="list-title"><span class="grow dim small">{trashed.length} in the trash</span><Btn label="Empty" variant="quiet" onclick={emptyTrash} /></div>
  {:else if ui.section === 'shelf' && shelfOpen && !ui.query}
    <div class="list-title"><span class="grow dim small">The shelf is open</span><Btn label="Close it" variant="quiet" onclick={() => void pass.change('shelf.close').catch((failure: any) => overlays.toast(failure?.message ?? 'The shelf could not be closed.'))} /></div>
  {/if}

  <!-- A list of items while there are some; with none, the words and buttons in their place are
       read and reached as they are, not as the options of an empty list. -->
  <div class="sr-only" role="status">{resultsSaid}</div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="list-body scroll"
    role={items.length || gridShown.length ? 'listbox' : 'region'}
    tabindex={items.length || gridShown.length ? 0 : -1}
    aria-label={heading || 'Items'}
    aria-multiselectable={items.length || gridShown.length ? true : undefined}
    aria-activedescendant={activeOption}
    onkeydown={keyed}
    oncontextmenu={listContextMenu}
    bind:this={listBody}
    data-help-anchor="list.rows"
  >
    {#if prefs.v.layout === 'grid' && !searched}
      <div class="grid">
        {#each gridShown as entry (entry.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="card"
            role="option"
            id="option-{entry.id}"
            tabindex="-1"
            aria-selected={isSelected(entry)}
            onclick={(e) => clicked(e, entry)}
            ondblclick={() => dbl(entry)}
            oncontextmenu={(e) => menu(e, entry)}
          >
            <Tile title={entry.title} site={entry.site} kind={entry.kind} label={entry.color} icon={entry.icon} size={40} />
            <span class="title ell">{entry.title || 'Untitled'}</span>
          </div>
        {/each}
      </div>
    {:else}
      <!-- A heading keys by its place too: a folder called Pinned has the same words as that part's. -->
      {#each parts as part, p (part.heading === null ? `p:${p}` : `h:${p}:${part.heading}`)}
        {#if part.heading !== null}
          <!-- A heading names its part for a screen reader as well as the eye. -->
          <div role="group" aria-labelledby="list-part-{p}">
            <div class="list-heading" id="list-part-{p}" role="presentation">{part.heading}</div>
            {@render rows(part.items)}
          </div>
        {:else}
          {@render rows(part.items)}
        {/if}
      {/each}
    {/if}

    {#if moreToDraw}
      {#key limit}<div use:more style="height:1px" aria-hidden="true"></div>{/key}
    {/if}

    {#if items.length === 0}
      {#if ui.query.trim()}
        <div class="empty">
          <p style="overflow-wrap:anywhere">Nothing matches “{ui.query.trim()}”.</p>
          <!-- Of the kind the list shows, so the new item is in the list it was searched from. -->
          {#if ui.section !== 'trash' && ui.section !== 'shelf'}<p style="padding-top:10px"><Btn label={addKind === 'login' ? 'Add it as a login' : `Add it as a new ${kindInSentence(addKind)}`} onclick={() => ui.add(addKind, { presetTitle: ui.query.trim() })} /></p>{/if}
        </div>
      {:else if ui.section === 'shelf' && !shelfOpen}
        <div class="empty">Open the shelf from the right to see what is on it.</div>
      {:else if ui.section === 'trash'}
        <div class="empty">Nothing in the trash.</div>
      {:else if ui.section === 'shelf'}
        <div class="empty">The shelf is empty.</div>
      {:else if ui.section === 'favourites'}
        <div class="empty">Star an item to find it here.</div>
      {:else if pass.entries.length === 0}
        <div class="empty">
          <p style="color:var(--text);font-weight:500">No items yet</p>
          <p style="padding:4px 0 12px">Import from another app, or add your first login.</p>
          <div class="stack" style="align-items:center">
            <Btn label="Add a login" variant="primary" onclick={() => ui.add('login')} />
            <Btn label="Import" onclick={() => ui.go('settings/import')} />
          </div>
        </div>
      {:else if ui.section.startsWith('folder:')}
        <div class="empty">This folder is empty. Move items here from their More menu, or choose several and Move to a folder.</div>
      {:else}
        <div class="empty">Nothing here.</div>
      {/if}
    {/if}
  </div>
</section>
