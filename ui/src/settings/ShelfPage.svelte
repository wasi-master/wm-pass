<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import EntryLines from './EntryLines.svelte';
  import type { Summary } from '../engine/types';

  let open = $state(false);
  let items = $state.raw<Summary[]>([]);
  let password = $state('');
  let error = $state<string | null>(null);

  $effect(() => {
    void pass.revision;
    void pass.info().then(async (info) => {
      open = info.shelf_open;
      items = open ? await pass.search('hidden') : [];
    });
  });

  async function openIt() {
    const secret = bytesOf(password);
    try {
      await pass.change('shelf.open', {}, { secret });
      password = '';
      error = null;
    } catch (failure: any) {
      error = failure.message;
    } finally {
      secret.fill(0);
    }
  }
</script>

{#if !open}
  <div class="stack" style="max-width:420px">
    <p class="dim">Entries on the shelf stay out of every list, search and autofill until you open it with your password. It closes again when the vault locks.</p>
    <PasswordField bind:value={password} placeholder="Master password" error={!!error} onenter={openIt} />
    {#if error}<p class="danger-text small">{error}</p>{/if}
    <div><Btn label="Open the shelf" variant="primary" disabled={!password} onclick={openIt} /></div>
  </div>
{:else}
  <div class="stack">
    {#if items.length === 0}<p class="dim">The shelf is empty.</p>{:else}<EntryLines {items} onpick={(e) => { ui.go('shelf'); ui.open(e.id); }} />{/if}
    <div><Btn label="Close the shelf" icon="visibility_lock" onclick={() => pass.change('shelf.close')} /></div>
  </div>
{/if}
