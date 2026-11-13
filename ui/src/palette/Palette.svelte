<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import Field from '../parts/Field.svelte';
  import Tile from '../parts/Tile.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { stopped } from '../engine/stopped.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { kindName } from '../engine/face';
  import type { Summary } from '../engine/types';
  import { applyTheme } from '../theme/themes';
  import { untrack } from 'svelte';

  // A small search box over whatever you are doing. It holds nothing: it asks the session for what
  // the main window already has open, and a locked vault shows nothing at all.
  const desktop = host().desktop!;
  let query = $state('');
  let rows = $state.raw<Summary[]>([]);
  let chosen = $state(0);
  let locked = $state(true);
  let input = $state<HTMLInputElement | HTMLTextAreaElement>();
  // Why the last copy did not happen, shown until the next key or the next try.
  let failed = $state('');

  // Each key typed asks again, and an answer to an older query must not land after a newer one.
  let asked = 0;
  /** From a key or the box opening, where nothing waits on the answer: a search that fails empties the list. */
  function refreshNow(opening = false) {
    void refresh(opening).catch(() => {
      rows = [];
    });
  }
  // A key typed waits a moment for the next, so a word typed fast is one search of the vault, not one per letter.
  let typing: ReturnType<typeof setTimeout> | undefined;
  let waiting = false;
  function typed() {
    clearTimeout(typing);
    waiting = true;
    typing = setTimeout(() => {
      waiting = false;
      refreshNow();
    }, 60);
  }
  $effect(() => () => clearTimeout(typing));
  /** `opening`: the settings are read again, as the main window may have changed them while the box was away. */
  async function refresh(opening = false) {
    const turn = ++asked;
    failed = '';
    if (opening) await prefs.load();
    pass.handle = await desktop.currentHandle();
    if (turn !== asked) return;
    locked = pass.handle === 0;
    if (locked) {
      rows = [];
      return;
    }
    const found = (await pass.search(query.trim())).slice(0, 8);
    if (turn !== asked) return;
    rows = found;
    chosen = 0;
  }

  // The system's light or dark, followed while the box is open as the main window follows it.
  const media = matchMedia('(prefers-color-scheme: dark)');
  let systemDark = $state(media.matches);
  $effect(() => {
    const onChange = (event: MediaQueryListEvent) => (systemDark = event.matches);
    media.addEventListener('change', onChange);
    return () => media.removeEventListener('change', onChange);
  });

  $effect(() => {
    const dark = prefs.v.look === 'SYSTEM' ? systemDark : prefs.v.look !== 'LIGHT';
    applyTheme(prefs.theme, dark, prefs.v.look === 'OBSIDIAN');
    // Untracked, or the effect would read what it writes and run forever.
    untrack(() => prefs.themeApplied++);
  });

  $effect(() => {
    const stop = listen('palette-opened', () => {
      // The main window says what an internal error did; this box only shows the vault locked.
      stopped.on = false;
      query = '';
      refreshNow(true);
      input?.focus();
    });
    // A lock, from the timer, the tray or the main window, takes the titles and the search out of
    // this hidden box too, rather than leaving them until it next opens. So does an internal error,
    // which the main window explains.
    const stopLock = listen<string>('command', (event) => {
      if (event.payload !== 'lock' && event.payload !== 'panic' && event.payload !== 'internal') return;
      stopped.on = false;
      asked++;
      query = '';
      rows = [];
      locked = true;
    });
    refreshNow(true);
    return () => {
      void stop.then((run) => run());
      void stopLock.then((run) => run());
    };
  });

  async function copyPassword(entry: Summary) {
    await pass.copySecret('entry.reveal', { id: entry.id, field: 'password' }, prefs.v.clipboard_seconds);
    await desktop.hidePalette();
  }

  async function act(entry: Summary, event: KeyboardEvent | MouseEvent) {
    failed = '';
    try {
      // Nothing of that kind to copy: the box stays, saying so, rather than closing as if it had.
      const nothing = await actOn(entry, event);
      if (nothing) failed = nothing;
    } catch (failure: any) {
      // Most often the vault locked while the box was open: show it as it is now. Still open, the
      // copy itself failed, and that is said rather than the box sitting there as if it worked.
      await refresh().catch(() => undefined);
      if (!locked) failed = failure?.message ?? 'That could not be copied.';
    }
  }

  async function actOn(entry: Summary, event: KeyboardEvent | MouseEvent): Promise<string | void> {
    if (event.shiftKey && prefs.v.auto_type_enabled && entry.has_password) {
      await desktop.autoType(entry.id);
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      const detail = await pass.detail(entry.id);
      // A database item calls it user, as the main window knows.
      const name = detail.fields.find((f) => f.key === 'username' || f.key === 'user')?.value;
      if (!name) return 'This item has no username.';
      await host().copyText(String(name), false, 0);
      await desktop.hidePalette();
      return;
    }
    if (entry.has_password) await copyPassword(entry);
    else if (entry.has_otp) {
      await pass.copySecret('otp.code', { id: entry.id }, prefs.v.clipboard_seconds);
      await desktop.hidePalette();
    } else return 'This item has no password or code to copy.';
  }

  // Focus stays in the search box while the arrows move through the rows: the box is a combobox
  // whose active option is the chosen row. Field has no props for these, so they go on its input.
  $effect(() => {
    if (!input) return;
    input.setAttribute('role', 'combobox');
    input.setAttribute('aria-controls', 'palette-rows');
    input.setAttribute('aria-expanded', String(rows.length > 0));
    if (rows[chosen]) input.setAttribute('aria-activedescendant', `palette-row-${rows[chosen].id}`);
    else input.removeAttribute('aria-activedescendant');
  });

  function keys(event: KeyboardEvent) {
    // The input method's own keys while it composes: Enter takes its word, the arrows its choices.
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      void desktop.hidePalette();
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      chosen = Math.max(0, Math.min(chosen + 1, rows.length - 1));
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      chosen = Math.max(chosen - 1, 0);
    } else if (event.key === 'Enter' && waiting) {
      // Enter straight after typing acts on what was typed, not on the rows for the letters before it.
      event.preventDefault();
      clearTimeout(typing);
      waiting = false;
      void refresh()
        .then(() => rows[chosen] && act(rows[chosen], event))
        .catch(() => (rows = []));
    } else if (event.key === 'Enter' && rows[chosen]) {
      event.preventDefault();
      void act(rows[chosen], event);
    }
  }
</script>

<!-- Locked, there is no field to take Escape, and the box still has to close by keyboard. -->
<svelte:window onkeydown={(event) => { if (locked && event.key === 'Escape') { event.preventDefault(); void desktop.hidePalette(); } }} />

<div class="palette" data-tauri-drag-region>
  {#if locked}
    <div class="empty" style="margin:auto">
      <p>WM Pass is locked.</p>
      <p style="padding-top:10px"><button type="button" class="btn primary" onclick={() => desktop.showMain()}>Open WM Pass</button></p>
    </div>
  {:else}
    <div class="head">
      <Field bind:value={query} bind:ref={input} placeholder="Search your vault" autofocus oninput={typed} onkeydown={keys} />
    </div>
    <div class="list scroll" id="palette-rows" role="listbox" aria-label="Matches">
      {#each rows as entry, index (entry.id)}
        <button type="button" class="row" role="option" id="palette-row-{entry.id}" tabindex="-1" aria-selected={index === chosen} onclick={(e) => act(entry, e)} onpointermove={() => (chosen = index)}>
          <Tile title={entry.title} site={entry.site} kind={entry.kind} label={entry.color} icon={entry.icon} size={28} />
          <span class="grow">
            <span class="title ell" style="display:block">{entry.title || 'Untitled'}</span>
            <span class="sub ell" style="display:block">{entry.subtitle || kindName(entry.kind)}</span>
          </span>
        </button>
      {/each}
      {#if rows.length === 0}<p class="empty">Nothing matches.</p>{/if}
    </div>
    {#if failed}
      <p class="danger-text small" role="alert" style="padding:6px 12px;border-top:1px solid var(--hair)">{failed}</p>
    {/if}
    <p class="dimmer small" style="padding:6px 12px;border-top:1px solid var(--hair)">
      Enter copies the password. {host().os === 'mac' ? '⌘' : 'Ctrl+'}Enter copies the username.{prefs.v.auto_type_enabled ? ` ${host().os === 'mac' ? '⇧' : 'Shift+'}Enter types it.` : ''}
    </p>
  {/if}
</div>
