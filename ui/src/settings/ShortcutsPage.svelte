<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Group from '../parts/Group.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import { chordLabel, chordOf } from '../engine/text';

  const NAMES: [string, string][] = [
    ['search', 'Search'],
    ['add', 'New item'],
    ['edit', 'Edit item'],
    ['lock', 'Lock'],
    ['generator', 'Password generator'],
    ['codes', 'Codes'],
    ['health', 'Health'],
    ['settings', 'Settings'],
    ['copy_password', 'Copy password'],
    ['copy_username', 'Copy username'],
    ['copy_code', 'Copy code'],
    ['open_site', 'Open site'],
    ['toggle_sidebar', 'Show or hide the sidebar'],
    ['palette', 'Quick search'],
    ['help', 'What is this page?'],
  ];
  let editing = $state<string | null>(null);
  const os = host().os;

  function pressed(event: KeyboardEvent) {
    if (!editing) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      editing = null;
      return;
    }
    const chord = chordOf(event, os);
    if (!chord) return;
    prefs.setShortcut(editing, chord);
    editing = null;
  }

  async function reset() {
    const yes = await overlays.ask({ title: 'Back to the usual keys?', body: 'Every shortcut you changed goes back to how it started.', action: 'Reset' });
    if (yes) prefs.set('shortcuts_desktop', '');
  }
</script>

<svelte:window onkeydowncapture={pressed} />

<p class="dim" style="padding-bottom:14px">These work while this window is in front. Choose one to change it.</p>
<Group>
  {#each NAMES as [id, name] (id)}
    <button type="button" class="set-row" onclick={() => (editing = id)}>
      <span class="text">{name}</span>
      {#if prefs.shortcuts[id]}<span class="kbd">{chordLabel(prefs.shortcuts[id], os)}</span>{:else}<span class="dimmer small">None</span>{/if}
    </button>
  {/each}
</Group>
<div><Btn label="Back to the usual keys" icon="history" onclick={reset} /></div>

{#if editing}
  {@const id = editing}
  <Dialog title={NAMES.find((n) => n[0] === id)?.[1]} onclose={() => (editing = null)}>
    <p class="dim" style="padding:18px 0;text-align:center">Press the keys you want</p>
    {#snippet foot()}
      <Btn label="No shortcut" onclick={() => { prefs.setShortcut(id, ''); editing = null; }} />
      <Btn label="Cancel" onclick={() => (editing = null)} />
    {/snippet}
  </Dialog>
{/if}
