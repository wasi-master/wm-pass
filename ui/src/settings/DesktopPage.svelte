<script lang="ts">
  import Dialog from '../parts/Dialog.svelte';
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { chordLabel, chordOf } from '../engine/text';
  import Toggle from './Toggle.svelte';

  const desktop = host().desktop!;
  const os = host().os;
  let autostart = $state(false);
  let capturing = $state(false);

  $effect(() => {
    void desktop.autostart().then((on) => (autostart = on));
  });

  async function setAutostart(on: boolean) {
    try {
      await desktop.setAutostart(on);
      autostart = on;
    } catch (failure: any) {
      overlays.toast(failure.message);
    }
  }

  async function setChord(chord: string) {
    capturing = false;
    if (await desktop.setGlobalShortcut(chord)) prefs.set('global_palette', chord);
    else overlays.toast('Something else on this computer already uses that shortcut.');
  }

  function pressed(event: KeyboardEvent) {
    if (!capturing) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      capturing = false;
      return;
    }
    const chord = chordOf(event, os);
    if (chord) void setChord(chord);
  }
</script>

<svelte:window onkeydowncapture={pressed} />

<Group title="Window">
  <Toggle title="Keep running in the tray" sub="Closing the window leaves WM Pass in the tray, still locking on time. Scheduled backups need this." icon="window" pref="close_to_tray" onchange={(on) => desktop.setCloseToTray(on)} />
  <Toggle title="Start hidden" sub="Open to the tray rather than a window." icon="visibility_off" pref="start_hidden" />
  <Row title="Open when you sign in" icon="schedule">
    <Switch checked={autostart} label="Open when you sign in" onchange={setAutostart} />
  </Row>
</Group>
<Group title="From anywhere">
  <button type="button" class="set-row" onclick={() => (capturing = true)}>
    <span class="tile-glyph" style="background:var(--text3);display:none"></span>
    <span class="text">Quick search<span class="sub" style="display:block">Opens a small search box over whatever you are doing.</span></span>
    {#if prefs.v.global_palette}<span class="kbd">{chordLabel(prefs.v.global_palette, os)}</span>{:else}<span class="dimmer small">None</span>{/if}
  </button>
</Group>
<Group title="Typing for you">
  <Toggle
    title="Auto-type"
    sub={os === 'mac' ? 'Types a login into the app behind WM Pass. macOS asks for Accessibility permission the first time.' : os === 'linux' ? 'Types a login into the app behind WM Pass. Works under X11; Wayland desktops usually refuse.' : 'Types a login into the app behind WM Pass.'}
    icon="keyboard"
    pref="auto_type_enabled"
  />
</Group>

{#if capturing}
  <Dialog title="Quick search" onclose={() => (capturing = false)}>
    <p class="dim" style="padding:18px 0;text-align:center">Press the keys you want</p>
    {#snippet foot()}
      <Btn label="No shortcut" onclick={() => setChord('')} />
      <Btn label="Cancel" onclick={() => (capturing = false)} />
    {/snippet}
  </Dialog>
{/if}
