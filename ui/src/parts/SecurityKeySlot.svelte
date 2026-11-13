<script lang="ts">
  // A YubiKey as part of a KeePass database's key, as KeePassXC sets one up: its HMAC-SHA1
  // challenge-response slot, 2 unless the key was set up otherwise. Zero is no key.
  import Btn from './Btn.svelte';
  import Segmented from './Segmented.svelte';

  let { slot = $bindable(0), wide = true }: { slot?: 0 | 1 | 2; wide?: boolean } = $props();
</script>

{#if slot === 0}
  <Btn label="Use security key" icon="security_key" variant="quiet" {wide} onclick={() => (slot = 2)} />
{:else}
  <div class="stack" style="gap:6px">
    <div class="gap" style="justify-content:space-between;flex-wrap:wrap;gap:6px">
      <span class="dim small">Security key, challenge-response slot</span>
      <Segmented label="Challenge-response slot" value={slot} options={[[1, 'Slot 1'], [2, 'Slot 2']]} onchange={(chosen) => (slot = chosen)} />
    </div>
    <p class="dimmer small">Plug the key in. Touch it if it blinks.</p>
    <Btn label="Don’t use a security key" variant="quiet" {wide} onclick={() => (slot = 0)} />
  </div>
{/if}
