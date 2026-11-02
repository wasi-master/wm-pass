<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import { pass } from '../engine/pass.svelte';
  import { copyPlainLine } from '../settings/clipboard';

  // Hands one item to a device in the same room. The other side shows a code holding a one-use
  // key, this side seals the item to that key, and the result opens only there, once. A desktop
  // has no camera to rely on, so the codes are text to paste, and a phone can scan them from here.
  let { id, onclose }: { id: string; onclose: () => void } = $props();
  let code = $state('');
  let sealed = $state<string | null>(null);
  let problem = $state<string | null>(null);

  async function seal() {
    problem = null;
    try {
      const reply = await pass.net<{ text: string }>('share.seal', false, { id, text: code.trim() });
      sealed = reply.json.text;
    } catch (failure: any) {
      problem = failure.message;
    }
  }
</script>

<Dialog title="Send to a device" {onclose}>
  <div class="stack">
    {#if sealed}
      <p class="dim">Paste this on the other device. It can be opened only there, once.</p>
      <Field value={sealed} multiline rows={6} mono label="Sealed item" />
      <div><Btn label="Copy" icon="content_copy" onclick={() => copyPlainLine('Sealed item', sealed!)} /></div>
    {:else}
      <p class="dim">On the other device, choose Sync and accounts, then Receive an item. It shows a code. Paste the text of that code here. Nothing travels over a network.</p>
      <Field bind:value={code} multiline rows={4} mono placeholder="The other device's code" />
      {#if problem}<p class="danger-text">{problem}</p>{/if}
    {/if}
  </div>
  {#snippet foot()}
    <Btn label="Close" onclick={onclose} />
    {#if !sealed}<Btn label="Seal it" variant="primary" disabled={!code.trim()} onclick={seal} />{/if}
  {/snippet}
</Dialog>
