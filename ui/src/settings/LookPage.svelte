<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { ui } from '../vault/ui.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';
</script>

<Group title="Interface">
  <Row title="Theme" help="action.theme_choose" sub="Dracula, Catppuccin, Nord, Solarized and more, or your own" icon="palette" value={prefs.theme.name} chevron onclick={() => ui.go('settings/themes')} />
  <Choice title="Dark mode" icon="contrast" pref="look" options={[['SYSTEM', 'System'], ['LIGHT', 'Light'], ['DARK', 'Dark'], ['OBSIDIAN', 'Black']]} />
  <Choice title="Row height" icon="density_medium" pref="density" options={[['compact', 'Compact'], ['comfortable', 'Comfortable'], ['spacious', 'Spacious']]} />
  <Row title="Text size" help="setting.text_scale" icon="text_increase" value="{prefs.v.text_scale}%">
    <span style="width:160px;display:flex">
      <input type="range" min="85" max="130" step="5" value={prefs.v.text_scale} aria-label="Text size" oninput={(e) => prefs.set('text_scale', Number(e.currentTarget.value))} />
    </span>
  </Row>
</Group>
<Group title="Motion">
  <Toggle title="Animations" sub="Turn off for shorter transitions." icon="animation" pref="rich_motion" />
</Group>
