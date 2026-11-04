<script lang="ts">
  import HelpButton from '../help/HelpButton.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { kindIcon, kindPlural } from '../engine/face';
  import type { IconName } from '../engine/icons.svelte';
  import { ui } from './ui.svelte';
  import { TAB_INFO } from './tabs';
  import { overlays } from '../parts/floating.svelte';
  import { tags as tagLook } from '../engine/tags.svelte';

  const entries = $derived(pass.entries);
  const kinds = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const e of entries) counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  });
  const tags = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const e of entries) for (const t of e.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  });
  const name = $derived(pass.state.kind === 'unlocked' ? pass.state.name : '');
  const tabs = $derived(prefs.listOf('tabs'));

  function count(tab: string) {
    if (tab === 'all') return entries.length;
    if (tab === 'favourites') return entries.filter((e) => e.favourite).length;
    if (tab === 'codes') return entries.filter((e) => e.has_otp).length;
    return null;
  }

  function folderCount(id: string) {
    return entries.filter((e) => e.folder === id).length;
  }

  function go(section: string) {
    void ui.guard(() => ui.go(section));
  }

  async function folderMenu(event: MouseEvent, id: string, current: string) {
    event.preventDefault();
    const choice = await overlays.popup(
      [
        { id: 'rename', label: 'Rename…' },
        { id: 'remove', label: 'Remove folder', danger: true },
      ],
      event.clientX,
      event.clientY,
    );
    if (choice === 'rename') ui.renamingFolder = { id, name: current };
    if (choice === 'remove') {
      const yes = await overlays.ask({ title: `Remove “${current}”?`, body: 'What is in it stays in the vault, in no folder.', action: 'Remove', danger: true });
      if (yes) await pass.change('folder.remove', { id });
    }
  }
</script>

{#snippet item(section: string, icon: IconName, label: string, shown: number | null = null)}
  <button type="button" class="side-item" aria-current={ui.section === section} onclick={() => go(section)}>
    <span class="ico"><Icon name={icon} size={18} /></span>
    <span class="grow ell">{label}</span>
    {#if prefs.v.sidebar_counts && shown !== null}<span class="count">{shown}</span>{/if}
  </button>
{/snippet}

<aside class="sidebar" style="width:{prefs.v.sidebar_width}px" aria-label="Sections">
  <div class="top" data-tauri-drag-region></div>
  <nav class="items scroll">
    <div data-help-anchor="sidebar.tabs">
      {#each tabs as tab}
        {#if TAB_INFO[tab]}
          {@render item(tab, TAB_INFO[tab].icon, TAB_INFO[tab].label, count(tab))}
        {/if}
      {/each}
    </div>

    {#if prefs.v.sidebar_kinds && kinds.length > 0}
      <div class="side-title">Types</div>
      {#each kinds as [kind, shown]}
        {@render item(`kind:${kind}`, kindIcon(kind), kindPlural(kind), shown)}
      {/each}
    {/if}

    {#if prefs.v.sidebar_folders}
      <div class="side-title" data-help-anchor="sidebar.folders">
        <span>Folders</span>
        <IconBtn icon="add" label="New folder" size={15} class="tiny" onclick={() => (ui.renamingFolder = { id: '', name: '' })} />
      </div>
      {#each pass.folders as folder}
        <div oncontextmenu={(e) => folderMenu(e, folder.id, folder.name)} role="presentation">
          {@render item(`folder:${folder.id}`, 'folder', folder.name, folderCount(folder.id))}
        </div>
      {/each}
    {/if}

    {#if prefs.v.sidebar_tags && tags.length > 0}
      <div class="side-title">Tags</div>
      {#each tags as [tag, shown]}
        <div oncontextmenu={(e) => { e.preventDefault(); ui.managingTag = tag; }} role="presentation">
          {@render item(`tag:${tag}`, tagLook.glyph(tag), tag, shown)}
        </div>
      {/each}
    {/if}
  </nav>
  <div class="side-foot">
    <span class="name ell" title={name}>{pass.child ?? name}</span>
    <HelpButton id="screen.window" title="this window" />
    {#if prefs.v.show_lock}<span data-help-anchor="sidebar.lock" style="display:flex"><IconBtn icon="lock" label="Lock" onclick={() => void pass.lock()} /></span>{/if}
    <span data-help-anchor="sidebar.settings" style="display:flex"><IconBtn icon="settings" label="Settings" on={ui.section.startsWith('settings')} onclick={() => go('settings')} /></span>
  </div>
</aside>
