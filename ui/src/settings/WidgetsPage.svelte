<script lang="ts">
  // The Mac's widgets: whether the gallery offers them, the quick bar, the lock button and codes.
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { ui } from '../vault/ui.svelte';
  import Choice from './Choice.svelte';
  import Reorder from './Reorder.svelte';
  import Toggle from './Toggle.svelte';
  import { byName } from '../engine/text';
  import { WIDGET_BUTTONS } from './widgets.svelte';

  const MAX_CODES = 8;
  const buttons = $derived(prefs.listOf('widget_buttons').filter((b) => b in WIDGET_BUTTONS));
  const chosen = $derived(prefs.listOf('widget_codes'));
  const withCodes = $derived(pass.entries.filter((e) => e.has_otp && !e.trashed && !e.hidden).slice().sort((a, b) => byName(a.title, b.title)));

  function pick(id: string, on: boolean) {
    if (on && chosen.length >= MAX_CODES) return;
    // Kept in the order of the list below, which is the order the widget draws them.
    const next = new Set(on ? [...chosen, id] : chosen.filter((c) => c !== id));
    prefs.setList(
      'widget_codes',
      withCodes.map((e) => e.id).filter((c) => next.has(c)),
    );
  }
</script>

<Group title="Gallery">
  <Toggle title="Offer widgets" sub="Click the date in the menu bar and choose Edit Widgets to add them. Needs macOS 14 or later." icon="widgets" pref="desktop_widgets" />
</Group>
<Group title="Quick bar" help="setting.widget_buttons">
  <Reorder items={buttons} label={(id) => WIDGET_BUTTONS[id] ?? id} onchange={(next) => prefs.setList('widget_buttons', next)} />
  {#each Object.keys(WIDGET_BUTTONS).filter((b) => !buttons.includes(b)) as id}
    <Row title="Add {WIDGET_BUTTONS[id]}" icon="add" onclick={() => prefs.setList('widget_buttons', [...buttons, id])} />
  {/each}
</Group>
<Group title="Buttons">
  <Choice
    title="Lock button"
    sub={prefs.v.widget_lock === 'panic' ? 'Locks and refuses every password for an hour.' : 'Locks the vault.'}
    icon="lock"
    pref="widget_lock"
    options={[
      ['lock', 'Lock'],
      ['panic', 'Panic lock'],
    ]}
  />
</Group>
<Group title="Codes" help="setting.widget_codes">
  {#if !prefs.v.widget_access}
    <Row title="Show on widgets is off" sub="Turn it on under Security to see codes and favorites on widgets." icon="visibility_off" chevron onclick={() => ui.go('settings/security')} />
  {:else}
    <Choice
      title="Codes ahead"
      icon="schedule"
      pref="widget_codes_mode"
      options={[
        ['upcoming', 'The next few minutes'],
        ['current', 'Only the current code'],
      ]}
    />
    <Row title="Which codes" sub={chosen.length ? `${chosen.length} chosen, up to ${MAX_CODES}` : 'None chosen: your favorites that have a code'} icon="pin" />
    {#each withCodes as entry (entry.id)}
      <Row title={entry.title}>
        <!-- At the limit, the rest wait until one is turned off rather than ignoring the click. -->
        <Switch checked={chosen.includes(entry.id)} disabled={!chosen.includes(entry.id) && chosen.length >= MAX_CODES} label="Show {entry.title} on the Codes widget" onchange={(on) => pick(entry.id, on)} />
      </Row>
    {:else}
      <Row title="No item has a code yet." />
    {/each}
  {/if}
</Group>
