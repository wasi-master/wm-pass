<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { host } from '../engine/host';
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

  /** Runs a change to all of them, and says so if the vault refuses it. */
  async function attempt(work: () => Promise<unknown>) {
    try {
      await work();
      return true;
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be done.');
      return false;
    }
  }

  // A second click while the first change is still running would apply it twice.
  let busy = false;
  function once(work: () => Promise<void>) {
    return async () => {
      if (busy) return;
      busy = true;
      try {
        await work();
      } finally {
        busy = false;
      }
    };
  }

  const trash = once(async () => {
    const ids = [...ui.multi];
    const inTrash = ui.section === 'trash';
    if (!(await attempt(() => ui.bulk(inTrash ? 'restore' : 'trash', {}, ids)))) return;
    done();
    if (ui.selected && ids.includes(ui.selected)) ui.deselect(ui.selected);
    if (inTrash) {
      overlays.toast(ids.length === 1 ? '1 item restored.' : `${ids.length} items restored.`);
      return;
    }
    overlays.toast(ids.length === 1 ? '1 item moved to the trash.' : `${ids.length} items moved to the trash.`, {
      action: 'Undo',
      run: () => void attempt(() => ui.bulk('restore', {}, ids)),
    });
  });

  // In the trash, as one item's page offers it: gone for good, after asking.
  const purge = once(async () => {
    const ids = [...ui.multi];
    const yes = await overlays.ask({
      title: ids.length === 1 ? 'Delete 1 item for good?' : `Delete ${ids.length} items for good?`,
      body: 'Their passwords, history and attachments go with them. A backup made before now would still hold them.',
      action: 'Delete for good',
      danger: true,
    });
    if (!yes) return;
    if (!(await attempt(() => ui.bulk('purge', {}, ids)))) return;
    done();
    if (ui.selected && ids.includes(ui.selected)) ui.deselect(ui.selected);
    overlays.toast(ids.length === 1 ? '1 item deleted for good.' : `${ids.length} items deleted for good.`);
  });

  // The shelf's own items are not among the vault's entries, so there they are all hidden.
  const allHidden = $derived(ui.section === 'shelf' || (chosen.length > 0 && chosen.every((e) => e.hidden)));

  const hide = once(async () => {
    const wasHidden = allHidden;
    if (!(await attempt(() => ui.bulk('hide', { flag: !wasHidden })))) return;
    done();
    overlays.toast(wasHidden ? 'Taken off the hidden shelf.' : 'Moved to the hidden shelf.');
  });

  // What the tag clicks so far come to, until the vault has caught up with them: a second quick
  // click builds on the first rather than on tags that have not changed yet, and runs after it.
  let pendingTags = $state<string[] | null>(null);
  let tagging = Promise.resolve();
  let queued = 0;

  function retag(next: string[]) {
    const ids = [...ui.multi];
    const from = pendingTags ?? sharedTags;
    pendingTags = next;
    queued += 1;
    tagging = tagging.then(async () => {
      await attempt(() =>
        pass.batch(async () => {
          for (const tag of next.filter((t) => !from.includes(t))) await ui.bulk('tag_add', { tag }, ids);
          for (const tag of from.filter((t) => !next.includes(t))) await ui.bulk('tag_remove', { tag }, ids);
        }),
      );
      queued -= 1;
      if (queued === 0) pendingTags = null;
    });
  }

  /** Each one lists the others under Linked, keeping the links it had. */
  const linkTogether = once(async () => {
    const ids = [...ui.multi];
    const before = new Map<string, string[]>();
    try {
      for (const id of ids) before.set(id, (await pass.detail(id)).links);
      await pass.batch(async () => {
        for (const [id, have] of before) {
          const next = [...new Set([...have, ...ids.filter((other) => other !== id)])];
          if (next.length !== have.length) await pass.change('entry.common', { id, links: next });
        }
      });
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'They could not be linked.');
      return;
    }
    done();
    overlays.toast(ids.length === 2 ? 'Linked the two.' : `Linked all ${ids.length}.`, {
      action: 'Undo',
      run: () =>
        void attempt(() =>
          pass.batch(async () => {
            for (const [id, have] of before) await pass.change('entry.common', { id, links: have });
          }),
        ),
    });
  });
</script>

<div class="detail-body" style="padding-top:40px">
  <div class="detail-inner stack">
    <h2 class="page-title">{ui.multi.length} selected</h2>
    {#if ui.section === 'trash'}
      <div class="gap wrap">
        <Btn label="Restore" icon="restore_from_trash" onclick={trash} />
        <Btn label="Delete for good" icon="delete" variant="danger" onclick={purge} />
      </div>
    {:else}
      <div class="gap wrap">
        <Btn label="Move to a folder" icon="folder" onclick={() => (picker = 'folder')} />
        <Btn label="Color" icon="palette" onclick={() => (picker = 'colour')} />
        <Btn label="Tags" icon="sell" onclick={() => (picker = 'tags')} />
        <Btn label="Link together" icon="link" title="Each one lists the others under Linked" onclick={linkTogether} />
      </div>
      <div class="gap wrap">
        <Btn label={allFav ? 'Remove from favorites' : 'Add to favorites'} icon="star" onclick={() => attempt(() => ui.bulk('favourite', { flag: !allFav }))} />
        <Btn label={allPin ? 'Unpin' : 'Pin to the top'} icon="keep" onclick={() => attempt(() => ui.bulk('pin', { flag: !allPin }))} />
        {#if allHidden}
          <Btn label="Remove from hidden shelf" icon="visibility_lock" onclick={hide} />
        {:else}
          <Btn label="Put on the hidden shelf" icon="visibility_lock" title="Out of lists and autofill until you open the shelf" onclick={hide} />
        {/if}
        <Btn label="Move to trash" icon="delete" variant="danger" onclick={trash} />
      </div>
    {/if}
    <p class="dim small">{host().os === 'mac' ? '⌘' : 'Ctrl'}-click rows to add or remove them. Shift-click, or Shift and the arrow keys, chooses a range.</p>
  </div>
</div>

{#if picker === 'folder'}
  <Pickers kind="folder" current={sharedFolder} onclose={() => (picker = null)} onpick={(folder) => void attempt(() => ui.bulk('folder', { folder })).then((ok) => ok && done())} />
{:else if picker === 'colour'}
  <Pickers kind="colour" current={sharedColour} onclose={() => (picker = null)} onpick={(color) => void attempt(() => ui.bulk('color', { color })).then((ok) => ok && done())} />
{:else if picker === 'tags'}
  <Pickers kind="tags" current={pendingTags ?? sharedTags} onclose={() => (picker = null)} onpick={retag} />
{/if}
