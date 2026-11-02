<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Toggle from './Toggle.svelte';

  const SECTIONS: [string, string, IconName][] = [
    ['sites', 'Websites', 'language'],
    ['otp', 'One-time code', 'pin'],
    ['notes', 'Notes', 'sticky_note_2'],
    ['custom', 'More fields', 'tag'],
    ['questions', 'Security questions', 'help'],
    ['organise', 'Organise', 'folder'],
    ['details', 'More details', 'tune'],
  ];
  const open = $derived(prefs.listOf('editor_open'));
</script>

<Group title="Sections open at the start">
  {#each SECTIONS as [key, name, icon] (key)}
    <Row title={name} {icon}>
      <Switch checked={open.includes(key)} label={name} onchange={(on) => prefs.toggleInList('editor_open', key, on)} />
    </Row>
  {/each}
</Group>
<Group title="Behaviour">
  <Toggle title="Remember what I open" sub="A section you open or close stays that way next time." icon="history" pref="editor_remember" />
  <Toggle title="Fold less used fields" sub="Such as a card’s PIN or a database’s connection string." icon="expand_more" pref="editor_fold_details" />
  <Toggle title="Start logins with a password" sub="Fill in a generated password on every new login." icon="casino" pref="editor_generate" />
</Group>
