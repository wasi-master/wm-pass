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
      problem = failure?.message ?? 'Those are not settings WM Pass can read.';
    }
  }

  async function fromFile() {
    try {
      const file = await host().pickFile('Choose a settings file', ['json']);
      if (file) take(new TextDecoder().decode(file.bytes));
    } catch (failure: any) {
      problem = failure?.message ?? 'That file could not be read.';
    }
  }

  async function saveFile() {
    try {
      if (await host().saveFile('Save settings', 'wm-pass-settings.json', bytesOf(prefs.exportText()))) result = 'Settings saved to a file.';
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The settings file could not be saved.');
    }
  }

  function startImport() {
    // What went wrong with the last try is no help with the next one.
    problem = null;
    text = '';
    importing = true;
  }

  async function reset() {
    const yes = await overlays.ask({ title: 'Reset all customization?', body: 'Look, icons, layout and clicks go back to how they started.', action: 'Reset', danger: true });
    if (yes) {
      prefs.resetCustomisations();
      result = 'Everything is back to how it started.';
    }
  }
</script>

<p class="dim" style="padding-bottom:10px">Carry your look and layout to another device, or share them. Nothing from your vault is in this file, and neither are lock timers, screenshot blocking or other protection settings.</p>
{#if result}<p style="padding-bottom:10px">{result}</p>{/if}
<Group>
  <Row title="Save to a file" help="action.settings_save_file" icon="download" chevron onclick={saveFile} />
  <Row title="Copy as text" help="action.settings_copy_text" icon="content_copy" chevron onclick={() => copyPlainLine('Settings', prefs.exportText())} />
  <Row title="Import" help="action.settings_import" icon="upload" chevron onclick={startImport} />
  <Row title="Reset all customization" help="action.settings_reset" sub="Keeps your own themes and protection settings" icon="refresh" chevron onclick={reset} />
</Group>

{#if importing}
  <Dialog title="Import settings" onclose={() => (importing = false)}>
    <div class="stack">
      <Field bind:value={text} multiline rows={6} mono placeholder="Paste settings" autofocus />
      {#if problem}<p class="danger-text small" role="alert">{problem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (importing = false)} />
      <Btn label="Choose a file" icon="folder" onclick={fromFile} />
      <Btn label="Import" variant="primary" disabled={!text.trim()} onclick={() => take(text)} />
    {/snippet}
  </Dialog>
{/if}
