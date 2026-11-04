<script lang="ts">
  import Btn from '$ui/parts/Btn.svelte';

  // Shown when the extension is in this browser. The vault there runs code the browser's store
  // signed; the vault here runs what this site served today. Both are offered, the first first.
  let { onextension, onhere }: { onextension: (always: boolean) => void; onhere: () => void } = $props();
  let always = $state(false);
</script>

<main class="handoff">
  <img src="./icons/wm-pass-256.png" alt="" width="64" height="64" />
  <h1>WM Pass is in this browser</h1>
  <p class="dim">Open your vault in the extension. Its code comes signed from the browser’s store and stays the same between updates, where this page is sent by the website each visit.</p>
  <div class="actions">
    <Btn label="Open in the extension" variant="primary" wide onclick={() => onextension(always)} />
    <Btn label="Use the web vault here" variant="quiet" wide onclick={onhere} />
  </div>
  <label class="small dim"><input type="checkbox" bind:checked={always} /> Always open the extension from here</label>
  <p class="small dimmer">The two keep separate vaults. Move one across with a backup place, or the file under Settings, Export.</p>
</main>

<style>
  .handoff { max-width: 420px; margin: 12vh auto 0; padding: 0 16px; display: grid; gap: 14px; justify-items: center; text-align: center; }
  h1 { font-size: var(--fs-xl, 22px); margin: 0; }
  p { margin: 0; }
  .actions { display: grid; gap: 8px; width: 100%; }
  label { display: flex; gap: 8px; align-items: center; }
</style>
