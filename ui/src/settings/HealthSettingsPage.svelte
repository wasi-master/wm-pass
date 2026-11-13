<script lang="ts">
  import type { TopicId } from '$help';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Toggle from './Toggle.svelte';

  const CHECKS: [string, string, IconName][] = [
    ['breached', 'In a known breach', 'dangerous'],
    ['reused', 'Used more than once', 'swap_horiz'],
    ['weak', 'Easy to guess', 'gpp_bad'],
    ['insecure', 'Unencrypted sites', 'warning'],
    ['expiring', 'Expiring soon', 'schedule'],
    ['old', 'Not changed in a year', 'history'],
    ['twofactor', 'No second factor', 'pin'],
    ['duplicates', 'Duplicate items', 'layers'],
    ['similar', 'Maybe the same account', 'layers'],
  ];
  const hidden = $derived(prefs.listOf('health_hidden'));
</script>

<Group title="Page">
  <Toggle title="Show the score" icon="ecg_heart" pref="health_score" />
  <Toggle title="Leave out local addresses" sub="Logins only for your router, NAS or other devices at home." icon="router" pref="health_skip_local" />
</Group>
<Group title="Problems to list" help="group.health.problems">
  {#each CHECKS as [key, name, icon] (key)}
    <!-- The guide explains Maybe the same account only for the phone so far. -->
    <Row title={name} {icon} help={key === 'similar' ? null : (`action.health_check_${key}` as TopicId)}>
      <Switch checked={!hidden.includes(key)} label={name} onchange={(on) => prefs.toggleInList('health_hidden', key, !on)} />
    </Row>
  {/each}
</Group>
