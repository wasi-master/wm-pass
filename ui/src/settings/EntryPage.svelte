<script lang="ts">
  import Group from '../parts/Group.svelte';
  import { pass } from '../engine/pass.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';

  let limit = $state<number | null>(null);
  $effect(() => {
    void pass.info().then((info) => (limit = info.history_limit));
  });
  const limits = $derived([...new Set([5, 10, 20, 50, limit ?? 10])].sort((a, b) => a - b).map((n) => [n, String(n)] as [number, string]));
</script>

<Group title="Buttons at the top">
  <Toggle title="Show them" icon="touch_app" pref="entry_actions" />
  <Toggle title="Copy username" icon="person" pref="entry_action_username" />
  <Toggle title="Open site" icon="open_in_new" pref="entry_action_open" />
</Group>
<Group title="Fields">
  <Toggle title="Click a field to copy it" icon="content_copy" pref="tap_copies" />
  <Choice title="Hide shown secrets after" icon="timer" pref="reveal_seconds" options={[[15, '15 seconds'], [45, '45 seconds'], [120, '2 minutes'], [0, 'Leaving']] as [number, string][]} />
  <Toggle title="Colour digits and symbols" sub="Makes a password easier to read aloud or retype." icon="palette" pref="colour_secrets" />
</Group>
{#if limit !== null}
  <Group title="History">
    <Choice title="Old passwords to keep" icon="history" value={limit} options={limits} onchange={(value) => { limit = value; void pass.change('vault.prefs', { history_limit: value }); }} />
  </Group>
{/if}
