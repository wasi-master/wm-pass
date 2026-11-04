<script lang="ts">
  import HelpSheet from '../../ui/src/help/HelpSheet.svelte';
  import { help } from '../../ui/src/help/help.svelte';
  import Btn from '../../ui/src/parts/Btn.svelte';
  import Field from '../../ui/src/parts/Field.svelte';
  import IconBtn from '../../ui/src/parts/IconBtn.svelte';
  import Tile from '../../ui/src/parts/Tile.svelte';
  import Door from '../../ui/src/door/Door.svelte';
  import { applyTheme } from '../../ui/src/theme/themes';
  import { kindName } from '../../ui/src/engine/face';
  import { host } from '../../ui/src/engine/host';
  import { pass } from '../../ui/src/engine/pass.svelte';
  import { prefs } from '../../ui/src/engine/prefs.svelte';
  import { send } from './host';
  import type { SiteAnswer, SiteRow } from './shared';

  const chrome = (globalThis as any).chrome;

  let tabUrl = $state('');
  let site = $state<SiteAnswer | null>(null);
  let query = $state('');
  let message = $state<string | null>(null);
  // The row Enter acts on, as in the desktop app's quick search.
  let chosen = $state(0);

  const dark = matchMedia('(prefers-color-scheme: dark)').matches;
  applyTheme(prefs.theme, dark, false);

  const unlocked = $derived(pass.state.kind === 'unlocked');
  const fromDesktop = $derived(site?.source === 'desktop' && !site.locked);
  const hostname = $derived.by(() => {
    try {
      return tabUrl.startsWith('http') ? new URL(tabUrl).hostname : '';
    } catch {
      return '';
    }
  });

  async function load() {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    tabUrl = tab?.url ?? '';
    site = tabUrl.startsWith('http') ? await send<SiteAnswer>({ type: 'site', url: tabUrl }) : { source: unlocked ? 'vault' : 'none', locked: !unlocked, rows: [], foreign: null };
  }

  $effect(() => {
    void pass.state.kind;
    void load();
  });

  // The whole vault, ranked by the engine's own search: logins, codes, cards and notes alike.
  let found = $state.raw<(SiteRow & { login: boolean })[]>([]);
  let searching = 0;
  $effect(() => {
    const words = query.trim();
    const run = ++searching;
    if (!words || !unlocked) {
      found = [];
      return;
    }
    void pass.search(words).then((list) => {
      if (run !== searching) return;
      found = list.slice(0, 30).map((e) => ({
        id: e.id,
        title: e.title,
        subtitle: e.subtitle || kindName(e.kind),
        site: e.site,
        icon: e.icon,
        has_password: e.has_password,
        has_otp: e.has_otp,
        login: e.kind === 'login',
      }));
    });
  });

  const rows = $derived<(SiteRow & { login?: boolean })[]>(query.trim() ? found : (site?.rows ?? []));
  const isOnSite = (row: SiteRow) => site?.rows.some((r) => r.id === row.id) ?? false;
  $effect(() => {
    void rows;
    chosen = 0;
  });

  /** Enter fills a login for this page, or copies the password or code. Mod+Enter copies the username. */
  async function act(row: SiteRow & { login?: boolean }, event: KeyboardEvent) {
    if (event.metaKey || event.ctrlKey) {
      if (row.login !== false) await copy(row, 'username');
    } else if (isOnSite(row)) await fill(row);
    else if (row.has_password) await copy(row, 'password');
    else if (row.has_otp) await copy(row, 'code');
  }

  function keys(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') {
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

  async function fill(row: SiteRow) {
    message = null;
    try {
      await send({ type: 'fill.tab', id: row.id });
      window.close();
    } catch (failure: any) {
      message = failure.message === 'refused' ? 'Not filled.' : failure.message;
    }
  }

  async function copy(row: SiteRow, what: 'username' | 'password' | 'code') {
    message = null;
    const clear = prefs.v.clipboard_seconds;
    if (what === 'username') await host().copyText(row.subtitle, false, 0);
    else if (what === 'password') await pass.copySecret('entry.reveal', { id: row.id, field: 'password' }, clear);
    else await pass.copySecret('otp.code', { id: row.id }, clear);
    message = what === 'username' ? 'Username copied.' : what === 'password' ? 'Password copied.' : 'Code copied.';
  }

  const openFull = () => chrome.tabs.create({ url: 'vault.html' });
</script>

<div class="popup">
  {#if unlocked || fromDesktop}
    <header>
      <div class="where">
        <strong>{hostname || 'WM Pass'}</strong>
        {#if fromDesktop && !unlocked}<span class="dim small">From the desktop app</span>{/if}
      </div>
      <IconBtn icon="help" label="About this window" onclick={() => help.open('screen.popup')} />
      <IconBtn icon="open_in_new" label="Open WM Pass in a tab" onclick={openFull} />
      {#if unlocked}<IconBtn icon="lock" label="Lock" onclick={() => pass.lock()} />{/if}
    </header>
    {#if unlocked}
      <div class="search"><Field bind:value={query} placeholder="Search your vault" autofocus onkeydown={keys} /></div>
    {/if}
    <div class="rows scroll">
      {#each rows as row, index (row.id)}
        <div class="row" class:chosen={unlocked && index === chosen} onpointermove={() => (chosen = index)} role="presentation">
          <button class="main" onclick={() => fill(row)} disabled={!isOnSite(row)} title={isOnSite(row) ? 'Fill this page' : 'This login is not for this site'}>
            <Tile title={row.title} site={row.site} icon={row.icon} size={28} />
            <span class="words">
              <span class="title">{row.title}</span>
              <span class="dim small sub">{row.subtitle}</span>
            </span>
          </button>
          {#if unlocked}
            {#if row.login !== false}<IconBtn icon="person" label="Copy username" onclick={() => copy(row, 'username')} />{/if}
            {#if row.has_password}<IconBtn icon="key" label="Copy password" onclick={() => copy(row, 'password')} />{/if}
            {#if row.has_otp}<IconBtn icon="pin" label="Copy code" onclick={() => copy(row, 'code')} />{/if}
          {/if}
        </div>
      {:else}
        <p class="dim empty">{query.trim() ? 'Nothing matches.' : hostname ? `Nothing saved for ${hostname}.` : 'Open a website to see its logins here.'}</p>
      {/each}
    </div>
    {#if message}<p class="small dim note">{message}</p>
    {:else if unlocked && rows.length > 0}<p class="dimmer small note">Enter fills this page or copies the password. {host().os === 'mac' ? '⌘' : 'Ctrl+'}Enter copies the username.</p>{/if}
  {:else if pass.state.kind === 'none'}
    <div class="first">
      <p>No vault here yet.</p>
      <p class="dim small">Create one, bring one from a backup, or connect the WM Pass desktop app in Settings.</p>
      <Btn label="Open WM Pass" variant="primary" onclick={openFull} />
    </div>
  {:else}
    <Door />
  {/if}
</div>
<HelpSheet simple />

<style>
  .popup { width: 380px; min-height: 440px; max-height: 600px; display: flex; flex-direction: column; background: var(--bg); }
  header { display: flex; align-items: center; gap: 4px; padding: 10px 10px 8px 14px; }
  .where { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .where strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .search { padding: 0 12px 8px; }
  .rows { flex: 1; min-height: 0; padding: 0 6px 8px; }
  .row { display: flex; align-items: center; gap: 2px; border-radius: var(--r-m); padding-right: 4px; }
  .row:hover, .row.chosen { background: var(--s1); }
  .main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: 0; background: none; color: inherit; text-align: left; cursor: pointer; font: inherit; }
  .main:disabled { cursor: default; }
  .words { min-width: 0; display: flex; flex-direction: column; }
  .title, .sub { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { padding: 28px 16px; text-align: center; }
  .note { padding: 6px 14px 10px; border-top: 1px solid var(--line); margin: 0; }
  .first { padding: 40px 24px; display: grid; gap: 10px; justify-items: start; }
  .first p { margin: 0; }
</style>
