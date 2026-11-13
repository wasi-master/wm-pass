<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { pass } from '../engine/pass.svelte';
  import type { Summary } from '../engine/types';
  import { ui } from '../vault/ui.svelte';
  import Choice from './Choice.svelte';
  import EntryLines from './EntryLines.svelte';

  let items = $state.raw<Summary[]>([]);
  let days = $state(30);

  // Every change asks again, and an older answer must not land after a newer one.
  let asked = 0;
  $effect(() => {
    void pass.revision;
    const turn = ++asked;
    // A vault that locks meanwhile answers with an error, and the page is leaving anyway.
    void pass
      .trashed()
      .then((list) => turn === asked && (items = list))
      .catch(() => undefined);
    void pass
      .info()
      .then((info) => turn === asked && (days = info.trash_days))
      .catch(() => undefined);
  });

  // Another device or the terminal app may have chosen 0, which keeps them until the trash is emptied.
  const dayText = (d: number) => (d === 0 ? 'Until emptied' : d === 1 ? '1 day' : `${d} days`);
  const options = $derived([...new Set([7, 30, 90, 365, days])].sort((a, b) => (a || Infinity) - (b || Infinity)).map((d) => [d, dayText(d)] as [number, string]));

  async function keepFor(value: number) {
    const before = days;
    days = value;
    try {
      await pass.change('vault.prefs', { trash_days: value });
    } catch (failure: any) {
      days = before;
      overlays.toast(failure?.message ?? 'That could not be changed.');
    }
  }

  let emptying = $state(false);

  async function empty() {
    if (emptying) return;
    const yes = await overlays.ask({ title: 'Empty the trash?', body: items.length === 1 ? 'This item and its attachments will be deleted for good.' : `These ${items.length} items and their attachments will be deleted for good.`, action: items.length === 1 ? 'Delete it for good' : 'Delete them for good', danger: true });
    if (!yes || emptying) return;
    emptying = true;
    try {
      await pass.change('entries.bulk', { op: 'purge', ids: items.map((e) => e.id) });
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The trash could not be emptied.');
    } finally {
      emptying = false;
    }
  }
</script>

<Group>
  <Choice title="Keep deleted items for" help="action.trash_keep" icon="timer" value={days} {options} onchange={(value) => void keepFor(value)} />
</Group>

{#if items.length === 0}
  <p class="dim">Nothing in the trash.</p>
{:else}
  <div class="stack">
    <EntryLines {items} onpick={(e) => { ui.go('trash'); ui.open(e.id); }} />
    <div><Btn label={emptying ? 'Emptying…' : 'Empty the trash'} icon="delete" variant="danger" disabled={emptying} onclick={empty} /></div>
  </div>
{/if}
