<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';
</script>

<Group title="Display">
  <Choice title="Size" icon="match_case" pref="code_size" options={[['small', 'Small'], ['normal', 'Normal'], ['large', 'Large']]} />
  <Toggle title="Split in two groups" sub="Shows 123 456 rather than 123456." icon="numbers" pref="code_groups" />
  <Toggle title="Show the next code" sub="Useful when a code is about to change." icon="schedule" pref="code_next" />
  <Toggle title="Time ring" sub="How long the code has left." icon="timer" pref="code_ring" />
  <Choice title="Order" icon="sort" pref="code_sort" options={[['default', 'As in the vault'], ['name', 'By name']]} />
</Group>
<Group title="Privacy">
  <Toggle title="Hide codes until clicked" sub="A click shows the code for 15 seconds and copies it." icon="visibility_off" pref="code_hidden" />
</Group>
