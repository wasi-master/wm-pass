<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Qr from '../parts/Qr.svelte';
  import { pass } from '../engine/pass.svelte';
  import { copyPlainLine } from '../settings/clipboard';

  // Hands one item to a device in the same room. The other side shows a code holding a one-use
  // key, this side seals the item to that key, and the result opens only there, once. A desktop
  // has no camera to rely on, so the other side's code is pasted here; the sealed answer shows as
  // a QR code a phone can scan, and as text to paste on another computer.
  let { id, onclose }: { id: string; onclose: () => void } = $props();
  let code = $state('');
  let sealed = $state<string | null>(null);
  let problem = $state<string | null>(null);
  let sealing = $state(false);

  async function seal() {
    if (sealing) return;
    sealing = true;
    problem = null;
    try {
      const reply = await pass.net<{ text: string }>('share.seal', false, { id, text: code.trim() });
      sealed = reply.json.text;
    } catch (failure: any) {
      problem = failure?.message ?? 'The item could not be sealed.';
    } finally {
      sealing = false;
    }
  }
</script>

<Dialog title="Send to a device" {onclose}>
  <div class="stack">
    {#if sealed}
      <p class="dim">Scan this with the other device, or paste the text below there. It can be opened only there, once.</p>
      <div style="display:flex;justify-content:center"><Qr text={sealed} /></div>
      <Field value={sealed} multiline rows={6} mono label="Sealed item" />
      <div><Btn label="Copy" icon="content_copy" onclick={() => copyPlainLine('Sealed item', sealed!)} /></div>
    {:else}
      <p class="dim">On the other device, choose Sync and accounts, then Receive an item. It shows a code. Paste the text of that code here. Nothing travels over a network.</p>
      <Field bind:value={code} multiline rows={4} mono placeholder="The other device's code" />
      {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
    {/if}
  </div>
  {#snippet foot()}
    <Btn label="Close" onclick={onclose} />
    {#if !sealed}<Btn label="Seal it" variant="primary" disabled={!code.trim() || sealing} onclick={seal} />{/if}
  {/snippet}
</Dialog>
