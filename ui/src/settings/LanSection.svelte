<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Qr from '../parts/Qr.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { consumeText, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { lan, type LanPeer } from './lan.svelte';
  import NetGate from './NetGate.svelte';

  type Mode = 'idle' | 'waiting' | 'qr' | 'code_wait' | 'code_compare';
  let mode = $state<Mode>('idle');
  let qrText = $state('');
  let message = $state<string | null>(null);
  let syncing = $state(false);
  let adding = $state(false);
  let pasting = $state(false);
  let pasted = $state('');
  let codeText = $state('');
  let codePeer = $state('');

  $effect(() => {
    void lan.load();
    return () => void lan.stop();
  });

  function failed(failure: any) {
    mode = 'idle';
    if (failure?.code === 'cancelled') return;
    message = failure?.message ?? 'That did not work.';
  }

  function report(json: any) {
    return `Done with ${json.peer}: ${json.sent} sent and ${json.received} received.`;
  }

  const paired = 'Paired. Choose Sync now to bring them together.';

  async function syncNow() {
    syncing = true;
    message = null;
    try {
      message = report(await lan.sync());
    } catch (failure) {
      failed(failure);
    }
    syncing = false;
  }

  async function wait() {
    mode = 'waiting';
    message = null;
    try {
      message = report(await lan.serve());
    } catch (failure) {
      failed(failure);
    }
    mode = 'idle';
  }

  async function showCode() {
    adding = false;
    const address = await host().lan!.localAddress();
    if (!address) {
      message = 'This computer is not on a network.';
      return;
    }
    try {
      const port = (await pass.engine('lan.listen', {}, {})).json.port;
      qrText = consumeText((await pass.engine('lan.pair_show', { address: `${address}:${port}` }, {})).secret);
      mode = 'qr';
      const peer = await lan.withIdentity(async (identity) => (await pass.engine('lan.pair_wait', { name: await lan.name() }, { secret: identity })).json);
      await lan.addPeer({ key: peer.key, name: peer.name });
      message = paired;
    } catch (failure) {
      failed(failure);
    }
    mode = 'idle';
    void lan.stop();
  }

  async function usePasted() {
    pasting = false;
    try {
      const reply = await lan.withIdentity(async (identity) => (await pass.engine('lan.pair_scan', { text: pasted.trim(), name: await lan.name() }, { secret: identity })).json);
      await lan.addPeer({ key: reply.peer.key, name: reply.peer.name });
      message = paired;
      pasted = '';
    } catch (failure) {
      failed(failure);
    }
  }

  async function compareWait() {
    adding = false;
    try {
      const port = (await pass.engine('lan.listen', {}, {})).json.port;
      await pass.engine('lan.advertise', { port }, {});
      mode = 'code_wait';
      const reply = await lan.withIdentity(async (identity) => (await pass.engine('lan.code_wait', { name: await lan.name() }, { secret: identity })).json);
      codeText = reply.code;
      codePeer = reply.peer.name;
      mode = 'code_compare';
    } catch (failure) {
      failed(failure);
    }
  }

  async function compareConnect() {
    adding = false;
    try {
      const found = await lan.browse();
      if (!found[0]) throw new Error('No device is waiting on this network.');
      const reply = await lan.withIdentity(async (identity) => (await pass.engine('lan.code_connect', { address: found[0], name: await lan.name() }, { secret: identity })).json);
      codeText = reply.code;
      codePeer = reply.peer.name;
      mode = 'code_compare';
    } catch (failure) {
      failed(failure);
    }
  }

  async function finish(accepted: boolean) {
    try {
      const peer = (await pass.engine('lan.code_finish', { accepted }, {})).json;
      if (accepted) {
        await lan.addPeer({ key: peer.key, name: peer.name });
        message = paired;
      }
    } catch (failure) {
      failed(failure);
    }
    mode = 'idle';
    void lan.stop();
  }

  async function forget(peer: LanPeer) {
    const yes = await overlays.ask({ title: `Forget ${peer.name}?`, body: 'It will need pairing again before it can sync with this computer.', action: 'Forget', danger: true });
    if (yes) await lan.removePeer(peer.key);
  }
</script>

<Group title="Your devices, on this network">
  <div class="set-body stack">
    <NetGate what="Sync talks directly to your own devices on this network, encrypted end to end, with nothing in between. It still counts as network use, so it needs your yes like the rest.">
      {#if mode === 'waiting'}
        <p style="font-weight:500">Waiting for your other device</p>
        <p class="dim">Choose Sync now on it. Stay on this page.</p>
        <div><Btn label="Stop" onclick={() => lan.stop()} /></div>
      {:else if mode === 'qr'}
        <Qr text={qrText} />
        <p style="font-weight:500">Scan this with your other device</p>
        <p class="dim">It works once and stops in two minutes.</p>
        <Field value={qrText} mono label="Pairing code" />
        <div><Btn label="Cancel" onclick={() => lan.stop()} /></div>
      {:else if mode === 'code_wait'}
        <p style="font-weight:500">Waiting for the other device to connect</p>
        <div><Btn label="Cancel" onclick={() => lan.stop()} /></div>
      {:else if mode === 'code_compare'}
        <p class="dim">Do both screens show this?</p>
        <p class="mono" style="font-size:36px;font-weight:600;line-height:1.2">{codeText}</p>
        <p class="dim">{codePeer}</p>
        <div class="gap">
          <Btn label="They differ" variant="danger" onclick={() => finish(false)} />
          <Btn label="They match" variant="primary" onclick={() => finish(true)} />
        </div>
      {:else if lan.peers.length > 0}
        <div class="gap wrap">
          <Btn label={syncing ? 'Looking…' : 'Sync now'} icon="sync" variant="primary" disabled={syncing} onclick={syncNow} />
          <Btn label="Wait for one" onclick={wait} />
        </div>
      {:else}
        <p class="dim">Pair a phone or another computer. Once paired, they sync directly with nothing in between.</p>
      {/if}
      {#if message}<p class="dim">{message}</p>{/if}
    </NetGate>
  </div>
  {#if mode === 'idle' && host().network}
    {#each lan.peers as peer (peer.key)}
      <Row title={peer.name} icon="devices" onclick={() => forget(peer)} />
    {/each}
    <Row title="Pair a device" sub="A phone and this computer, or two computers" icon="add" chevron onclick={() => (adding = true)} />
  {/if}
</Group>

{#if adding}
  <Dialog title="Pair a device" onclose={() => (adding = false)}>
    <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
      <Row title="Show a code" sub="The other device scans it" icon="qr_code_2" chevron onclick={showCode} />
      <Row title="Paste a code" sub="The other device shows it, and you copy its text" icon="content_paste" chevron onclick={() => { adding = false; pasting = true; }} />
      <Row title="Compare six digits" sub="This side waits" icon="pin" chevron onclick={compareWait} />
      <Row title="Compare six digits, finding the other" sub="The other device is already waiting" icon="pin" chevron onclick={compareConnect} />
    </div>
  </Dialog>
{/if}
{#if pasting}
  <Dialog title="Paste a code" onclose={() => (pasting = false)}>
    <Field bind:value={pasted} multiline rows={4} mono placeholder="The other device’s pairing code" autofocus />
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (pasting = false)} />
      <Btn label="Pair" variant="primary" disabled={!pasted.trim()} onclick={usePasted} />
    {/snippet}
  </Dialog>
{/if}
