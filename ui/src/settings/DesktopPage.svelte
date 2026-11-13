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
    void desktop
      .autostart()
      .then((on) => (autostart = on))
      .catch(() => undefined);
  });

  async function setAutostart(on: boolean) {
    try {
      await desktop.setAutostart(on);
      autostart = on;
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be changed.');
    }
  }

  let chordProblem = $state<string | null>(null);

  async function setChord(chord: string) {
    capturing = false;
    try {
      if (await desktop.setGlobalShortcut(chord)) prefs.set('global_palette', chord);
      else overlays.toast('Something else on this computer already uses that shortcut.');
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That shortcut could not be set.');
    }
  }

  function startCapture() {
    chordProblem = null;
    capturing = true;
  }

  function pressed(event: KeyboardEvent) {
    if (!capturing) return;
    // Plain Tab, Enter and Space still reach the dialog's buttons, so it can be left by keyboard.
    const plain = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
    if (plain && (event.key === 'Tab' || event.key === 'Enter' || event.key === ' ')) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      capturing = false;
      return;
    }
    const chord = chordOf(event, os);
    if (!chord) return;
    // This one works in every app: a key on its own, or with Shift only, would be taken from all
    // typing on the computer. Function keys are fine alone.
    // Only a Mac's ⌘ is part of a chord: elsewhere the system key is left out of it.
    const held = event.ctrlKey || event.altKey || (os === 'mac' && event.metaKey);
    if (!held && !/^F\d+$/.test(event.key)) {
      chordProblem = `Hold ${os === 'mac' ? '⌘, ⌥ or ⌃' : 'Ctrl or Alt'} as well, or use a function key.`;
      return;
    }
    void setChord(chord);
  }
</script>

<svelte:window onkeydowncapture={pressed} />

<Group title="Window" anchor="group:desktop.window">
  <Toggle title="Keep running in the tray" sub="Closing the window leaves WM Pass in the tray, still locking on time. Scheduled backups need this." icon="window" pref="close_to_tray" onchange={(on) => desktop.setCloseToTray(on).catch((failure) => overlays.toast(failure?.message ?? 'That could not be changed.'))} />
  <Toggle title="Start hidden" sub="Open to the tray rather than a window." icon="visibility_off" pref="start_hidden" />
  <Row title="Open when you sign in" help="action.autostart" icon="schedule">
    <Switch checked={autostart} label="Open when you sign in" onchange={setAutostart} />
  </Row>
</Group>
<Group title="From anywhere" anchor="group:desktop.palette">
  <Row title="Quick search" sub="Opens a small search box over whatever you are doing." help="setting.global_palette" onclick={startCapture}>
    {#if prefs.v.global_palette}<span class="kbd">{chordLabel(prefs.v.global_palette, os)}</span>{:else}<span class="dimmer small">None</span>{/if}
  </Row>
</Group>
<Group title="Typing for you" anchor="group:desktop.autotype">
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
    {#if chordProblem}<p class="danger-text small" role="alert" style="text-align:center">{chordProblem}</p>{/if}
    {#snippet foot()}
      <Btn label="No shortcut" onclick={() => setChord('')} />
      <Btn label="Cancel" onclick={() => (capturing = false)} />
    {/snippet}
  </Dialog>
{/if}
