<script lang="ts">
  import { untrack } from 'svelte';
  import HelpButton from '../help/HelpButton.svelte';
  import Btn from '../parts/Btn.svelte';
  import { pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import type { PresetView } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { GenState } from './gen.svelte';
  import GenControls from './GenControls.svelte';
  import GenValue from './GenValue.svelte';
  import { ui } from './ui.svelte';
  import { secondsText } from '../engine/text';

  const gen = new GenState();
  let presets = $state<PresetView[]>([]);

  // Once, on opening: the controls roll again themselves when a setting changes.
  $effect(() => {
    untrack(() => {
      void gen.roll();
      void pass.presets().then(
        (list) => (presets = list),
        () => undefined,
      );
    });
  });

  async function copy() {
    const what = gen.mode === 'passphrase' ? 'Passphrase' : gen.mode === 'pin' ? 'PIN' : 'Password';
    try {
      await host().copyText(gen.value, true, prefs.v.clipboard_seconds);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be copied.');
      return;
    }
    const seconds = prefs.v.clipboard_seconds;
    // As a copy from an item says it, with the same way to clear it at once.
    overlays.toast(seconds > 0 ? `${what} copied. Clears in ${secondsText(seconds)}.` : `${what} copied.`, {
      action: 'Clear now',
      run: () => void host().clearClipboard(),
      ms: 3200,
      keep: true,
    });
  }
</script>

<div class="settings-pane scroll">
  <div class="settings-inner stack" style="gap:14px">
    <div class="page-title-row">
      <h1 class="page-title">Generator</h1>
      <HelpButton id="screen.generator" title="the generator" />
    </div>
    <div data-help-anchor="generator.value"><GenValue state={gen} /></div>
    <div class="gap">
      <Btn label="Copy" variant="primary" icon="content_copy" disabled={!gen.value} onclick={copy} />
      <span data-help-anchor="generator.save" style="display:flex"><Btn label="Save as login" icon="add" disabled={!gen.value} onclick={() => ui.add('login', { presetPassword: gen.value })} /></span>
    </div>
    <GenControls state={gen} />
    {#if presets.length}
      <div class="form-row">
        <span class="label">Presets</span>
        <span class="gap wrap" role="group" aria-label="Presets">
          {#each presets as preset (preset.name)}
            <button type="button" class="chip" onclick={() => { gen.apply(preset); void gen.roll(); }}>{preset.name}</button>
          {/each}
        </span>
      </div>
    {/if}
  </div>
</div>
