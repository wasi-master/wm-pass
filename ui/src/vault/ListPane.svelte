<script lang="ts">
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Tile from '../parts/Tile.svelte';
  import Btn from '../parts/Btn.svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { kindName, kindPlural } from '../engine/face';
  import type { Summary } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { host } from '../engine/host';
  import { chordLabel } from '../engine/text';
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
  let searchTimer: ReturnType<typeof setTimeout>;

  $effect(() => {
    void pass.revision;
    if (ui.section === 'trash') void pass.trashed().then((list) => (trashed = list));
    if (ui.section === 'shelf') {
      void pass.info().then(async (info) => {
        shelfOpen = info.shelf_open;
        shelf = info.shelf_open ? (await pass.search('hidden')).filter((e) => e.hidden) : [];
      });
    }
  });

  $effect(() => {
    if (ui.searchFocus && searchInput) {
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
    searchTimer = setTimeout(async () => {
      ui.searchResults = await pass.search(query, prefs.v.sort === 'changed' ? 'modified' : 'title');
    }, 60);
  }

  $effect(() => {
    void pass.revision;
    if (ui.query.trim()) typed();
  });

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

  const heading = $derived.by(() => {
    const section = ui.section;
    if (section.startsWith('kind:')) return kindPlural(section.slice(5));
    if (section.startsWith('folder:')) return pass.folders.find((f) => f.id === section.slice(7))?.name ?? 'Folder';
    if (section.startsWith('tag:')) return section.slice(4);
    return TAB_INFO[section]?.label ?? '';
  });

  function sorted(list: Summary[]) {
    const byTitle = (a: Summary, b: Summary) => a.title.toLowerCase().localeCompare(b.title.toLowerCase());
    switch (prefs.v.sort) {
      case 'changed':
        return [...list].sort((a, b) => b.modified_ms - a.modified_ms);
      case 'kind':
        return [...list].sort((a, b) => kindName(a.kind).localeCompare(kindName(b.kind)) || byTitle(a, b));
      case 'site':
        return [...list].sort((a, b) => (a.site ?? '~').localeCompare(b.site ?? '~') || byTitle(a, b));
      default:
        return list;
    }
  }

  /** Pinned first, then the rest by name, with several logins on one site folded together. */
  const items = $derived.by((): Item[] => {
    const results = ui.searchResults;
    if (results) return results.map((entry) => ({ type: 'one', entry }));
    const list = sorted(base);
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
        groups.set(name, [...(groups.get(name) ?? []), entry]);
      }
      for (const name of [...groups.keys()].sort()) {
        if (headings) out.push({ type: 'heading', text: name });
        groups.get(name)!.forEach((entry) => out.push({ type: 'one', entry }));
      }
      return out;
    }
    if (prefs.v.grouping === 'types') {
      // The phone opens on types, then folders; here the sidebar has both, so the list keeps
      // one part for each type.
      const groups = new Map<string, Summary[]>();
      for (const entry of rest) groups.set(entry.kind, [...(groups.get(entry.kind) ?? []), entry]);
      for (const kind of [...groups.keys()].sort((a, b) => kindPlural(a).localeCompare(kindPlural(b)))) {
        if (headings) out.push({ type: 'heading', text: kindPlural(kind) });
        groups.get(kind)!.forEach((entry) => out.push({ type: 'one', entry }));
      }
      return out;
    }
    if (pinned.length && headings) out.push({ type: 'heading', text: 'All items' });
    const bySite = new Map<string, Summary[]>();
    if (prefs.v.group_sites) {
      for (const entry of rest) {
        if (entry.kind === 'login' && entry.site) bySite.set(entry.site, [...(bySite.get(entry.site) ?? []), entry]);
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

  const flatEntries = $derived(items.flatMap((item) => (item.type === 'one' ? [item.entry] : [])));

  let anchor: string | null = null;

  function clicked(event: MouseEvent, entry: Summary) {
    if (event.shiftKey && anchor) {
      const ids = flatEntries.map((e) => e.id);
      const from = ids.indexOf(anchor);
      const to = ids.indexOf(entry.id);
      if (from >= 0 && to >= 0) ui.multi = ids.slice(Math.min(from, to), Math.max(from, to) + 1);
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      const current = ui.multi.length ? ui.multi : ui.selected ? [ui.selected] : [];
      ui.multi = current.includes(entry.id) ? current.filter((id) => id !== entry.id) : [...current, entry.id];
      anchor = entry.id;
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

  function isSelected(entry: Summary) {
    return ui.multi.length ? ui.multi.includes(entry.id) : ui.selected === entry.id;
  }

  async function menu(event: MouseEvent, entry: Summary) {
    event.preventDefault();
    const choice = await overlays.popup(entryMenu(entry), event.clientX, event.clientY);
    if (choice) await runEntryMenu(choice, entry);
  }

  async function addMenu(event: MouseEvent) {
    const kinds = prefs.listOf('add_kinds');
    if (kinds.length === 1) {
      ui.add(kinds[0]);
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
    const ids = flatEntries.map((e) => e.id);
    if (!ids.length) return;
    const at = ui.selected ? ids.indexOf(ui.selected) : -1;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      ui.open(ids[Math.min(ids.length - 1, at + 1)]);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      ui.open(ids[Math.max(0, at - 1)]);
    } else if (event.key === 'Enter' && document.activeElement === searchInput && ids.length) {
      event.preventDefault();
      ui.open(ids[at >= 0 ? at : 0]);
    }
  }

  async function emptyTrash() {
    const yes = await overlays.ask({
      title: 'Empty the trash?',
      body: `These ${trashed.length} items and their attachments will be deleted for good.`,
      action: 'Delete them for good',
      danger: true,
    });
    if (yes) await ui.bulk('purge', {}, trashed.map((e) => e.id));
  }

  const searchHint = $derived(`Search${prefs.shortcuts.search ? `  ${chordLabel(prefs.shortcuts.search, host().os)}` : ''}`);
</script>

{#snippet badges(entry: Summary)}
  {#if prefs.v.row_badges}
    <span class="badges">
      {#if entry.favourite}<span style="color:var(--warn);display:flex"><Icon name="star_fill" size={13} /></span>{/if}
      {#if entry.has_otp}<Icon name="pin" size={13} />{/if}
      {#if entry.has_passkey}<Icon name="key_vertical" size={13} />{/if}
      {#if entry.attachments > 0}<Icon name="attach_file" size={13} />{/if}
      {#if entry.destroy_at_ms}<span style="color:var(--danger);display:flex"><Icon name="timer" size={13} /></span>{/if}
    </span>
  {/if}
{/snippet}

{#snippet row(entry: Summary, inside = false)}
  {@const compact = prefs.v.layout === 'compact'}
  {@const button = ui.effective(prefs.v.row_button, entry)}
  {@const sub = prefs.v.row_subtitle ? entry.subtitle : ''}
  <div
    class="row"
    class:compact
    class:inside
    role="option"
    tabindex="-1"
    aria-selected={isSelected(entry)}
    onclick={(e) => clicked(e, entry)}
    ondblclick={() => dbl(entry)}
    oncontextmenu={(e) => menu(e, entry)}
    onkeydown={(e) => e.key === 'Enter' && clicked(e as unknown as MouseEvent, entry)}
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
        <IconBtn icon="content_copy" size={16} label="Copy for {entry.title}" onclick={(e) => { e.stopPropagation(); void ui.perform(button, entry); }} />
      </span>
    {/if}
  </div>
{/snippet}

<section class="list-pane" style={ui.narrow ? 'flex:1' : `width:${prefs.v.list_width}px`} aria-label="Items">
  <div class="list-head" data-tauri-drag-region>
    <div class="grow" data-help-anchor="list.search">
      <Field bind:value={ui.query} bind:ref={searchInput} placeholder={searchHint} label="Search" oninput={typed} onkeydown={keyed}>
        {#snippet lead()}<Icon name="search" size={15} />{/snippet}
        {#snippet trail()}
          {#if ui.query}
            <IconBtn icon="close" label="Clear search" size={14} onclick={() => { ui.query = ''; ui.searchResults = null; }} />
          {/if}
        {/snippet}
      </Field>
    </div>
    <IconBtn icon="sort" label="Sort and layout" onclick={sortMenu} />
    <span data-help-anchor="list.add" style="display:flex"><IconBtn icon="add" label="Add an item" onclick={addMenu} /></span>
  </div>

  {#if ui.section === 'trash' && trashed.length > 0 && !ui.query}
    <div class="list-title"><span class="grow dim small">{trashed.length} in the trash</span><Btn label="Empty" variant="quiet" onclick={emptyTrash} /></div>
  {:else if ui.section === 'shelf' && shelfOpen && !ui.query}
    <div class="list-title"><span class="grow dim small">The shelf is open</span><Btn label="Close it" variant="quiet" onclick={() => void pass.change('shelf.close')} /></div>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="list-body scroll" role="listbox" tabindex="0" aria-label={heading} onkeydown={keyed} data-help-anchor="list.rows">
    {#if prefs.v.layout === 'grid' && !ui.searchResults}
      <div class="grid">
        {#each sorted(base) as entry (entry.id)}
          <div
            class="card"
            role="option"
            tabindex="-1"
            aria-selected={isSelected(entry)}
            onclick={(e) => clicked(e, entry)}
            ondblclick={() => dbl(entry)}
            oncontextmenu={(e) => menu(e, entry)}
            onkeydown={(e) => e.key === 'Enter' && clicked(e as unknown as MouseEvent, entry)}
          >
            <Tile title={entry.title} site={entry.site} kind={entry.kind} label={entry.color} icon={entry.icon} size={40} />
            <span class="title ell">{entry.title || 'Untitled'}</span>
          </div>
        {/each}
      </div>
    {:else}
      {#each items as item (item.type === 'one' ? item.entry.id : item.type === 'stack' ? `s:${item.site}` : `h:${item.text}`)}
        {#if item.type === 'heading'}
          <div class="list-heading">{item.text}</div>
        {:else if item.type === 'stack'}
          {@const open = openSites.includes(item.site)}
          {@const name = new Set(item.entries.map((e) => e.title)).size === 1 && item.entries[0].title ? item.entries[0].title : item.site}
          <button
            type="button"
            class="row"
            class:compact={prefs.v.layout === 'compact'}
            aria-expanded={open}
            onclick={() => (openSites = open ? openSites.filter((s) => s !== item.site) : [...openSites, item.site])}
          >
            <Tile title={name} site={item.site} size={prefs.v.layout === 'compact' ? 20 : 32} />
            {#if prefs.v.layout === 'compact'}
              <span class="grow gap" style="gap:4px">
                <span class="title ell">{name}</span>
                <span class="sub" style="flex:none;padding-left:4px">{item.entries.length} accounts</span>
              </span>
            {:else}
              <span class="grow">
                <span class="title ell" style="display:block">{name}</span>
                <span class="sub">{item.entries.length} accounts</span>
              </span>
            {/if}
            <span class="dimmer" style="display:flex;transform:rotate({open ? 180 : 0}deg);transition:transform var(--t-med)"><Icon name="expand_more" size={18} /></span>
          </button>
        {:else}
          {@render row(item.entry, item.inside)}
        {/if}
      {/each}
    {/if}

    {#if items.length === 0}
      {#if ui.query.trim()}
        <div class="empty">
          <p>Nothing matches “{ui.query.trim()}”.</p>
          <p style="padding-top:10px"><Btn label="Add it as a login" onclick={() => ui.add('login', { presetTitle: ui.query.trim() })} /></p>
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
      {:else}
        <div class="empty">Nothing here.</div>
      {/if}
    {/if}
  </div>
</section>
