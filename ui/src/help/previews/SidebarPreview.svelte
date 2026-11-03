<script lang="ts">
  import Icon from '../../parts/Icon.svelte';
  import { prefs } from '../../engine/prefs.svelte';
  import { TAB_INFO } from '../../vault/tabs';

  // The sidebar with its parts as they are now, and the setting being explained as shown.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  function pick<T>(key: string, current: T): T {
    return (pref === key && value !== undefined ? value : current) as T;
  }
  const kinds = $derived(pick('sidebar_kinds', prefs.v.sidebar_kinds));
  const folders = $derived(pick('sidebar_folders', prefs.v.sidebar_folders));
  const tags = $derived(pick('sidebar_tags', prefs.v.sidebar_tags));
  const counts = $derived(pick('sidebar_counts', prefs.v.sidebar_counts));
  const open = $derived(pick('sidebar_open', prefs.v.sidebar_open));
  const lock = $derived(pick('show_lock', prefs.v.show_lock));
  const tabs = $derived(prefs.listOf('tabs').filter((t) => TAB_INFO[t]));
  const sample: Record<string, number> = { all: 42, favourites: 6, codes: 9 };
</script>

{#if open}
  <div class="mock-sidebar">
    {#each tabs as tab (tab)}
      <span class="side-item"><span class="ico"><Icon name={TAB_INFO[tab].icon} size={16} /></span><span class="grow ell">{TAB_INFO[tab].label}</span>{#if counts && sample[tab]}<span class="count">{sample[tab]}</span>{/if}</span>
    {/each}
    {#if kinds}
      <div class="side-title">Types</div>
      <span class="side-item"><span class="ico"><Icon name="key" size={16} /></span><span class="grow">Logins</span>{#if counts}<span class="count">31</span>{/if}</span>
      <span class="side-item"><span class="ico"><Icon name="credit_card" size={16} /></span><span class="grow">Cards</span>{#if counts}<span class="count">3</span>{/if}</span>
    {/if}
    {#if folders}
      <div class="side-title">Folders</div>
      <span class="side-item"><span class="ico"><Icon name="folder" size={16} /></span><span class="grow">Work</span>{#if counts}<span class="count">12</span>{/if}</span>
    {/if}
    {#if tags}
      <div class="side-title">Tags</div>
      <span class="side-item"><span class="ico"><Icon name="sell" size={16} /></span><span class="grow">family</span>{#if counts}<span class="count">4</span>{/if}</span>
    {/if}
    <div class="side-foot" style="margin-top:6px"><span class="name ell">Personal</span>{#if lock}<span class="icon-btn"><Icon name="lock" size={16} /></span>{/if}<span class="icon-btn"><Icon name="settings" size={16} /></span></div>
  </div>
{:else}
  <p class="dim">The sidebar is hidden. {prefs.shortcuts.toggle_sidebar ? 'Its shortcut, or the View menu, brings it back.' : 'The View menu brings it back.'}</p>
{/if}
