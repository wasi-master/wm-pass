<script lang="ts">
  import { tick, untrack } from 'svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { host } from '../engine/host';
  import { chordsOf, nameText } from '../engine/text';
  import BulkPanel from './BulkPanel.svelte';
  import CodesPage from './CodesPage.svelte';
  import Detail, { loadDetail, type Loaded } from './Detail.svelte';
  import Editor, { loadEditor, type EditorLoaded } from './Editor.svelte';
  import GeneratorPage from './GeneratorPage.svelte';
  import HealthPage from './HealthPage.svelte';
  import ListPane from './ListPane.svelte';
  import Sidebar from './Sidebar.svelte';
  import TagManage from './TagManage.svelte';
  import { tags } from '../engine/tags.svelte';
  import { siteIcons } from '../engine/siteIcons.svelte';
  import Settings from '../settings/Settings.svelte';
  import { TAB_INFO } from './tabs';
  import { overlays } from '../parts/floating.svelte';
  import { ui, type Editing } from './ui.svelte';
  import { help } from '../help/help.svelte';
  import Hint from '../help/Hint.svelte';
  import { healthMemory } from './health.svelte';

  let editor = $state<Editor>();
  let shelfPassword = $state('');
  let shelfError = $state<string | null>(null);
  let shelfOpen = $state(false);
  let shelfChecking = $state(false);
  let dragging = $state<null | 'sidebar' | 'list'>(null);
  let folderName = $state('');

  $effect(() => {
    if (ui.renamingFolder) folderName = ui.renamingFolder.name;
  });

  // Open on, under Settings, Sidebar: this is made each time a vault opens, so it starts on the
  // first page in the sidebar, or on the one used last when that is still offered.
  untrack(() => {
    const tabs = prefs.listOf('tabs');
    const last = prefs.v.last_tab;
    const lastOffered = !!last && (tabs.includes(last) || last.startsWith('kind:'));
    ui.section = prefs.v.start_tab === 'last' && lastOffered ? last : (tabs[0] ?? 'all');
  });

  $effect(() => {
    void pass.revision;
    void tags.refresh();
  });

  // Locking takes the websites' icons out of memory with everything else, and what was on show:
  // the search, the item chosen, the last health report.
  $effect(() => () => {
    siteIcons.forget();
    // The tags' symbols name the tags, which the next vault to open may not have.
    tags.icons = {};
    ui.reset();
    healthMemory.last = null;
    healthMemory.open = [];
  });

  $effect(() => {
    void pass.revision;
    if (ui.section === 'shelf') void pass.info().then((info) => (shelfOpen = info.shelf_open), () => undefined);
  });

  // Leaving the shelf puts it away again; opening one of its items does not, since that stays here.
  let onShelf = false;
  $effect(() => {
    const here = ui.section === 'shelf';
    if (onShelf && !here) {
      shelfPassword = '';
      shelfError = null;
      void pass
        .info()
        .then((info) => (info.shelf_open ? pass.change('shelf.close') : null))
        .catch(() => undefined);
    }
    onShelf = here;
  });

  const isList = $derived(ui.isListSection(ui.section));
  const entry = $derived(pass.entries.find((e) => e.id === ui.selected));

  // The item on show. It moves to the chosen one only once that one has loaded, so the page
  // already there stays up through the round trip to the engine instead of the pane going blank.
  let shown = $state.raw<{ id: string; loaded: Loaded | null } | null>(null);
  $effect(() => {
    const id = ui.selected;
    const revision = pass.revision;
    if (!id) {
      // Starting a new item clears the choice; the page stays until the empty form is ready.
      if (!ui.editing) shown = null;
      return;
    }
    const now = untrack(() => shown);
    if (now?.id === id && now.loaded?.revision === revision) return;
    const done = (loaded: Loaded | null) => {
      if (ui.selected === id) shown = { id, loaded };
    };
    loadDetail(id).then(done, () => done(null));
  });

  // The form on show, held the same way: it opens once filled in, and after a save it stays until
  // the item's page has caught up with what was saved.
  let editShown = $state.raw<{ editing: Editing; loaded: EditorLoaded | null } | null>(null);
  $effect(() => {
    const editing = ui.editing;
    if (!editing) {
      const id = ui.selected;
      const caughtUp = !id || !shown || (shown.id === id && (shown.loaded === null || shown.loaded.revision === pass.revision));
      if (caughtUp) editShown = null;
      return;
    }
    if (untrack(() => editShown)?.editing === editing) return;
    const done = (loaded: EditorLoaded | null) => {
      if (ui.editing === editing) editShown = { editing, loaded };
    };
    loadEditor(editing).then(done, () => done(null));
  });

  function current() {
    return pass.entries.find((e) => e.id === ui.selected);
  }

  /** Everything a shortcut, a menu or the tray can ask for. */
  export function command(name: string) {
    switch (name) {
      case 'search':
      case 'palette':
        if (!isList) ui.go('all');
        // On a narrow window the list is behind the item's page.
        if (ui.narrow && !ui.editing) ui.showDetail = false;
        // After the list is drawn: a list that comes into view only now would take the ask as
        // the one already seen, and the search box would not get the focus.
        void tick().then(() => (ui.searchFocus += 1));
        break;
      case 'add':
        ui.add(prefs.listOf('add_kinds')[0] ?? 'login');
        break;
      case 'lock':
        void pass.lock().catch((error: Error) => overlays.toast(error?.message || 'The vault could not be locked.'));
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
        // An item in the trash is only looked at, as its page and its menu have it.
        if (e && ui.canDo('edit', e)) ui.edit(e.id, e.kind);
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
        void pass.reload().catch((error: Error) => overlays.toast(error?.message || 'The vault could not be read again.'));
        break;
      case 'linked-change':
        // The browser extension changed the vault open here. This window holds the file.
        void pass
          .reload()
          .then(() => pass.flushSoon())
          .catch((error: Error) => overlays.toast(error?.message || 'The change from the browser could not be read.'));
        break;
      case 'window-hidden':
        if (prefs.v.lock_on_hide) void pass.lock().catch(() => undefined);
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
    // Escape that ends an input method's composition (229 in WebKit) is not for the editor or the
    // search, which would otherwise be emptied along with the word being composed.
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === 'Escape') {
      // Escape in a dialog closes the dialog, and in the editor closes the editor: neither should
      // throw away the search as well.
      const inDialog = (event.target as Element | null)?.closest?.('dialog');
      // The editor takes it as its Close button, asking first about changes, unless a menu, a
      // picker or a question over it has it.
      if (ui.editing && editor && !event.defaultPrevented && !inDialog && !overlays.menu && !document.querySelector('dialog[open]')) {
        event.preventDefault();
        void editor.leave();
        return;
      }
      if (ui.query && !event.defaultPrevented && !inDialog && !ui.editing) {
        ui.query = '';
        ui.searchResults = null;
      }
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's' && ui.editing) {
      event.preventDefault();
      // A picker, the generator or a question over the form is answered first: saving under it
      // would leave out what it was about to put in.
      if (document.querySelector('dialog[open]') || overlays.menu) return;
      void editor?.save();
      return;
    }
    // As it is named now, or as a shortcut saved before named it.
    const chords = chordsOf(event, host().os);
    if (!chords.length) return;
    // A shortcut set to a plain key, such as / or a letter, would take that key away from every
    // field, passwords included. In a field only chords with a modifier or F-keys count.
    const target = event.target as HTMLElement | null;
    const typing = !!target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);
    if (typing && !event.metaKey && !event.ctrlKey && !event.altKey && !/^F\d+$/.test(event.key)) return;
    // A question, picker or sheet open over the window is answered first: a shortcut acting behind
    // it would change the page it is about, or put a second question over the first. Locking and
    // the guide still work from anywhere.
    const covered = !!document.querySelector('dialog[open]') || !!overlays.menu;
    for (const [action, bound] of Object.entries(prefs.shortcuts)) {
      if (bound && chords.includes(bound)) {
        if (covered && action !== 'lock' && action !== 'help') return;
        event.preventDefault();
        command(action);
        return;
      }
    }
  }

  function startDrag(which: 'sidebar' | 'list', event: PointerEvent) {
    if (event.button !== 0) return;
    // Held by the handle, so letting go past the window's edge still ends the drag, rather than
    // leaving the pane following the pointer once it comes back with no button down.
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
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
      window.removeEventListener('pointercancel', up);
      if (which === 'sidebar') prefs.set('sidebar_width', prefs.v.sidebar_width);
      else prefs.set('list_width', prefs.v.list_width);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
  }

  /** The arrow keys move a focused divider, as dragging it does. */
  function dragKey(which: 'sidebar' | 'list', event: KeyboardEvent) {
    const step = event.key === 'ArrowLeft' ? -16 : event.key === 'ArrowRight' ? 16 : 0;
    if (!step) return;
    event.preventDefault();
    if (which === 'sidebar') prefs.set('sidebar_width', Math.min(360, Math.max(160, prefs.v.sidebar_width + step)));
    else prefs.set('list_width', Math.min(560, Math.max(240, prefs.v.list_width + step)));
  }

  async function openShelf() {
    if (shelfChecking || !shelfPassword) return;
    shelfError = null;
    shelfChecking = true;
    const secret = bytesOf(shelfPassword);
    try {
      await pass.change('shelf.open', {}, { secret });
      shelfPassword = '';
      shelfOpen = true;
    } catch (failure: any) {
      shelfError = failure?.message || 'The shelf could not be opened.';
    } finally {
      secret.fill(0);
      shelfChecking = false;
    }
  }

  let savingFolder = false;
  async function saveFolder() {
    const target = ui.renamingFolder;
    const name = nameText(folderName);
    // Enter pressed twice in a row would otherwise make the folder twice.
    if (!target || !name || savingFolder) return;
    savingFolder = true;
    try {
      if (target.id) await pass.change('folder.rename', { id: target.id, name });
      else await pass.change('folder.add', { name });
      ui.renamingFolder = null;
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The folder could not be saved.');
    } finally {
      savingFolder = false;
    }
  }

  // On a narrow window the shelf's password box is where the item's page goes, behind the list:
  // with the shelf shut and nothing chosen, that page is the one shown, or it could not be opened.
  const shelfGate = $derived(ui.section === 'shelf' && !shelfOpen && !ui.selected && !ui.editing);

  const NARROW_TABS = ['all', 'codes', 'generator', 'settings'];

  // The page the window is on, in its title, which a screen reader says on switching to it.
  $effect(() => {
    const section = ui.section;
    const name = section.startsWith('settings')
      ? 'Settings'
      : section.startsWith('folder:')
        ? (pass.folders.find((f) => f.id === section.slice(7))?.name ?? 'Folder')
        : section.startsWith('tag:')
          ? section.slice(4)
          : (TAB_INFO[section.startsWith('kind:') ? 'all' : section]?.label ?? '');
    document.title = name ? `${name} – WM Pass` : 'WM Pass';
    return () => (document.title = 'WM Pass');
  });

  // A button that goes with what it did, such as an item's More menu once the item is in the trash,
  // Save once the form closes, or a dialog's Done with the dialog, leaves the keyboard nowhere, and
  // the next Tab starts again at the top of the window. The focus goes instead to what is now on
  // show: the item's name, the page's title, or the list.
  $effect(() => {
    let lost: Element | null = null;
    let frame = 0;
    const left = (event: FocusEvent) => {
      if (event.relatedTarget) return;
      lost = event.target as Element;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const gone = lost;
          lost = null;
          if (!gone || gone.isConnected) return;
          const now = document.activeElement;
          if (now && now !== document.body) return;
          // A sheet open over the window looks after its own.
          if (document.querySelector('dialog[open]')) return;
          const places = ['.detail h1[tabindex]', '.settings-pane h1[tabindex]', '.list-body[role="listbox"]', '.settings-nav [aria-current="true"]', '.shell > main'];
          for (const place of places) {
            const home = document.querySelector<HTMLElement>(place);
            if (home) return home.focus({ preventScroll: true });
          }
        }),
      );
    };
    document.addEventListener('focusout', left, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener('focusout', left, true);
    };
  });
</script>

<svelte:window onkeydown={keyed} />

<div class="shell" class:narrow={ui.narrow} style={ui.narrow ? 'flex-direction:column' : ''}>
  {#if !ui.narrow && prefs.v.sidebar_open && !ui.section.startsWith('settings')}
    <Sidebar />
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (a focusable separator is a control) -->
    <div class="drag-handle" class:dragging={dragging === 'sidebar'} role="separator" tabindex="0" aria-orientation="vertical" aria-label="Resize the sidebar" aria-valuemin={160} aria-valuemax={360} aria-valuenow={prefs.v.sidebar_width} onpointerdown={(e) => startDrag('sidebar', e)} onkeydown={(e) => dragKey('sidebar', e)}></div>
  {/if}

  <main style="flex:1;min-height:0;min-width:0;display:flex" tabindex="-1">
    {#if ui.section.startsWith('settings')}
      <Settings />
    {:else if ui.section === 'codes'}
      <CodesPage />
    {:else if ui.section === 'generator'}
      <GeneratorPage />
    {:else if ui.section === 'health'}
      <HealthPage />
    {:else}
      {#if !ui.narrow || (!ui.showDetail && !ui.editing && !shelfGate)}
        <ListPane />
        {#if !ui.narrow}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (a focusable separator is a control) -->
          <div class="drag-handle" class:dragging={dragging === 'list'} role="separator" tabindex="0" aria-orientation="vertical" aria-label="Resize the list" aria-valuemin={240} aria-valuemax={560} aria-valuenow={prefs.v.list_width} onpointerdown={(e) => startDrag('list', e)} onkeydown={(e) => dragKey('list', e)}></div>
        {/if}
      {/if}
      {#if !ui.narrow || ui.showDetail || ui.editing || shelfGate}
        <section class="detail" aria-label={editShown ? 'Editor' : ui.multi.length > 1 ? 'Chosen items' : 'Item'}>
          {#if editShown}
            {#key editShown.editing}
              <Editor bind:this={editor} editing={editShown.editing} initial={editShown.loaded} />
            {/key}
          {:else if ui.multi.length > 1}
            <BulkPanel />
          {:else if shown && (ui.selected ? entry || ui.section === 'trash' || ui.section === 'shelf' : ui.editing)}
            {#key shown.id}
              <Detail id={shown.id} initial={shown.loaded} />
            {/key}
          {:else if ui.section === 'shelf' && !shelfOpen}
            <div class="detail-body" style="padding-top:60px">
              <div class="detail-inner stack" style="max-width:360px">
                <h2 class="page-title">{TAB_INFO.shelf.label}</h2>
                <Hint id="shelf" topic="screen.shelf" />
                <p class="dim">Entries on the shelf stay out of every list, search and autofill until you open it with your password. It closes again when you leave this page or the vault locks.</p>
                <PasswordField bind:value={shelfPassword} placeholder="Master password" error={!!shelfError} describedby={shelfError ? 'shelf-problem' : undefined} onenter={openShelf} />
                {#if shelfError}<p class="danger-text small" role="alert" id="shelf-problem">{shelfError}</p>{/if}
                <div><Btn label={shelfChecking ? 'Checking the password…' : 'Open the shelf'} variant="primary" disabled={!shelfPassword || shelfChecking} onclick={openShelf} /></div>
              </div>
            </div>
          {:else}
            <div class="empty dimmer" style="margin:auto" data-tauri-drag-region>
              {#if pass.entries.length > 0}Choose an item{/if}
            </div>
          {/if}
        </section>
      {/if}
    {/if}
  </main>

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

{#if ui.managingTag}
  <TagManage tag={ui.managingTag} onclose={() => (ui.managingTag = null)} />
{/if}

{#if ui.renamingFolder}
  <Dialog title={ui.renamingFolder.id ? 'Rename folder' : 'New folder'} onclose={() => (ui.renamingFolder = null)}>
    <Field bind:value={folderName} placeholder="Name" autofocus onenter={saveFolder} />
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (ui.renamingFolder = null)} />
      <Btn label="Done" variant="primary" disabled={!folderName.trim()} onclick={saveFolder} />
    {/snippet}
  </Dialog>
{/if}
