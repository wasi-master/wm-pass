<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { pass } from '../engine/pass.svelte';
  import Pickers from './Pickers.svelte';
  import { ui } from './ui.svelte';

  // What can be done to several items at once, as on the phone's selection bar.
  let picker = $state<null | 'folder' | 'colour' | 'tags'>(null);

  const chosen = $derived(pass.entries.filter((e) => ui.multi.includes(e.id)));
  const allFav = $derived(chosen.length > 0 && chosen.every((e) => e.favourite));
  const allPin = $derived(chosen.length > 0 && chosen.every((e) => e.pinned));
  /** The folder every one is in, or a value no folder has when they differ, so none shows ticked. */
  const sharedFolder = $derived.by(() => {
    const folders = new Set(chosen.map((e) => e.folder ?? null));
    return folders.size === 1 ? [...folders][0] : '~mixed';
  });
  const sharedColour = $derived.by(() => {
    const colours = new Set(chosen.map((e) => e.color ?? null));
    return colours.size === 1 ? [...colours][0] : '~mixed';
  });
  /** The tags every one has: those show as on, and a click takes them off all. */
  const sharedTags = $derived(chosen.length ? chosen[0].tags.filter((t) => chosen.every((e) => e.tags.includes(t))) : []);

  function done() {
    ui.multi = [];
    picker = null;
  }

  async function trash() {
    const ids = [...ui.multi];
    const inTrash = ui.section === 'trash';
    await ui.bulk(inTrash ? 'restore' : 'trash', {}, ids);
    done();
    if (inTrash) return;
    overlays.toast(ids.length === 1 ? '1 item moved to the trash.' : `${ids.length} items moved to the trash.`, {
      action: 'Undo',
      run: () => void ui.bulk('restore', {}, ids),
    });
  }

  // The shelf's own items are not among the vault's entries, so there they are all hidden.
  const allHidden = $derived(ui.section === 'shelf' || (chosen.length > 0 && chosen.every((e) => e.hidden)));

  async function hide() {
    await ui.bulk('hide', { flag: !allHidden });
    done();
    overlays.toast(allHidden ? 'Taken off the hidden shelf.' : 'Moved to the hidden shelf.');
  }

  async function retag(next: string[]) {
    const ids = [...ui.multi];
    for (const tag of next.filter((t) => !sharedTags.includes(t))) await ui.bulk('tag_add', { tag }, ids);
    for (const tag of sharedTags.filter((t) => !next.includes(t))) await ui.bulk('tag_remove', { tag }, ids);
  }

  /** Each one lists the others under Linked, keeping the links it had. */
  async function linkTogether() {
    const ids = [...ui.multi];
    const before = new Map<string, string[]>();
    try {
      for (const id of ids) before.set(id, (await pass.detail(id)).links);
      for (const [id, have] of before) {
        const next = [...new Set([...have, ...ids.filter((other) => other !== id)])];
        if (next.length !== have.length) await pass.change('entry.common', { id, links: next });
      }
    } catch (failure: any) {
      overlays.toast(failure.message ?? 'They could not be linked.');
      return;
    }
    done();
    overlays.toast(ids.length === 2 ? 'Linked the two.' : `Linked all ${ids.length}.`, {
      action: 'Undo',
      run: () => {
        void (async () => {
          for (const [id, have] of before) await pass.change('entry.common', { id, links: have });
        })();
      },
    });
  }
</script>

<div class="detail-body" style="padding-top:40px">
  <div class="detail-inner stack">
    <h2 class="page-title">{ui.multi.length} selected</h2>
    {#if ui.section === 'trash'}
      <div class="gap wrap">
        <Btn label="Restore" icon="restore_from_trash" onclick={trash} />
      </div>
    {:else}
      <div class="gap wrap">
        <Btn label="Move to a folder" icon="folder" onclick={() => (picker = 'folder')} />
        <Btn label="Color" icon="palette" onclick={() => (picker = 'colour')} />
        <Btn label="Tags" icon="sell" onclick={() => (picker = 'tags')} />
        <Btn label="Link together" icon="link" title="Each one lists the others under Linked" onclick={linkTogether} />
      </div>
      <div class="gap wrap">
        <Btn label={allFav ? 'Remove from favorites' : 'Add to favorites'} icon="star" onclick={() => ui.bulk('favourite', { flag: !allFav })} />
        <Btn label={allPin ? 'Unpin' : 'Pin to the top'} icon="keep" onclick={() => ui.bulk('pin', { flag: !allPin })} />
        {#if allHidden}
          <Btn label="Remove from hidden shelf" icon="visibility_lock" onclick={hide} />
        {:else}
          <Btn label="Put on the hidden shelf" icon="visibility_lock" title="Out of lists and autofill until you open the shelf" onclick={hide} />
        {/if}
        <Btn label="Move to trash" icon="delete" variant="danger" onclick={trash} />
      </div>
    {/if}
    <p class="dim small">Hold the modifier key and click rows to add or remove them. Shift-click chooses a range.</p>
  </div>
</div>

{#if picker === 'folder'}
  <Pickers kind="folder" current={sharedFolder} onclose={() => (picker = null)} onpick={(folder) => void ui.bulk('folder', { folder }).then(done)} />
{:else if picker === 'colour'}
  <Pickers kind="colour" current={sharedColour} onclose={() => (picker = null)} onpick={(color) => void ui.bulk('color', { color }).then(done)} />
{:else if picker === 'tags'}
  <Pickers kind="tags" current={sharedTags} onclose={() => (picker = null)} onpick={(next) => void retag(next)} />
{/if}
