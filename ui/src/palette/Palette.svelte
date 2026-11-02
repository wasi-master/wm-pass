<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import Field from '../parts/Field.svelte';
  import Tile from '../parts/Tile.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { kindName } from '../engine/face';
  import type { Summary } from '../engine/types';
  import { applyTheme } from '../theme/themes';

  // A small search box over whatever you are doing. It holds nothing: it asks the session for what
  // the main window already has open, and a locked vault shows nothing at all.
  const desktop = host().desktop!;
  let query = $state('');
  let rows = $state.raw<Summary[]>([]);
  let chosen = $state(0);
  let locked = $state(true);
  let input = $state<HTMLInputElement | HTMLTextAreaElement>();

  async function refresh() {
    await prefs.load();
    pass.handle = await desktop.currentHandle();
    locked = pass.handle === 0;
    if (locked) {
      rows = [];
      return;
    }
    rows = (await pass.search(query.trim())).slice(0, 8);
    chosen = 0;
  }

  $effect(() => {
    const dark = prefs.v.look === 'SYSTEM' ? matchMedia('(prefers-color-scheme: dark)').matches : prefs.v.look !== 'LIGHT';
    applyTheme(prefs.theme, dark, prefs.v.look === 'OBSIDIAN');
    prefs.themeApplied++;
  });

  $effect(() => {
    const stop = listen('palette-opened', () => {
      query = '';
      void refresh();
      input?.focus();
    });
    void refresh();
    return () => void stop.then((run) => run());
  });

  async function copyPassword(entry: Summary) {
    await pass.copySecret('entry.reveal', { id: entry.id, field: 'password' }, prefs.v.clipboard_seconds);
    await desktop.hidePalette();
  }

  async function act(entry: Summary, event: KeyboardEvent | MouseEvent) {
    if (event.shiftKey && prefs.v.auto_type_enabled && entry.has_password) {
      await desktop.autoType(entry.id);
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      const detail = await pass.detail(entry.id);
      const name = detail.fields.find((f) => f.key === 'username')?.value;
      if (name) await host().copyText(String(name), false, 0);
      await desktop.hidePalette();
      return;
    }
    if (entry.has_password) await copyPassword(entry);
    else if (entry.has_otp) {
      await pass.copySecret('otp.code', { id: entry.id }, prefs.v.clipboard_seconds);
      await desktop.hidePalette();
    }
  }

  function keys(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      void desktop.hidePalette();
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      chosen = Math.min(chosen + 1, rows.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      chosen = Math.max(chosen - 1, 0);
    } else if (event.key === 'Enter' && rows[chosen]) {
      event.preventDefault();
      void act(rows[chosen], event);
    }
  }
</script>

<div class="palette" data-tauri-drag-region>
  {#if locked}
    <div class="empty" style="margin:auto">
      <p>WM Pass is locked.</p>
      <p style="padding-top:10px"><button type="button" class="btn primary" onclick={() => desktop.showMain()}>Open WM Pass</button></p>
    </div>
  {:else}
    <div class="head">
      <Field bind:value={query} bind:ref={input} placeholder="Search your vault" autofocus oninput={refresh} onkeydown={keys} />
    </div>
    <div class="list scroll">
      {#each rows as entry, index (entry.id)}
        <button type="button" class="row" aria-selected={index === chosen} onclick={(e) => act(entry, e)} onpointermove={() => (chosen = index)}>
          <Tile title={entry.title} site={entry.site} kind={entry.kind} label={entry.color} icon={entry.icon} size={28} />
          <span class="grow">
            <span class="title ell" style="display:block">{entry.title}</span>
            <span class="sub ell" style="display:block">{entry.subtitle || kindName(entry.kind)}</span>
          </span>
        </button>
      {/each}
      {#if rows.length === 0}<p class="empty">Nothing matches.</p>{/if}
    </div>
    <p class="dimmer small" style="padding:6px 12px;border-top:1px solid var(--hair)">
      Enter copies the password. {host().os === 'mac' ? '⌘' : 'Ctrl+'}Enter copies the username.{prefs.v.auto_type_enabled ? ' Shift+Enter types it.' : ''}
    </p>
  {/if}
</div>
