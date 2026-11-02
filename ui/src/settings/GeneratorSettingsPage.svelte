<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';
</script>

<Group title="What it starts with">
  <Choice title="Type" icon="casino" pref="gen_mode" options={[['password', 'Random'], ['passphrase', 'Words'], ['pronounceable', 'Pronounceable'], ['pin', 'PIN'], ['pattern', 'Pattern']]} />
  <Choice title="Length" icon="numbers" pref="gen_length" options={[12, 16, 20, 24, 32, 48, 64].map((n) => [n, String(n)] as [number, string])} />
  <Choice title="Words" icon="abc" pref="gen_words" options={[3, 4, 5, 6, 7, 8].map((n) => [n, String(n)] as [number, string])} />
  <Toggle title="Symbols" icon="tag" pref="gen_symbols" />
  <Toggle title="Avoid look-alikes" sub="Leaves out characters like 0 and O." icon="visibility" pref="gen_no_ambiguous" />
</Group>
<Group title="Display">
  <Toggle title="Show strength in bits" icon="balance" pref="gen_show_bits" />
</Group>
