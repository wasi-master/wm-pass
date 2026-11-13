<script lang="ts">
  import Group from '../parts/Group.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';

  // The phone or a settings file may have chosen a time not listed here: it shows rather than a blank menu.
  const revealText = (s: number) => (s === 0 ? 'Leaving' : s % 60 === 0 ? (s === 60 ? '1 minute' : `${s / 60} minutes`) : `${s} seconds`);
  const revealOptions = $derived(
    [...new Set([15, 45, 120, 0, prefs.v.reveal_seconds])].sort((a, b) => (a || Infinity) - (b || Infinity)).map((s) => [s, revealText(s)] as [number, string]),
  );

  let limit = $state<number | null>(null);
  $effect(() => {
    void pass
      .info()
      .then((info) => (limit = info.history_limit))
      .catch(() => undefined);
  });
  // Another device or the terminal app may have chosen 0, which keeps every old password.
  const limits = $derived([...new Set([5, 10, 20, 50, limit ?? 10])].sort((a, b) => (a || Infinity) - (b || Infinity)).map((n) => [n, n === 0 ? 'All' : String(n)] as [number, string]));

  async function keep(value: number) {
    const before = limit;
    limit = value;
    try {
      await pass.change('vault.prefs', { history_limit: value });
    } catch (failure: any) {
      limit = before;
      overlays.toast(failure?.message ?? 'That could not be changed.');
    }
  }
</script>

<Group title="Buttons at the top">
  <Toggle title="Show them" icon="touch_app" pref="entry_actions" />
  <!-- Which buttons, only while there are buttons to show. -->
  {#if prefs.v.entry_actions}
    <Toggle title="Copy username" icon="person" pref="entry_action_username" />
    <Toggle title="Open site" icon="open_in_new" pref="entry_action_open" />
  {/if}
</Group>
<Group title="Fields">
  <Toggle title="Click a field to copy it" icon="content_copy" pref="tap_copies" />
  <Choice title="Hide shown secrets after" icon="timer" pref="reveal_seconds" options={revealOptions} />
  <Toggle title="Color digits and symbols" sub="Makes a password easier to read aloud or retype." icon="palette" pref="colour_secrets" />
</Group>
{#if limit !== null}
  <Group title="History">
    <Choice title="Old passwords to keep" help="action.history_keep" icon="history" value={limit} options={limits} onchange={(value) => void keep(value)} />
  </Group>
{/if}
