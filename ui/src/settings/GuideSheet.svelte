<script module lang="ts">
  /** The device picked last, so the next guide opens on the same one, as the website does. */
  let lastDevice: string | null = null;
</script>

<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Icon from '../parts/Icon.svelte';
  import Tile from '../parts/Tile.svelte';
  import { host } from '../engine/host';
  import { GUIDES, GUIDE_LIST, loadGuide, publisherSite, type GuidePage, type Run } from './guides';

  // Import, How to export from your old app: the list of apps, then one app's steps in the same
  // sheet, with a tab for each device it runs on.
  let { onclose }: { onclose: () => void } = $props();

  let reading = $state.raw<(typeof GUIDE_LIST)[number] | null>(null);
  let page = $state.raw<GuidePage | null | undefined>(undefined);
  let device = $state<string | null>(null);

  /** The tab for this computer, when the guide has one. */
  function here(tabs: string[]) {
    const os = host().os;
    const mine = host().kind === 'extension' ? ['Browser extension', 'Web', 'Computer'] : [];
    const system = os === 'mac' ? 'Mac' : os === 'windows' ? 'Windows' : os === 'linux' ? 'Linux' : null;
    const order = [lastDevice, ...mine, system, 'Computer', 'Web', 'Browser extension'];
    return order.find((t) => t && tabs.includes(t)) ?? tabs[0] ?? null;
  }

  async function open(guide: (typeof GUIDE_LIST)[number]) {
    reading = guide;
    page = undefined;
    const loaded = await loadGuide(guide.page);
    if (reading !== guide) return;
    page = loaded;
    device = loaded ? here(loaded.tabs) : null;
  }

  function pick(tab: string) {
    device = tab;
    lastDevice = tab;
  }
</script>

{#snippet runs(list: Run[])}
  {#each list as run, n (n)}
    {#if run.t === 'bold'}<strong>{run.s}</strong>{:else if run.t === 'code'}<code class="guide-code">{run.s}</code>{:else if run.t === 'link'}<a
        href={run.url}
        onclick={(e) => {
          e.preventDefault();
          void host().openUrl(run.url);
        }}>{run.s}</a
      >{:else}{run.s}{/if}
  {/each}
{/snippet}

{#if reading}
  <Dialog title={page?.title ?? reading.name} wide onclose={() => (reading = null)}>
    <div class="guide-body scroll">
      {#if page === undefined}
        <p class="dim">Opening…</p>
      {:else if page === null}
        <p class="dim">This guide is not in this version of the app. The website has it.</p>
      {:else}
        {@const tabsAt = page.blocks.findIndex((b) => b.tab !== null)}
        {#each page.blocks as block, n (n)}
          {#if n === tabsAt}
            <div class="guide-tabs" role="group" aria-label="Device">
              {#each page.tabs as tab (tab)}
                <button type="button" class="chip" aria-pressed={device === tab} onclick={() => pick(tab)}>{tab}</button>
              {/each}
            </div>
          {/if}
          {#if block.tab === null || block.tab === device}
            {#if block.t === 'heading'}
              <h3 class="guide-heading">{block.text}</h3>
            {:else if block.t === 'paragraph'}
              <p class="dim">{@render runs(block.runs)}</p>
            {:else if block.t === 'bullet'}
              <p class="guide-bullet"><span class="dimmer">•</span><span>{@render runs(block.runs)}</span></p>
            {:else if block.t === 'step'}
              <p class="guide-step"><span class="guide-number">{block.number}</span><span>{@render runs(block.runs)}</span></p>
            {:else if block.t === 'note'}
              <div class="guide-note">
                {#if block.title}<strong style="display:block">{block.title}</strong>{/if}
                <span class="dim">{@render runs(block.runs)}</span>
              </div>
            {:else if block.t === 'picture'}
              <button type="button" class="guide-picture" title={block.caption} onclick={() => void host().openUrl(block.src)}>
                <span class="dimmer" style="display:flex"><Icon name="image" size={18} /></span>
                <span style="color:var(--primary)">Picture on {publisherSite(block.publisher)}</span>
              </button>
            {/if}
          {/if}
        {/each}
      {/if}
    </div>
    {#snippet foot()}
      <Btn label="Open on the website" icon="open_in_new" onclick={() => reading && void host().openUrl(`${GUIDES}${reading.page}/`)} />
      <Btn label="Back" onclick={() => (reading = null)} />
    {/snippet}
  </Dialog>
{:else}
  <Dialog title="Export from your old app" {onclose}>
    <div class="set-group" style="margin-bottom:0;max-height:60vh;overflow-y:auto">
      <div class="body">
        {#each GUIDE_LIST as guide (guide.page)}
          <button type="button" class="set-row" onclick={() => void open(guide)}>
            <Tile title={guide.name} icon={guide.brand ? `brand:${guide.brand}` : null} size={28} />
            <span class="text"><span class="ell" style="display:block;font-weight:500">{guide.name}</span></span>
            <span class="dimmer" style="display:flex"><Icon name="chevron_right" size={16} /></span>
          </button>
        {/each}
      </div>
    </div>
  </Dialog>
{/if}

<style>
  .guide-body {
    max-height: 62vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-right: 4px;
  }
  .guide-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 0;
  }
  .guide-heading {
    font-size: var(--fs-s);
    font-weight: 600;
    color: var(--text2);
    margin: 10px 0 0;
  }
  .guide-bullet,
  .guide-step {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .guide-number {
    flex: none;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--s3);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: var(--fs-s);
    font-weight: 600;
  }
  .guide-note {
    background: var(--s2);
    border-radius: var(--r-m);
    padding: 10px 12px;
  }
  .guide-picture {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-left: 32px;
    padding: 4px 6px;
    border-radius: var(--r-s);
    text-align: left;
  }
  .guide-picture:hover {
    background: var(--hover);
  }
  .guide-code {
    font-family: var(--mono, monospace);
    background: var(--s3);
    border-radius: 4px;
    padding: 0 4px;
  }
</style>
