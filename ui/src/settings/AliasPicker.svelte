<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Row from '../parts/Row.svelte';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import { ALIAS_SERVICES, serviceKey } from './aliasServices';
  import { online } from './net.svelte';

  // From the editor: a new alias for the item being edited, with its name as the note.
  let { hint, onclose, onpick }: { hint: string; onclose: () => void; onpick: (address: string) => void } = $props();
  const ready = $derived(ALIAS_SERVICES.filter((s) => pass.entries.some((e) => e.kind === 'api' && e.title === s.keyTitle)));
  let busy = $state<string | null>(null);
  let problem = $state<string | null>(null);

  async function make(id: string, title: string) {
    // A second click while the first alias is being made would make two.
    if (busy) return;
    busy = id;
    problem = null;
    let secret: Uint8Array | null = null;
    try {
      secret = await serviceKey(title);
      const made = await pass.engine<{ address: string }>('alias.create', { provider: id, note: hint }, { secret, online: online() });
      onpick(made.json.address);
    } catch (failure: any) {
      problem = failure?.message ?? 'The alias could not be made.';
    } finally {
      secret?.fill(0);
      busy = null;
    }
  }
</script>

<Dialog title="Use an alias" {onclose}>
  {#if ready.length === 0}
    <p class="dim">Set up a forwarding service first. Then each new login can have an address of its own.</p>
  {:else}
    <div class="stack">
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        {#each ready as service (service.id)}
          <Row title={busy === service.id ? 'Making it…' : service.name} sub="A new address that forwards to you" icon="alternate_email" onclick={() => make(service.id, service.keyTitle)} />
        {/each}
      </div>
      {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
    </div>
  {/if}
  {#snippet foot()}
    <Btn label="Close" onclick={onclose} />
    {#if ready.length === 0}<Btn label="Set one up" variant="primary" onclick={() => { onclose(); ui.go('settings/aliases'); }} />{/if}
  {/snippet}
</Dialog>
