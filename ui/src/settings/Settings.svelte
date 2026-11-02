<script lang="ts">
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import { ui } from '../vault/ui.svelte';
  import { highlight, ROUTE_COLOURS } from './nav.svelte';
  import { GROUPS, PAGES, SEARCH_INDEX, type PageInfo } from './pages';

  import AboutPage from './AboutPage.svelte';
  import AddMenuPage from './AddMenuPage.svelte';
  import AliasPage from './AliasPage.svelte';
  import BackupPage from './BackupPage.svelte';
  import BreachPage from './BreachPage.svelte';
  import BrowserPage from './BrowserPage.svelte';
  import DesktopLinkPage from './DesktopLinkPage.svelte';
  import CodesSettingsPage from './CodesSettingsPage.svelte';
  import ComparePage from './ComparePage.svelte';
  import DesktopPage from './DesktopPage.svelte';
  import EditorPage from './EditorPage.svelte';
  import EntryPage from './EntryPage.svelte';
  import ExportPage from './ExportPage.svelte';
  import FilePage from './FilePage.svelte';
  import FoldersPage from './FoldersPage.svelte';
  import GeneratorSettingsPage from './GeneratorSettingsPage.svelte';
  import HardwarePage from './HardwarePage.svelte';
  import HealthSettingsPage from './HealthSettingsPage.svelte';
  import IconsPage from './IconsPage.svelte';
  import ImportPage from './ImportPage.svelte';
  import InboxPage from './InboxPage.svelte';
  import ListPage from './ListPage.svelte';
  import LookPage from './LookPage.svelte';
  import NavigationPage from './NavigationPage.svelte';
  import ReceivePage from './ReceivePage.svelte';
  import SecurityPage from './SecurityPage.svelte';
  import ShelfPage from './ShelfPage.svelte';
  import ShortcutsPage from './ShortcutsPage.svelte';
  import SshPage from './SshPage.svelte';
  import SyncPage from './SyncPage.svelte';
  import ThemesPage from './ThemesPage.svelte';
  import TrashPage from './TrashPage.svelte';
  import UpdatesPage from './UpdatesPage.svelte';
  import UsernamePage from './UsernamePage.svelte';
  import VaultsPage from './VaultsPage.svelte';

  const COMPONENTS: Record<string, any> = {
    security: SecurityPage,
    shelf: ShelfPage,
    breaches: BreachPage,
    folders: FoldersPage,
    trash: TrashPage,
    import: ImportPage,
    export: ExportPage,
    backup: BackupPage,
    sync: SyncPage,
    vaults: VaultsPage,
    receive: ReceivePage,
    hardware: HardwarePage,
    aliases: AliasPage,
    inbox: InboxPage,
    usernames: UsernamePage,
    compare: ComparePage,
    look: LookPage,
    themes: ThemesPage,
    icons: IconsPage,
    list: ListPage,
    navigation: NavigationPage,
    add: AddMenuPage,
    editor: EditorPage,
    entry: EntryPage,
    codes: CodesSettingsPage,
    generator: GeneratorSettingsPage,
    health: HealthSettingsPage,
    shortcuts: ShortcutsPage,
    file: FilePage,
    about: AboutPage,
    browser: BrowserPage,
    'desktop-link': DesktopLinkPage,
    ssh: SshPage,
    updates: UpdatesPage,
    desktop: DesktopPage,
  };

  let query = $state('');
  const pageId = $derived(ui.section.split('/')[1] ?? null);
  const page = $derived(pageId ? PAGES.find((p) => p.id === pageId) : null);
  const Current = $derived(pageId ? COMPONENTS[pageId] : null);
  const backTo = $derived(page?.parent ? PAGES.find((p) => p.id === page!.parent) : null);
  const isDesktop = host().kind === 'desktop';

  function shown(info: PageInfo) {
    if (info.desktop && !isDesktop) return false;
    if (info.extension && host().kind !== 'extension') return false;
    if (info.parent) return false;
    if (info.needsNetwork && !host().network) return false;
    return true;
  }

  const found = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    const words = q.split(/\s+/);
    return SEARCH_INDEX.filter((item) => {
      if (item.desktop && !isDesktop) return false;
      const text = `${item.title} ${item.where} ${item.words ?? ''}`.toLowerCase();
      return words.every((w) => text.includes(w));
    }).sort((a, b) => rank(a.title, q) - rank(b.title, q));
  });

  function rank(title: string, q: string) {
    const t = title.toLowerCase();
    return t.startsWith(q) ? 0 : t.includes(q) ? 1 : 2;
  }

  // On a wide window the first page is already open beside the list.
  $effect(() => {
    if (!ui.narrow && ui.section === 'settings') ui.section = 'settings/security';
  });

  let pane = $state<HTMLElement>();
  $effect(() => {
    void ui.section;
    pane?.scrollTo(0, 0);
  });

  function go(id: string) {
    ui.section = `settings/${id}`;
  }

  function jump(route: string, title: string) {
    go(route);
    query = '';
    setTimeout(() => highlight.point(title), 60);
  }

  function tileStyle(id: string) {
    const style = prefs.v.settings_icons;
    if (style === 'off') return null;
    if (style === 'coloured') return { className: 'tile-glyph', css: `background:${ROUTE_COLOURS[id] ?? 'var(--text3)'}`, size: 16 };
    if (style === 'mono') return { className: 'tile-glyph mono', css: '', size: 16 };
    return { className: 'tile-glyph bare', css: `color:${ROUTE_COLOURS[id] ?? 'var(--text2)'}`, size: 18 };
  }
</script>

{#snippet tile(id: string, icon: IconName)}
  {@const look = tileStyle(id)}
  {#if look}<span class={look.className} style={look.css}><Icon name={icon} size={look.size} /></span>{/if}
{/snippet}

{#if !ui.narrow || !pageId}
  <div class="list-pane" style={ui.narrow ? 'flex:1' : 'width:250px;background:var(--s1)'}>
    {#if !ui.narrow}
      <div class="top" style="flex:none" data-tauri-drag-region></div>
      <div style="padding:0 8px 4px">
        <button type="button" class="side-item" onclick={() => ui.go(prefs.v.last_tab || 'all')}>
          <span class="ico"><Icon name="arrow_back" size={18} /></span>
          <span class="grow ell">Back to the vault</span>
        </button>
      </div>
    {/if}
    <div class="list-head" style="padding-top:4px">
      <Field bind:value={query} placeholder="Search settings" />
    </div>
    <div class="list-body scroll">
      {#if query.trim()}
        {#if found.length === 0}
          <p class="empty">No settings match.</p>
        {/if}
        {#each found as item}
          <button type="button" class="row" style="min-height:40px" onclick={() => jump(item.route, item.title)}>
            <span class="grow">
              <span class="ell" style="display:block">{item.title}</span>
              <span class="dim small ell" style="display:block">{item.where}</span>
            </span>
          </button>
        {/each}
      {:else}
        {#each GROUPS as group}
          {@const pages = PAGES.filter((p) => p.group === group && shown(p))}
          {#if pages.length > 0}
            <div class="side-title" style="padding-left:8px">{group}</div>
            {#each pages as info (info.id)}
              <button type="button" class="side-item" style="height:32px" aria-current={pageId === info.id || page?.parent === info.id} onclick={() => go(info.id)}>
                {@render tile(info.id, info.icon)}
                <span class="grow ell">{info.title}</span>
              </button>
            {/each}
          {/if}
        {/each}
      {/if}
    </div>
  </div>
{/if}

{#if Current && page && (!ui.narrow || pageId)}
  <div bind:this={pane} class="settings-pane scroll">
    <div class="settings-inner">
      <div class="gap" style="padding-bottom:14px">
        {#if ui.narrow || backTo}
          <IconBtn icon="arrow_back" label="Back" onclick={() => (backTo ? go(backTo.id) : (ui.section = 'settings'))} />
        {/if}
        <h1 class="page-title">{page.title}</h1>
      </div>
      {#key pageId}
        <Current />
      {/key}
    </div>
  </div>
{/if}
