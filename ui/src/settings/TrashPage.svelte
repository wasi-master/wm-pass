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

  $effect(() => {
    void pass.revision;
    void pass.trashed().then((list) => (items = list));
    void pass.info().then((info) => (days = info.trash_days));
  });

  const options = $derived([...new Set([7, 30, 90, 365, days])].sort((a, b) => a - b).map((d) => [d, `${d} days`] as [number, string]));

  async function empty() {
    const yes = await overlays.ask({ title: 'Empty the trash?', body: `These ${items.length} items and their attachments will be deleted for good.`, action: 'Delete them for good', danger: true });
    if (yes) await pass.change('entries.bulk', { op: 'purge', ids: items.map((e) => e.id) });
  }
</script>

<Group>
  <Choice title="Keep deleted items for" help="action.trash_keep" icon="timer" value={days} {options} onchange={(value) => { days = value; void pass.change('vault.prefs', { trash_days: value }); }} />
</Group>

{#if items.length === 0}
  <p class="dim">Nothing in the trash.</p>
{:else}
  <div class="stack">
    <EntryLines {items} onpick={(e) => { ui.go('trash'); ui.open(e.id); }} />
    <div><Btn label="Empty the trash" icon="delete" variant="danger" onclick={empty} /></div>
  </div>
{/if}
