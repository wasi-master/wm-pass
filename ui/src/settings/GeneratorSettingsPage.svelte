<script lang="ts">
  import Group from '../parts/Group.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';

  // The phone offers lengths these do not, and a value from it shows here rather than a blank menu.
  const withCurrent = (list: number[], now: number) => [...new Set([...list, now])].sort((a, b) => a - b).map((n) => [n, String(n)] as [number, string]);
</script>

<Group title="What it starts with">
  <Choice title="Type" icon="casino" pref="gen_mode" options={[['password', 'Random'], ['passphrase', 'Words'], ['pronounceable', 'Pronounceable'], ['pin', 'PIN'], ['pattern', 'Pattern']]} />
  <!-- Only what the chosen type uses: a length means nothing to words, nor a word count to a password. -->
  {#if prefs.v.gen_mode === 'password'}
    <Choice title="Length" icon="numbers" pref="gen_length" options={withCurrent([12, 16, 20, 24, 32, 48, 64], prefs.v.gen_length)} />
    <Toggle title="Symbols" icon="tag" pref="gen_symbols" />
    <Toggle title="Avoid look-alikes" sub="Leaves out characters like 0 and O." icon="visibility" pref="gen_no_ambiguous" />
  {:else if prefs.v.gen_mode === 'passphrase'}
    <Choice title="Words" icon="abc" pref="gen_words" options={withCurrent([3, 4, 5, 6, 7, 8], prefs.v.gen_words)} />
  {/if}
</Group>
<Group title="Display">
  <Toggle title="Show strength in bits" icon="balance" pref="gen_show_bits" />
</Group>
