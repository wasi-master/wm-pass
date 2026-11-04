<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';

  const ACTIONS: [string, string][] = [
    ['password', 'Copy password'],
    ['username', 'Copy username'],
    ['code', 'Copy code'],
  ];
</script>

<Group title="Layout">
  <Choice title="Layout" icon="view_agenda" pref="layout" options={[['list', 'List'], ['compact', 'Compact'], ['grid', 'Grid']]} />
  <Choice title="Grouping" icon="layers" pref="grouping" options={[['smart', 'By site'], ['folder', 'By folder'], ['types', 'By type']]} />
  <Choice title="Sort by" icon="sort" pref="sort" options={[['name', 'Name'], ['changed', 'Last changed'], ['kind', 'Type'], ['site', 'Website']]} />
  <Toggle title="Pinned items first" sub="Keep pinned items above the rest." icon="keep" pref="pinned_on_top" />
  <Toggle title="Fold logins on one site" sub="Show several logins for a site as one row." icon="layers" pref="group_sites" />
  <Toggle title="Section headings" icon="format_list_bulleted" pref="list_headings" />
</Group>
<Group title="Rows">
  <Toggle title="Icons" icon="image" pref="row_icons" />
  <Toggle title="Second line" sub="The username or other detail under the name." icon="text_fields" pref="row_subtitle" />
  <Toggle title="Small marks" sub="Favorite, one-time code, passkey and attachment." icon="star" pref="row_badges" />
  <Choice title="Button on the row" sub="Shows when the pointer is over a row." icon="content_copy" pref="row_button" options={[...ACTIONS, ['none', 'None']]} />
</Group>
<Group title="Clicks">
  <Choice title="Click" icon="touch_app" pref="tap_action" options={[['open', 'Open'], ...ACTIONS]} />
  <Choice title="Double-click" icon="touch_app" pref="double_click" options={[...ACTIONS.slice(0, 2), ['edit', 'Edit'], ['open_site', 'Open site'], ['none', 'Nothing']]} />
</Group>
