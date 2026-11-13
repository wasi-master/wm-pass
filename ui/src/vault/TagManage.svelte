<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import { GLYPH_CHOICES, TAG_GLYPHS, glyphName, tagAutomatic } from '../engine/glyphs';
  import { tags } from '../engine/tags.svelte';
  import { nameText } from '../engine/text';
  import { overlays } from '../parts/floating.svelte';
  import { ui } from './ui.svelte';

  // Renames a tag, deletes it, or gives it a symbol, on every item that has it.
  let { tag, onclose }: { tag: string; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(tag);
  let busy = $state(false);
  let error = $state<string | null>(null);
  const holders = $derived(tags.holders(tag).length);
  const renamed = $derived(nameText(name));
  const picked = $derived(tags.icons[tag] ?? null);

  async function run(work: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await work();
      onclose();
    } catch (failure: any) {
      error = failure?.message ?? 'That could not be done.';
    } finally {
      busy = false;
    }
  }

  function rename() {
    if (!renamed || renamed === tag || busy) return;
    void run(async () => {
      await tags.rename(tag, renamed);
      ui.tagChanged = { from: tag, to: renamed };
      // The list showing this tag follows it to its new name.
      if (ui.section === `tag:${tag}`) ui.go(`tag:${renamed}`);
    });
  }

  async function remove() {
    const yes = await overlays.ask({ title: `Delete “${tag}”?`, body: 'It comes off every item that has it. The items stay.', action: 'Delete tag', danger: true });
    if (yes)
      void run(async () => {
        await tags.remove(tag);
        ui.tagChanged = { from: tag, to: null };
        if (ui.section === `tag:${tag}`) ui.go('all');
      });
  }
</script>

<Dialog title={tag} {onclose}>
  <div class="stack">
    <p class="dim">{holders === 1 ? 'On 1 item.' : `On ${holders} items.`}</p>
    <div class="gap">
      <div class="grow"><Field bind:value={name} label="Name" placeholder="Name" onenter={rename} /></div>
      <Btn label="Rename" disabled={!renamed || renamed === tag || busy} onclick={rename} />
    </div>
    <div>
      <p class="dim small" style="padding-bottom:6px" id="tag-icon-label">Icon</p>
      <div class="gap wrap" style="gap:6px" role="group" aria-labelledby="tag-icon-label">
        {#each [null, ...TAG_GLYPHS] as id (id ?? 'automatic')}
          {@const glyph = id === null ? tagAutomatic(tag) : GLYPH_CHOICES[id]}
          {@const label = id === null ? 'Automatic' : glyphName(id)}
          <button
            type="button"
            class="icon-btn"
            class:on={picked === id}
            title={label}
            aria-label={label}
            aria-pressed={picked === id}
            style="width:36px;height:36px;border-radius:50%;background:var(--s1)"
            onclick={() => void tags.setIcon(tag, id).catch((failure: any) => (error = failure?.message ?? 'The icon could not be changed.'))}
          ><Icon name={glyph} /></button>
        {/each}
      </div>
    </div>
    {#if error}<p class="danger-text small" role="alert">{error}</p>{/if}
  </div>
  {#snippet foot()}
    <Btn label="Delete from every item" icon="delete" variant="danger" disabled={busy} onclick={remove} />
    <span class="grow"></span>
    <Btn label="Done" onclick={onclose} />
  {/snippet}
</Dialog>
