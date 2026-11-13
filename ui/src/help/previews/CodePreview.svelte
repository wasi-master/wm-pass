<script lang="ts">
  import Ring from '../../parts/Ring.svelte';
  import Tile from '../../parts/Tile.svelte';
  import { prefs } from '../../engine/prefs.svelte';

  // One code row, stopped at a moment with eighteen seconds left.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  function pick<T>(key: string, current: T): T {
    return (pref === key && value !== undefined ? value : current) as T;
  }
  const size = $derived(pick('code_size', prefs.v.code_size));
  const groups = $derived(pick('code_groups', prefs.v.code_groups));
  const next = $derived(pick('code_next', prefs.v.code_next));
  const ring = $derived(pick('code_ring', prefs.v.code_ring));
  const hiddenUntilClicked = $derived(pick('code_hidden', prefs.v.code_hidden));
  let revealed = $state(false);
  const hidden = $derived(hiddenUntilClicked && !revealed);
  const show = (code: string) => (groups ? `${code.slice(0, 3)} ${code.slice(3)}` : code);
</script>

<button type="button" class="code-row" style="border-bottom:0;padding:4px 2px" onclick={() => (revealed = !revealed)}>
  <Tile title="GitHub" site="github.com" kind="otp" size={36} />
  <span class="grow">
    <span class="ell" style="display:block;font-weight:500">GitHub</span>
    <span class="dim small ell" style="display:block">sam@example.com</span>
    {#if hidden}
      <span class="code-value dimmer {size}">••• •••</span>
    {:else}
      <span class="code-value {size}">{show('482913')}</span>
    {/if}
    {#if next && !hidden}<span class="dim small mono" style="display:block">Next {show('105377')}</span>{/if}
  </span>
  {#if ring}<Ring period={30} left={18} size={26} />{/if}
</button>
<p class="preview-caption">A sample code.{hiddenUntilClicked ? ' Click it to show and hide it.' : ''}</p>
