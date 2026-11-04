<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { host } from '../engine/host';
  import { chordOf } from '../engine/text';
  import BulkPanel from './BulkPanel.svelte';
  import CodesPage from './CodesPage.svelte';
  import Detail from './Detail.svelte';
  import Editor from './Editor.svelte';
  import GeneratorPage from './GeneratorPage.svelte';
  import HealthPage from './HealthPage.svelte';
  import ListPane from './ListPane.svelte';
  import Sidebar from './Sidebar.svelte';
  import Settings from '../settings/Settings.svelte';
  import { TAB_INFO } from './tabs';
  import { ui } from './ui.svelte';
  import { help } from '../help/help.svelte';

  let editor = $state<Editor>();
  let shelfPassword = $state('');
  let shelfError = $state<string | null>(null);
  let shelfOpen = $state(false);
  let dragging = $state<null | 'sidebar' | 'list'>(null);
  let folderName = $state('');

  $effect(() => {
    if (ui.renamingFolder) folderName = ui.renamingFolder.name;
  });

  $effect(() => {
    void pass.revision;
    if (ui.section === 'shelf') void pass.info().then((info) => (shelfOpen = info.shelf_open));
  });

  const isList = $derived(ui.isListSection(ui.section));
  const entry = $derived(pass.entries.find((e) => e.id === ui.selected));

  function current() {
    return pass.entries.find((e) => e.id === ui.selected);
  }

  /** Everything a shortcut, a menu or the tray can ask for. */
  export function command(name: string) {
    switch (name) {
      case 'search':
        if (!isList) ui.go('all');
        ui.searchFocus += 1;
        break;
      case 'add':
        ui.add(prefs.listOf('add_kinds')[0] ?? 'login');
        break;
      case 'lock':
        void pass.lock();
        break;
      case 'generator':
      case 'codes':
      case 'health':
        void ui.guard(() => ui.go(name));
        break;
      case 'settings':
        void ui.guard(() => ui.go('settings'));
        break;
      case 'edit': {
        const e = current();
        if (e) ui.edit(e.id, e.kind);
        break;
      }
      case 'copy_password': {
        const e = current();
        // With no password, the code and then the username, as the row's copy button does.
        const action = e && ui.effective('password', e);
        if (e && action) void ui.perform(action, e);
        break;
      }
      case 'copy_username': {
        const e = current();
        if (e) void ui.copyUsername(e.id);
        break;
      }
      case 'copy_code': {
        const e = current();
        if (e?.has_otp) void ui.copyCode(e.id);
        break;
      }
      case 'open_site': {
        const e = current();
        if (e) void ui.openSite(e.id);
        break;
      }
      case 'toggle_sidebar':
        prefs.set('sidebar_open', !prefs.v.sidebar_open);
        break;
      case 'palette':
        ui.searchFocus += 1;
        break;
      case 'help':
        help.openHere();
        break;
      case 'guide':
        void ui.guard(() => ui.go('settings/guide'));
        break;
      case 'tour':
        void ui.guard(() => help.startWindowTour());
        break;
      case 'reload':
        void pass.reload();
        break;
      case 'window-hidden':
        if (prefs.v.lock_on_hide) void pass.lock();
        break;
      case 'backups':
        void ui.guard(() => ui.go('settings/backup'));
        break;
      default:
        if (name.startsWith('add:')) ui.add(name.slice(4));
        // From the Favorites widget: an item id, which names nothing by itself.
        else if (name.startsWith('item:')) {
          const id = name.slice(5);
          void ui.guard(() => ui.open(id));
        }
    }
  }

  function keyed(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      if (ui.query) {
        ui.query = '';
        ui.searchResults = null;
      }
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === 's' && ui.editing) {
      event.preventDefault();
      void editor?.save();
      return;
    }
    const chord = chordOf(event, host().os);
    if (!chord) return;
    for (const [action, bound] of Object.entries(prefs.shortcuts)) {
      if (bound === chord) {
        event.preventDefault();
        command(action);
        return;
      }
    }
  }

  function startDrag(which: 'sidebar' | 'list', event: PointerEvent) {
    dragging = which;
    const startX = event.clientX;
    const startWidth = which === 'sidebar' ? prefs.v.sidebar_width : prefs.v.list_width;
    const move = (e: PointerEvent) => {
      const next = startWidth + e.clientX - startX;
      if (which === 'sidebar') prefs.v.sidebar_width = Math.min(360, Math.max(160, next));
      else prefs.v.list_width = Math.min(560, Math.max(240, next));
    };
    const up = () => {
      dragging = null;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      prefs.set('sidebar_width', prefs.v.sidebar_width);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  async function openShelf() {
    shelfError = null;
    const secret = bytesOf(shelfPassword);
    try {
      await pass.change('shelf.open', {}, { secret });
      shelfPassword = '';
      shelfOpen = true;
    } catch (failure: any) {
      shelfError = failure.message;
    } finally {
      secret.fill(0);
    }
  }

  async function saveFolder() {
    const target = ui.renamingFolder;
    const name = folderName.trim();
    if (!target || !name) return;
    if (target.id) await pass.change('folder.rename', { id: target.id, name });
    else await pass.change('folder.add', { name });
    ui.renamingFolder = null;
  }

  const NARROW_TABS = ['all', 'codes', 'generator', 'settings'];
</script>

<svelte:window onkeydown={keyed} />

<div class="shell" class:narrow={ui.narrow} style={ui.narrow ? 'flex-direction:column' : ''}>
  {#if !ui.narrow && prefs.v.sidebar_open && !ui.section.startsWith('settings')}
    <Sidebar />
    <div class="drag-handle" class:dragging={dragging === 'sidebar'} role="separator" onpointerdown={(e) => startDrag('sidebar', e)}></div>
  {/if}

  <div style="flex:1;min-height:0;min-width:0;display:flex">
    {#if ui.section.startsWith('settings')}
      <Settings />
    {:else if ui.section === 'codes'}
      <CodesPage />
    {:else if ui.section === 'generator'}
      <GeneratorPage />
    {:else if ui.section === 'health'}
      <HealthPage />
    {:else}
      {#if !ui.narrow || (!ui.showDetail && !ui.editing)}
        <ListPane />
        {#if !ui.narrow}
          <div class="drag-handle" class:dragging={dragging === 'list'} role="separator" onpointerdown={(e) => startDrag('list', e)}></div>
        {/if}
      {/if}
      {#if !ui.narrow || ui.showDetail || ui.editing}
        <main class="detail">
          {#if ui.editing}
            {#key ui.editing}
              <Editor bind:this={editor} editing={ui.editing} />
            {/key}
          {:else if ui.multi.length > 1}
            <BulkPanel />
          {:else if ui.selected && (entry || ui.section === 'trash' || ui.section === 'shelf')}
            {#key ui.selected}
              <Detail id={ui.selected} />
            {/key}
          {:else if ui.section === 'shelf' && !shelfOpen}
            <div class="detail-body" style="padding-top:60px">
              <div class="detail-inner stack" style="max-width:360px">
                <h2 class="page-title">{TAB_INFO.shelf.label}</h2>
                <p class="dim">Entries on the shelf stay out of every list, search and autofill until you open it with your password. It closes again when the vault locks.</p>
                <PasswordField bind:value={shelfPassword} placeholder="Master password" error={!!shelfError} onenter={openShelf} />
                {#if shelfError}<p class="danger-text small">{shelfError}</p>{/if}
                <div><Btn label="Open the shelf" variant="primary" disabled={!shelfPassword} onclick={openShelf} /></div>
              </div>
            </div>
          {:else}
            <div class="empty dimmer" style="margin:auto" data-tauri-drag-region>
              {#if pass.entries.length > 0}Choose an item{/if}
            </div>
          {/if}
        </main>
      {/if}
    {/if}
  </div>

  {#if ui.narrow && !ui.editing && !ui.showDetail}
    <nav class="narrow-tabs" aria-label="Sections" data-help-anchor="narrow.tabs">
      {#each NARROW_TABS as tab}
        {@const info = tab === 'settings' ? { label: 'Settings', icon: 'settings' as const } : TAB_INFO[tab]}
        <button type="button" aria-current={tab === 'settings' ? ui.section.startsWith('settings') : ui.section === tab} onclick={() => ui.go(tab)}>
          <Icon name={info.icon} size={20} />
          {info.label}
        </button>
      {/each}
    </nav>
  {/if}
</div>

{#if ui.renamingFolder}
  <Dialog title={ui.renamingFolder.id ? 'Rename folder' : 'New folder'} onclose={() => (ui.renamingFolder = null)}>
    <Field bind:value={folderName} placeholder="Name" autofocus onenter={saveFolder} />
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (ui.renamingFolder = null)} />
      <Btn label="Done" variant="primary" disabled={!folderName.trim()} onclick={saveFolder} />
    {/snippet}
  </Dialog>
{/if}
