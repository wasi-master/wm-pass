<script lang="ts">
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

  const gen = new GenState();
  let presets = $state<PresetView[]>([]);

  $effect(() => {
    void gen.roll();
    void pass.presets().then((list) => (presets = list));
  });

  async function copy() {
    await host().copyText(gen.value, true, prefs.v.clipboard_seconds);
    const seconds = prefs.v.clipboard_seconds;
    overlays.toast(seconds > 0 ? `Password copied. Clears in ${seconds} seconds.` : 'Password copied.');
  }
</script>

<div class="settings-pane scroll">
  <div class="settings-inner stack" style="gap:14px">
    <h1 class="page-title">Generator</h1>
    <div><GenValue state={gen} /></div>
    <div class="gap">
      <Btn label="Copy" variant="primary" icon="content_copy" onclick={copy} />
      <Btn label="Save as login" icon="add" onclick={() => ui.add('login', { presetPassword: gen.value })} />
    </div>
    <GenControls state={gen} />
    {#if presets.length}
      <div class="form-row">
        <span class="label">Presets</span>
        <span class="gap wrap">
          {#each presets as preset (preset.name)}
            <button type="button" class="chip" onclick={() => { gen.apply(preset); void gen.roll(); }}>{preset.name}</button>
          {/each}
        </span>
      </div>
    {/if}
  </div>
</div>
