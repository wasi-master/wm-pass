<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import Toggle from './Toggle.svelte';

  const desktop = host().desktop!;
  let shown = $state<{ running: boolean; paired: { id: string; name: string }[] }>({ running: false, paired: [] });
  let fingerprint = $state('');
  let installed = $state<string[] | null>(null);

  async function refresh() {
    shown = await desktop.bridgeState();
  }

  $effect(() => {
    void refresh().catch(() => undefined);
    const timer = setInterval(() => void refresh().catch(() => undefined), 1500);
    return () => clearInterval(timer);
  });

  $effect(() => {
    if (prefs.v.bridge_enabled) void desktop.bridgeFingerprint().then((f) => (fingerprint = f), () => (fingerprint = ''));
  });

  async function install() {
    try {
      installed = await desktop.installBrowserHost();
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The browsers could not be set up.');
    }
  }

  async function apply() {
    try {
      await desktop.bridgeSetEnabled(prefs.v.bridge_enabled, prefs.v.bridge_ask_every_site);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The connection could not be changed.');
    }
    await refresh().catch(() => undefined);
  }

  // Forgetting one means comparing fingerprints again before it can fill anything.
  async function forget(item: { id: string; name: string }) {
    const yes = await overlays.ask({ title: `Forget ${item.name}?`, body: 'It will need connecting again, with the fingerprint checked, before it can use this vault.', action: 'Forget', danger: true });
    if (!yes) return;
    try {
      await desktop.bridgeForget(item.id);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That extension could not be forgotten.');
    }
    await refresh().catch(() => undefined);
  }
</script>

<p class="dim" style="padding-bottom:14px">The browser extension works on its own. Connected to this app it uses the vault already open here, so you unlock once. The two compare a fingerprint on first contact, and the link is encrypted after that.</p>

<Group title="Connection" anchor="group:browser.connect">
  <Toggle title="Let the extension connect" icon="extension" pref="bridge_enabled" onchange={apply} />
  <Toggle title="Ask before the first fill on each site" sub="Recommended. A site approved once fills without asking again." icon="rule" pref="bridge_ask_every_site" onchange={apply} />
  <Row title="Set up browsers" help="action.setup_browsers" sub={installed ? (installed.length ? `Ready in ${installed.join(', ')}.` : 'No supported browser was found.') : 'Registers WM Pass with Chrome, Edge, Brave, Vivaldi, Chromium and Firefox.'} icon="language" chevron onclick={install} />
</Group>

{#if prefs.v.bridge_enabled && fingerprint}
  <Group title="This computer" anchor="group:browser.setup">
    <Row title="Fingerprint" help="action.bridge_fingerprint"><span class="mono dim selectable">{fingerprint}</span></Row>
  </Group>
{/if}

{#if shown.paired.length > 0}
  <Group title="Connected extensions" help="group.browser.paired" anchor="group:browser.paired">
    {#each shown.paired as item (item.id)}
      <Row title={item.name} icon="extension">
        <IconBtn icon="delete" label="Forget {item.name}" onclick={() => forget(item)} />
      </Row>
    {/each}
  </Group>
{/if}
