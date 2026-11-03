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
    void refresh();
    const timer = setInterval(() => void refresh(), 1500);
    return () => clearInterval(timer);
  });

  $effect(() => {
    if (prefs.v.bridge_enabled) void desktop.bridgeFingerprint().then((f) => (fingerprint = f));
  });

  async function install() {
    try {
      installed = await desktop.installBrowserHost();
    } catch (failure: any) {
      overlays.toast(failure.message);
    }
  }

  const apply = () => desktop.bridgeSetEnabled(prefs.v.bridge_enabled, prefs.v.bridge_ask_every_site);
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
        <IconBtn icon="delete" label="Forget {item.name}" onclick={() => desktop.bridgeForget(item.id).then(refresh)} />
      </Row>
    {/each}
  </Group>
{/if}
