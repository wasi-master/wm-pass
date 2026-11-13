<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import { host } from '../engine/host';
  import { overlays } from '../parts/floating.svelte';

  const link = host().link!;
  let shown = $state<any>({ status: 'off', code: null, unlocked: false, problem: null });
  let offerSave = $state(true);

  // A link that cannot answer leaves the last state shown rather than an unhandled failure.
  const refresh = async () => {
    try {
      shown = await link.state();
    } catch {
      // Shown again on the next change.
    }
  };

  $effect(() => {
    void refresh();
    void link.option('offerSave', true).then((v) => (offerSave = v !== false), () => undefined);
    return link.onChange(() => void refresh());
  });

  const WORDS: Record<string, string> = {
    off: 'Not connected.',
    absent: 'The WM Pass app is closed, or this browser has not been set up from it.',
    connecting: 'Connecting…',
    waiting: 'Waiting for the app to confirm.',
    ready: 'Connected.',
    compare: 'Check the code below.',
    failed: 'Could not connect.',
    declined: 'The app refused the connection.',
  };

  /** A step asked of the link: what it answers is shown, and a failure says so rather than nothing. */
  async function step(run: () => Promise<any>) {
    try {
      shown = await run();
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The desktop app did not answer.');
    }
  }

  // Connecting again means comparing codes with the app again, so it is asked first.
  async function forget() {
    const yes = await overlays.ask({
      title: 'Disconnect from the desktop app?',
      body: 'This extension stops using the vault open there. Connecting again means comparing codes with the app again.',
      action: 'Disconnect',
      danger: true,
    });
    if (!yes) return;
    try {
      shown = await link.forget();
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The connection could not be forgotten.');
    }
  }
</script>

<p class="dim" style="padding-bottom:14px">Connected to the desktop app, this extension uses the vault already open there, so you unlock once. Everything between the two is encrypted, and the program in the middle cannot read it.</p>

<Group title="Connection">
  <Row
    title={shown.status === 'failed' ? (shown.problem || WORDS.failed) : shown.status === 'absent' && shown.problem ? 'Could not connect.' : WORDS[shown.status]}
    icon="computer"
    sub={shown.status === 'ready' ? (shown.unlocked ? 'The vault there is open.' : 'The vault there is locked.') : shown.status === 'absent' && shown.problem ? shown.problem : undefined}
  >
    {#if shown.status === 'ready'}
      <Btn label="Disconnect" onclick={forget} />
    {:else if shown.status !== 'compare' && shown.status !== 'waiting' && shown.status !== 'connecting'}
      <Btn label="Connect" variant="primary" onclick={() => step(() => link.connect())} />
    {/if}
  </Row>
</Group>

{#if shown.status === 'compare'}
  <Group title="Compare this code">
    <div class="set-body stack">
      <p class="dim">The desktop app is showing a code too. If they differ, something else is listening.</p>
      <p class="mono" style="font-size:var(--fs-xl)">{shown.code}</p>
      <div class="gap">
        <Btn label="They differ" variant="danger" onclick={() => step(() => link.answer(false))} />
        <Btn label="They match" variant="primary" onclick={() => step(() => link.answer(true))} />
      </div>
    </div>
  </Group>
{/if}

<Group title="Saving">
  <Row title="Offer to save logins" help="action.offer_save" sub="After you sign in somewhere new, or with a changed password." icon="edit">
    <Switch checked={offerSave} label="Offer to save logins" onchange={(on) => {
        offerSave = on;
        void link.setOption('offerSave', on).catch((failure: any) => {
          offerSave = !on;
          overlays.toast(failure?.message ?? 'That could not be changed.');
        });
      }} />
  </Row>
</Group>
