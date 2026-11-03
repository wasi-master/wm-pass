<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { kindIcon, kindName } from '../engine/face';
  import { DEFAULT_ADD, prefs } from '../engine/prefs.svelte';
  import Reorder from './Reorder.svelte';

  const shown = $derived(prefs.listOf('add_kinds'));
  const hidden = $derived(DEFAULT_ADD.filter((k) => !shown.includes(k)));
</script>

<Group title="Shown" help="setting.add_kinds">
  <Reorder items={shown} label={kindName} onchange={(next) => prefs.setList('add_kinds', next)} />
</Group>
{#if hidden.length > 0}
  <Group title="Hidden">
    {#each hidden as kind}
      <Row title="Add {kindName(kind)}" icon={kindIcon(kind)} onclick={() => prefs.setList('add_kinds', [...shown, kind])} />
    {/each}
  </Group>
{/if}
<Group>
  <Row title="Show all in the usual order" help="action.add_show_all" icon="refresh" onclick={() => prefs.setList('add_kinds', DEFAULT_ADD)} />
</Group>
