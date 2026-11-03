<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Glyph from '../parts/Glyph.svelte';
  import Icon from '../parts/Icon.svelte';
  import Tile from '../parts/Tile.svelte';
  import { icons, type IconName } from '../engine/icons.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Choice from './Choice.svelte';

  const ICON_STYLES = [
    ['outlined', 'Material Symbols', 'The standard look'],
    ['rounded', 'Material Symbols Rounded', 'Softer corners'],
    ['sharp', 'Material Symbols Sharp', 'Square corners'],
    ['light', 'Rounded light', 'Thin strokes and soft corners'],
    ['phosphor', 'Phosphor', 'Even lines and friendly, rounded shapes'],
    ['phosphor_bold', 'Phosphor Bold', 'The same shapes in heavier lines'],
    ['phosphor_light', 'Phosphor Light', 'The same shapes in fine lines'],
    ['lucide', 'Lucide', 'Plain line drawings, as on many websites'],
  ];
  const SITE_STYLES = [
    ['brand', 'Brand colors', 'Each site’s mark on its own color'],
    ['tinted', 'Colored marks', 'Each mark in its color, on one background'],
    ['uniform', 'One color', 'Every mark the same, on one background'],
    ['letters', 'Letters', 'First letters only, no logos'],
  ];
  const SAMPLE: IconName[] = ['key', 'lock', 'search', 'settings', 'star', 'folder', 'delete'];
  let tables = $state<Record<string, Record<string, string>>>({});

  $effect(() => {
    for (const [style] of ICON_STYLES) void icons.tableOf(style).then((table) => (tables[style] = table));
  });
</script>

<Group title="App icons" help="setting.icon_style">
  {#each ICON_STYLES as [id, name, about] (id)}
    <button type="button" class="set-row" style="align-items:flex-start" onclick={() => prefs.set('icon_style', id as never)}>
      <span class="text stack" style="gap:8px">
        <span><span style="display:block">{name}</span><span class="sub" style="display:block">{about}</span></span>
        <span class="gap dim" style="gap:12px">
          {#each SAMPLE as glyph}
            <Glyph d={tables[id]?.[glyph]} style={id} size={20} />
          {/each}
        </span>
      </span>
      {#if prefs.v.icon_style === id}<span class="ok-text" style="display:flex"><Icon name="check" size={18} /></span>{/if}
    </button>
  {/each}
</Group>

<Group title="Site icons" help="setting.site_icon_style">
  {#each SITE_STYLES as [id, name, about] (id)}
    <button type="button" class="set-row" style="align-items:flex-start" onclick={() => prefs.set('site_icon_style', id as never)}>
      <span class="text stack" style="gap:8px">
        <span><span style="display:block">{name}</span><span class="sub" style="display:block">{about}</span></span>
        <span class="gap wrap">
          <Tile title="GitHub" site="github.com" size={30} style={id} />
          <Tile title="Discord" site="discord.com" size={30} style={id} />
          <Tile title="Amazon" site="amazon.com" size={30} style={id} />
          <Tile title="Figma" site="figma.com" size={30} style={id} />
          <Tile title="My bank" size={30} style={id} />
          <Tile title="Everyday Visa" kind="card" size={30} style={id} />
        </span>
      </span>
      {#if prefs.v.site_icon_style === id}<span class="ok-text" style="display:flex"><Icon name="check" size={18} /></span>{/if}
    </button>
  {/each}
  <Choice title="Shape" icon="image" pref="site_icon_shape" options={[['circle', 'Circle'], ['rounded', 'Rounded'], ['square', 'Square'], ['none', 'No background']]} />
</Group>

<Group title="Settings icons">
  <Choice title="Style" icon="palette" pref="settings_icons" options={[['coloured', 'Colored'], ['mono', 'One color'], ['bare', 'No background'], ['off', 'None']]} />
</Group>
