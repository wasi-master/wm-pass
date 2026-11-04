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
  let checking = $state(false);
  /** Set when one of its items is opened, which keeps the shelf open. */
  let keep = false;

  // Leaving the page puts the shelf away again; opening one of its items does not.
  $effect(() => () => {
    if (keep) return;
    void pass
      .info()
      .then((info) => (info.shelf_open ? pass.change('shelf.close') : null))
      .catch(() => undefined);
  });

  $effect(() => {
    void pass.revision;
    void pass.info().then(async (info) => {
      open = info.shelf_open;
      items = open ? await pass.search('hidden') : [];
    });
  });

  async function openIt() {
    if (checking || !password) return;
    checking = true;
    const secret = bytesOf(password);
    try {
      await pass.change('shelf.open', {}, { secret });
      password = '';
      error = null;
    } catch (failure: any) {
      error = failure.message;
    } finally {
      secret.fill(0);
      checking = false;
    }
  }
</script>

{#if !open}
  <div class="stack" style="max-width:420px">
    <p class="dim">Entries on the shelf stay out of every list, search and autofill until you open it with your password. It closes again when you leave this page or the vault locks.</p>
    <PasswordField bind:value={password} placeholder="Master password" error={!!error} onenter={openIt} />
    {#if error}<p class="danger-text small">{error}</p>{/if}
    <div><Btn label={checking ? 'Checking the password…' : 'Open the shelf'} variant="primary" disabled={!password || checking} onclick={openIt} /></div>
  </div>
{:else}
  <div class="stack">
    {#if items.length === 0}<p class="dim">The shelf is empty.</p>{:else}<EntryLines {items} onpick={(e) => { keep = true; ui.go('shelf'); ui.open(e.id); }} />{/if}
    <div><Btn label="Close the shelf" icon="visibility_lock" onclick={() => pass.change('shelf.close')} /></div>
  </div>
{/if}
