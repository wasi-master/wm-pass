<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { bytesOf } from '../engine/frame';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import { copyPlainLine } from './clipboard';

  let importing = $state(false);
  let text = $state('');
  let problem = $state<string | null>(null);
  let result = $state<string | null>(null);

  function take(raw: string) {
    try {
      const changed = prefs.importText(raw);
      result = changed === 1 ? '1 setting changed.' : `${changed} settings changed.`;
      importing = false;
      text = '';
      problem = null;
    } catch (failure: any) {
      problem = failure.message;
    }
  }

  async function fromFile() {
    const file = await host().pickFile('Choose a settings file', ['json']);
    if (file) take(new TextDecoder().decode(file.bytes));
  }

  async function reset() {
    const yes = await overlays.ask({ title: 'Reset all customisation?', body: 'Look, icons, layout and clicks go back to how they started.', action: 'Reset' });
    if (yes) {
      prefs.resetCustomisations();
      result = 'Everything is back to how it started.';
    }
  }
</script>

<p class="dim" style="padding-bottom:10px">Carry your look and layout to another device, or share them. Nothing from your vault is in this file, and neither are lock timers, screenshot blocking or other protection settings.</p>
{#if result}<p style="padding-bottom:10px">{result}</p>{/if}
<Group>
  <Row title="Save to a file" icon="download" chevron onclick={() => host().saveFile('Save settings', 'wm-pass-settings.json', bytesOf(prefs.exportText()))} />
  <Row title="Copy as text" icon="content_copy" chevron onclick={() => copyPlainLine('Settings', prefs.exportText())} />
  <Row title="Import" icon="upload" chevron onclick={() => (importing = true)} />
  <Row title="Reset all customisation" sub="Keeps your own themes and protection settings" icon="refresh" chevron onclick={reset} />
</Group>

{#if importing}
  <Dialog title="Import settings" onclose={() => (importing = false)}>
    <div class="stack">
      <Field bind:value={text} multiline rows={6} mono placeholder="Paste settings" autofocus />
      {#if problem}<p class="danger-text small">{problem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (importing = false)} />
      <Btn label="Choose a file" icon="folder" onclick={fromFile} />
      <Btn label="Import" variant="primary" disabled={!text.trim()} onclick={() => take(text)} />
    {/snippet}
  </Dialog>
{/if}
