<script lang="ts">
  import type { Snippet } from 'svelte';
  import Btn from '../parts/Btn.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { networkExists } from './net.svelte';

  // In front of any feature that leaves this device: what it sends and to whom, and a choice.
  // Until it is made, what is inside does not run and nothing goes anywhere.
  // `remote` is for a page whose calls the desktop app makes on this shell's behalf.
  let { what, remote = false, children }: { what: string; remote?: boolean; children?: Snippet } = $props();
</script>

{#if !networkExists() && !remote}
  <p class="dim">This version of WM Pass does not make these calls itself. Use the desktop app or the phone for this.</p>
{:else if prefs.v.network_allowed}
  {@render children?.()}
{:else}
  <div class="stack" style="align-items:flex-start;max-width:480px">
    <p style="font-size:var(--fs-l);font-weight:600">This needs the network</p>
    <p class="dim">{what}</p>
    <p class="dimmer small">WM Pass stays offline until you allow this. You can turn it off again in Settings.</p>
    <Btn label="Allow network features" variant="primary" onclick={() => prefs.set('network_allowed', true)} />
  </div>
{/if}
