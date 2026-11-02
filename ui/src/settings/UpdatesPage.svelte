<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import { overlays } from '../parts/floating.svelte';
  import NetGate from './NetGate.svelte';
  import Toggle from './Toggle.svelte';

  const desktop = host().desktop!;
  let checking = $state(false);
  let found = $state<{ version: string; notes: string } | null>(null);
  let message = $state<string | null>(null);
  let installing = $state(false);

  async function check() {
    checking = true;
    message = null;
    try {
      found = await desktop.checkUpdate();
      if (!found) message = 'WM Pass is up to date.';
    } catch (failure: any) {
      message = failure.message;
    }
    checking = false;
  }

  async function install() {
    installing = true;
    try {
      await desktop.installUpdate();
    } catch (failure: any) {
      overlays.toast(failure.message);
      installing = false;
    }
  }
</script>

<NetGate what="Checks GitHub for a newer release, and sends nothing but the request itself. Each release is signed, and WM Pass refuses one whose signature does not match the key built into this app.">
  <Group>
    <Toggle title="Check when WM Pass starts" icon="update" pref="update_check" />
  </Group>
  <div class="stack" style="max-width:520px">
    {#if found}
      <p style="font-weight:500">WM Pass {found.version} is available</p>
      {#if found.notes}<p class="dim selectable" style="white-space:pre-wrap">{found.notes}</p>{/if}
      <div><Btn label={installing ? 'Installing…' : 'Install and restart'} variant="primary" disabled={installing} onclick={install} /></div>
    {:else}
      <div><Btn label={checking ? 'Checking…' : 'Check now'} icon="refresh" disabled={checking} onclick={check} /></div>
      {#if message}<p class="dim">{message}</p>{/if}
    {/if}
  </div>
</NetGate>
