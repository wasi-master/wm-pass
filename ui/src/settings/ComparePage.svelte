<script lang="ts">
  import PasswordField from '../parts/PasswordField.svelte';
  import Strength from '../parts/Strength.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import type { CompareView, StrengthView } from '../engine/types';

  let first = $state('');
  let second = $state('');
  let result = $state<CompareView | null>(null);

  // Typed on while one was compared: only the newest answer is shown.
  let asked = 0;
  $effect(() => {
    const turn = ++asked;
    const a = first;
    const b = second;
    if (!a || !b) {
      result = null;
      return;
    }
    const left = bytesOf(a);
    const right = bytesOf(b);
    void pass
      .compare(left, right)
      .then((r) => turn === asked && (result = r))
      .catch(() => turn === asked && (result = null))
      .finally(() => {
        left.fill(0);
        right.fill(0);
      });
  });
</script>

{#snippet side(view: StrengthView, winner: boolean)}
  <Strength score={view.score} active />
  <p class="small" class:ok-text={winner} class:dim={!winner}>Would hold for {view.crack_slow_hash}</p>
{/snippet}

<div class="stack" style="max-width:480px">
  <PasswordField bind:value={first} placeholder="First password" autofocus />
  {#if result}{@render side(result.first, result.winner === 'first')}{/if}
  <PasswordField bind:value={second} placeholder="Second password" />
  {#if result}{@render side(result.second, result.winner === 'second')}{/if}
  {#if result}
    <p style="font-weight:500;padding-top:6px">
      {result.winner === 'even' ? 'About the same.' : `The ${result.winner} is about ${Math.round(result.factor).toLocaleString()} times harder to guess.`}
    </p>
  {/if}
</div>
