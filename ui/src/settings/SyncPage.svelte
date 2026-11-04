<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { ui } from '../vault/ui.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { help } from '../help/help.svelte';
  import { PAGES } from './pages';
  import Toggle from './Toggle.svelte';
  import { accounts, type AccountRef } from './accounts.svelte';
  import AddAccount from './AddAccount.svelte';
  import LanSection from './LanSection.svelte';
  import PlaceDialog from './PlaceDialog.svelte';
  import { places } from './places.svelte';

  let refreshing = $state(false);
  let addingAccount = $state(false);
  let addingPlace = $state(false);
  let fileSyncing = $state(false);
  let fileMessage = $state<string | null>(null);

  const current = $derived(accounts.list.find((a) => a.id === pass.account));

  let choosing = $state(false);
  let guideReady = $state(false);
  void help.load().then(() => (guideReady = true));

  /** What can sync, named as the guide names it and grouped by the page it is on. */
  const syncable = $derived.by(() => {
    void guideReady;
    return prefs.syncable
      .map((key) => {
        const topic = help.topic(`setting.${key}`);
        const page = PAGES.find((p) => p.id === topic?.page)?.title ?? '';
        return { key, title: topic?.title ?? key.replaceAll('_', ' '), page, here: !!topic };
      })
      .filter((s) => s.here)
      .sort((a, b) => a.page.localeCompare(b.page) || a.title.localeCompare(b.title));
  });
  const skipped = $derived(prefs.listOf('sync_skip'));

  async function syncAccount() {
    refreshing = true;
    await accounts.syncNow();
    refreshing = false;
  }

  async function syncFiles() {
    fileSyncing = true;
    await places.syncNow();
    fileMessage = places.syncProblem ?? 'Up to date.';
    fileSyncing = false;
  }

  async function remove(item: AccountRef) {
    const yes = await overlays.ask({ title: `Remove ${item.label}?`, body: 'WM Pass forgets this account on this computer, with its offline copy. Nothing on the server or in the file changes.', action: 'Remove', danger: true });
    if (yes) await accounts.remove(item.id);
  }
</script>

{#if pass.account && current?.kind !== 'kdbx'}
  <Group title="This account" help="group.sync.account">
    <Row
      title={current?.label ?? 'Server'}
      sub={accounts.syncProblem ?? 'Changes go up after each edit. If the server cannot be reached they wait here.'}
      icon="sync"
    >
      <IconBtn icon="sync" label="Sync now" disabled={refreshing} onclick={syncAccount} />
    </Row>
  </Group>
{/if}

{#if host().lan}<LanSection />{/if}

{#if places.available}
  <Group title="Through a folder or server" help="group.sync.files" anchor="group:sync.places">
    <div class="set-body"><p class="dim">Each device leaves its own encrypted copy there and merges the others. Pick any place you set up for backups.</p></div>
    {#each places.list as place (place.id)}
      <Row title={place.label} icon={place.kind === 'folder' ? 'folder' : 'cloud_sync'}>
        <Switch checked={place.sync} label="Sync through {place.label}" onchange={(on) => places.put({ ...place, sync: on })} />
      </Row>
    {/each}
    <Row title="Add a place" icon="add" chevron onclick={() => (addingPlace = true)} />
    {#if places.list.some((p) => p.sync)}
      <div class="set-body stack">
        <div><Btn label={fileSyncing ? 'Syncing…' : 'Sync now'} icon="sync" disabled={fileSyncing} onclick={syncFiles} /></div>
        {#if fileMessage}<p class="dim">{fileMessage}</p>{/if}
      </div>
    {/if}
  </Group>
{/if}

<Group title="Settings" help="setting.sync_settings">
  <Toggle title="Sync settings with the vault" sub="Your customizations follow this vault to your other devices. Protection settings stay here." icon="tune" pref="sync_settings" />
  {#if prefs.v.sync_settings}
    <Row title="Settings that stay on this device" sub={skipped.length ? `${skipped.length} kept here` : 'None: everything syncs'} icon="devices" help="setting.sync_skip" chevron onclick={() => (choosing = !choosing)} />
    {#if choosing}
      {#each syncable as item (item.key)}
        <Row title={item.title} sub={item.page}>
          <Switch checked={!skipped.includes(item.key)} label="Sync {item.title}" onchange={(on) => prefs.toggleInList('sync_skip', item.key, !on)} />
        </Row>
      {/each}
    {/if}
  {/if}
</Group>

<Group title="Accounts held elsewhere" help="group.sync.accounts" anchor="group:sync.accounts">
  <div class="set-body"><p class="dim">Open a KeePass file where it is, or sign in to Bitwarden, Vaultwarden or a WM Pass server. They appear on the door beside this vault.</p></div>
  {#each accounts.list as item (item.id)}
    <Row title={item.label} sub={item.kind === 'kdbx' ? 'KeePass file' : item.server || 'bitwarden.com'} icon={item.kind === 'kdbx' ? 'key' : 'sync'} onclick={() => remove(item)} />
  {/each}
  <Row title="Add an account" help="action.add_account" icon="add" chevron onclick={() => (addingAccount = true)} />
</Group>

<Group title="Vaults inside this one">
  <Row title="Nested and shared vaults" sub="Separate spaces, and vaults you share with family" icon="family_restroom" chevron onclick={() => ui.go('settings/vaults')} />
</Group>

<Group title="Hand over one item">
  <Row title="Receive an item" sub="Show a code to the other device" icon="qr_code_scanner" chevron onclick={() => ui.go('settings/receive')} />
</Group>

{#if addingAccount}<AddAccount onclose={() => (addingAccount = false)} />{/if}
{#if addingPlace}<PlaceDialog onclose={() => (addingPlace = false)} />{/if}
