<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { ALL_TABS, prefs } from '../engine/prefs.svelte';
  import { TAB_INFO } from '../vault/tabs';
  import Choice from './Choice.svelte';
  import Reorder from './Reorder.svelte';
  import Toggle from './Toggle.svelte';

  const tabs = $derived(prefs.listOf('tabs'));
</script>

<Group title="Sidebar" help="setting.tabs">
  <Reorder items={tabs} label={(id) => TAB_INFO[id]?.label ?? id} locked={['all']} onchange={(next) => prefs.setList('tabs', next)} />
  {#each ALL_TABS.filter((t) => !tabs.includes(t)) as tab}
    <Row title="Add {TAB_INFO[tab].label}" icon={TAB_INFO[tab].icon} onclick={() => prefs.setList('tabs', [...tabs, tab])} />
  {/each}
</Group>
<Group title="Also in the sidebar">
  <Toggle title="Types" sub="Logins, cards and so on, once you have some." icon="dashboard" pref="sidebar_kinds" />
  <Toggle title="Folders" icon="folder" pref="sidebar_folders" />
  <Toggle title="Tags" icon="sell" pref="sidebar_tags" />
  <Toggle title="Counts" icon="numbers" pref="sidebar_counts" />
  <Toggle title="Show the sidebar" icon="view_sidebar" pref="sidebar_open" />
</Group>
<Group title="Starting">
  <Choice title="Open on" icon="history" pref="start_tab" options={[['first', 'First page'], ['last', 'Last page used']]} />
  <Toggle title="Lock button" sub="Show the padlock at the foot of the sidebar." icon="lock" pref="show_lock" />
</Group>
