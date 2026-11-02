<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { host } from '../engine/host';

  const link = host().link!;
  let shown = $state<any>({ status: 'off', code: null, unlocked: false, problem: null });
  let offerSave = $state(true);

  const refresh = async () => (shown = await link.state());

  $effect(() => {
    void refresh();
    void link.option('offerSave', true).then((v) => (offerSave = v as boolean));
    return link.onChange(() => void refresh());
  });

  const WORDS: Record<string, string> = {
    off: 'Not connected.',
    absent: 'The WM Pass app is closed, or this browser has not been set up from it.',
    connecting: 'Connecting…',
    waiting: 'Waiting for the app to confirm.',
    ready: 'Connected.',
    declined: 'The app refused the connection.',
  };

  async function forget() {
    shown = await link.forget();
  }
</script>

<p class="dim" style="padding-bottom:14px">Connected to the desktop app, this extension uses the vault already open there, so you unlock once. Everything between the two is encrypted, and the program in the middle cannot read it.</p>

<Group title="Connection">
  <Row title={shown.status === 'failed' ? shown.problem : WORDS[shown.status]} icon="computer" sub={shown.status === 'ready' ? (shown.unlocked ? 'The vault there is open.' : 'The vault there is locked.') : undefined}>
    {#if shown.status === 'ready'}
      <Btn label="Disconnect" onclick={forget} />
    {:else if shown.status !== 'compare' && shown.status !== 'waiting' && shown.status !== 'connecting'}
      <Btn label="Connect" variant="primary" onclick={async () => (shown = await link.connect())} />
    {/if}
  </Row>
</Group>

{#if shown.status === 'compare'}
  <Group title="Compare this code">
    <div class="set-body stack">
      <p class="dim">The desktop app is showing a code too. If they differ, something else is listening.</p>
      <p class="mono" style="font-size:var(--fs-xl)">{shown.code}</p>
      <div class="gap">
        <Btn label="They differ" variant="danger" onclick={async () => (shown = await link.answer(false))} />
        <Btn label="They match" variant="primary" onclick={async () => (shown = await link.answer(true))} />
      </div>
    </div>
  </Group>
{/if}

<Group title="Saving">
  <Row title="Offer to save logins" sub="After you sign in somewhere new, or with a changed password." icon="edit">
    <input type="checkbox" checked={offerSave} onchange={(e) => { offerSave = e.currentTarget.checked; void link.setOption('offerSave', offerSave); }} aria-label="Offer to save logins" />
  </Row>
</Group>
