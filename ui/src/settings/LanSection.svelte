<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Qr from '../parts/Qr.svelte';
  import Row from '../parts/Row.svelte';
  import ScanDialog from '../parts/ScanDialog.svelte';
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
  /** Set while the digits on show are for sending this vault, not for pairing. */
  let sendingVault = $state(false);
  let joining = $state(false);
  let pasteFor = $state<'pair' | 'join'>('pair');
  /** What a camera scan is for, while the scanner is open. */
  let scanning = $state<'pair' | 'join' | null>(null);
  /** Only the vault in this folder goes to a new device: not an account, not a vault inside it. */
  const canSend = $derived(!!pass.file && pass.account === null && pass.child === null);

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

  function scanned(texts: string[]) {
    const wanted = scanning;
    scanning = null;
    pasted = texts.find((t) => t.startsWith('wmpass-pair:')) ?? texts[0];
    pasteFor = wanted ?? 'pair';
    void usePasted();
  }

  async function usePasted() {
    pasting = false;
    if (pasteFor === 'join') return sendByCode(pasted.trim());
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

  const sent = (name: string) => `Sent to ${name}. Unlock it there with this vault’s master password. The two are paired, so Sync now keeps them together.`;

  async function findNewDevice() {
    joining = false;
    message = null;
    try {
      mode = 'code_wait';
      const found = await lan.joinFind();
      if (!found[0]) throw new Error('No new device is waiting on this network. On it, choose I already have a vault, then Get it from another device.');
      const reply = await lan.joinConnect(found[0]);
      codeText = reply.code;
      codePeer = reply.peer.name;
      sendingVault = true;
      mode = 'code_compare';
    } catch (failure) {
      failed(failure);
    }
  }

  async function sendByCode(text: string) {
    message = null;
    try {
      await pass.flush();
      const peer = await lan.joinScan(text);
      await lan.addPeer({ key: peer.key, name: peer.name });
      message = sent(peer.name);
      pasted = '';
    } catch (failure) {
      failed(failure);
    }
  }

  async function finish(accepted: boolean) {
    if (sendingVault) {
      sendingVault = false;
      try {
        await pass.flush();
        const peer = await lan.joinSend(accepted);
        await lan.addPeer({ key: peer.key, name: peer.name });
        message = sent(peer.name);
      } catch (failure) {
        failed(failure);
      }
      mode = 'idle';
      void lan.stop();
      return;
    }
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

<Group title="Your devices, on this network" help="group.sync.lan" anchor="group:sync.lan">
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
    {#if canSend}
      <Row title="Add a new device" help="action.lan_join" sub="Send this vault to a phone or computer that has none yet" icon="devices" chevron onclick={() => (joining = true)} />
    {/if}
    <Row title="Pair a device" help="action.lan_pair" sub="A phone and this computer, or two computers" icon="add" chevron onclick={() => (adding = true)} />
  {/if}
</Group>

{#if adding}
  <Dialog title="Pair a device" onclose={() => (adding = false)}>
    <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
      <Row title="Show a code" sub="The other device scans it" icon="qr_code_2" chevron onclick={showCode} />
      <Row title="Scan a code" sub="The other device shows it; this computer's camera reads it" icon="qr_code_scanner" chevron onclick={() => { adding = false; scanning = 'pair'; }} />
      <Row title="Paste a code" sub="The other device shows it, and you copy its text" icon="content_paste" chevron onclick={() => { adding = false; pasteFor = 'pair'; pasting = true; }} />
      <Row title="Compare six digits" sub="This side waits" icon="pin" chevron onclick={compareWait} />
      <Row title="Compare six digits, finding the other" sub="The other device is already waiting" icon="pin" chevron onclick={compareConnect} />
    </div>
  </Dialog>
{/if}
{#if joining}
  <Dialog title="Add a new device" onclose={() => (joining = false)}>
    <div class="stack">
      <p class="dim">On the new device, choose <strong>I already have a vault</strong>, then <strong>Get it from another device</strong>. It shows a code and waits.</p>
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        <Row title="Scan its code" sub="With this computer's camera, or from a picture of it" icon="qr_code_scanner" chevron onclick={() => { joining = false; scanning = 'join'; }} />
        <Row title="Find it on this network" sub="Then compare six digits on both screens" icon="pin" chevron onclick={findNewDevice} />
        <Row title="Paste its code" sub="The text under the QR code on the new device" icon="content_paste" chevron onclick={() => { joining = false; pasteFor = 'join'; pasting = true; }} />
      </div>
      <p class="dim small">The vault goes over encrypted and opens there only with its master password.</p>
    </div>
  </Dialog>
{/if}
{#if scanning}
  <ScanDialog
    title={scanning === 'join' ? 'Scan the new device’s code' : 'Scan the other device’s code'}
    onfound={scanned}
    onclose={() => (scanning = null)}
  />
{/if}
{#if pasting}
  <Dialog title="Paste a code" onclose={() => (pasting = false)}>
    <Field bind:value={pasted} multiline rows={4} mono placeholder="The other device’s pairing code" autofocus />
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (pasting = false)} />
      <Btn label={pasteFor === 'join' ? 'Send the vault' : 'Pair'} variant="primary" disabled={!pasted.trim()} onclick={usePasted} />
    {/snippet}
  </Dialog>
{/if}
