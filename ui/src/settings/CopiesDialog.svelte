<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Row from '../parts/Row.svelte';
  import type { PlaceFile } from '../engine/host';
  import { placeProblem, places, type Place } from './places.svelte';

  // The copies kept at a place, newest first. Choosing one reads it and hands it on: Backup merges
  // it into the open vault, the door restores it as this device's vault.
  let { place, onclose, onchoose }: { place: Place; onclose: () => void; onchoose: (bytes: Uint8Array) => Promise<void> } = $props();

  let copies = $state<PlaceFile[] | null>(null);
  let problem = $state<string | null>(null);
  let busy = $state(false);

  $effect(() => {
    const current = place;
    copies = null;
    problem = null;
    places.copies(current).then(
      (found) => (copies = found),
      (failure) => (problem = placeProblem(failure)),
    );
  });

  function whenShort(ms: number) {
    return new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
  }

  async function choose(file: PlaceFile) {
    busy = true;
    problem = null;
    try {
      await onchoose(await places.read(place, file));
    } catch (failure) {
      problem = placeProblem(failure);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title="Copies at {place.label}" {onclose}>
  {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
  {#if busy}<p class="dim">Reading the copy…</p>
  {:else if copies === null && !problem}<p class="dim">Looking…</p>{/if}
  {#if copies?.length === 0}<p class="dim">There are no copies here yet.</p>{/if}
  {#if copies?.length && !busy}
    <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
      {#each copies.slice(0, 30) as file}
        <Row title={file.modified_ms > 0 ? whenShort(file.modified_ms) : file.name} icon="history" onclick={() => choose(file)} />
      {/each}
    </div>
  {/if}
  {#snippet foot()}<Btn label="Close" onclick={onclose} />{/snippet}
</Dialog>
