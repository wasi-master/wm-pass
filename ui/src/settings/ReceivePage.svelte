<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Qr from '../parts/Qr.svelte';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import { copyPlainLine } from './clipboard';

  // The other half of a one-time handover: show a code, then read the answer.
  let token = $state('');
  let offer = $state('');
  let answer = $state('');
  let problem = $state<string | null>(null);
  let tries = $state(0);

  $effect(() => {
    void tries;
    pass
      .engine<{ token: string; text: string }>('share.offer', {}, {})
      .then((reply) => {
        token = reply.json.token;
        offer = reply.json.text;
      })
      .catch((failure) => (problem = failure?.message ?? 'A code could not be made.'));
  });

  // The code works once: a second click while the first is on its way would spend it and fail.
  let opening = $state(false);
  async function open() {
    if (opening) return;
    opening = true;
    problem = null;
    try {
      const reply = await pass.net<{ id: string }>('share.open', false, { token, text: answer.trim() });
      await pass.reload();
      await pass.flush();
      ui.go('all');
      ui.open(reply.json.id);
    } catch (failure: any) {
      problem = failure?.message ?? 'The item could not be opened.';
      tries++;
    } finally {
      opening = false;
    }
  }
</script>

<div class="stack" style="max-width:520px">
  {#if offer}<Qr text={offer} />{/if}
  <p style="font-weight:500">On the other device</p>
  <p class="dim">Open the item, choose Send to a device, and scan or paste this. Then paste the answer it gives you below. This works once and stops in two minutes, and nothing travels over a network.</p>
  <div><Btn label="Copy the code" icon="content_copy" disabled={!offer} onclick={() => copyPlainLine('Code', offer)} /></div>
  <Field bind:value={answer} multiline rows={4} mono placeholder="Paste its answer" />
  {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
  <div><Btn label={opening ? 'Opening…' : 'Open it'} variant="primary" disabled={!answer.trim() || !offer || opening} onclick={open} /></div>
</div>
