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
  import { lan, shown, type LanPeer } from './lan.svelte';
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
  /** The paired device whose sheet is open. */
  let open = $state<LanPeer | null>(null);
  let nickname = $state('');
  /** Choosing which vault stays, for the device in `open`. */
  let mergeFor = $state<LanPeer | null>(null);
  let busy = $state(false);

  $effect(() => {
    void lan.load();
    return () => void lan.stop();
  });

  // Pairing and sending the vault advertise this computer for a moment; the background wait
  // steps aside meanwhile.
  $effect(() => {
    const busy = mode === 'qr' || mode === 'code_wait' || mode === 'code_compare' || mode === 'waiting';
    lan.pause(busy);
  });
  $effect(() => () => lan.pause(false));

  function failed(failure: any) {
    mode = 'idle';
    if (failure?.code === 'cancelled') return;
    message = failure?.message ?? 'That did not work.';
  }

  function when(at: number) {
    const minutes = Math.round((Date.now() - at) / 60_000);
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 24) return `${hours} h ago`;
    return new Date(at).toLocaleDateString();
  }

  function status(peer: LanPeer) {
    if (!peer.last) return 'Not synced yet';
    return `${peer.last.ok ? '' : 'Failed: '}${peer.last.text} ${when(peer.last.at)}`;
  }

  const paired = 'Paired. Choose Sync now to bring them together.';

  async function syncNow() {
    syncing = true;
    message = null;
    try {
      const results = await lan.sync();
      message = results.map((r) => `${shown(r.peer)}: ${r.text}`).join('\n');
    } catch (failure) {
      failed(failure);
    }
    syncing = false;
  }

  async function wait() {
    mode = 'waiting';
    message = null;
    try {
      message = await lan.serve();
    } catch (failure) {
      failed(failure);
    }
    mode = 'idle';
  }

  async function showCode() {
    adding = false;
    message = null;
    try {
      const address = await host().lan!.localAddress();
      if (!address) {
        message = 'This computer is not on a network.';
        return;
      }
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
      // Stop advertising, as Show a code does: nobody is coming for these digits now.
      void lan.stop();
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

  // An answer goes once: a second press while the first is on its way would answer again.
  let finishing = $state(false);

  async function finish(accepted: boolean) {
    if (finishing) return;
    finishing = true;
    try {
      await finishOnce(accepted);
    } finally {
      finishing = false;
    }
  }

  async function finishOnce(accepted: boolean) {
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

  function openPeer(peer: LanPeer) {
    open = peer;
    nickname = shown(peer);
  }

  async function saveName() {
    if (!open) return;
    const name = nickname.trim();
    try {
      await lan.rename(open.key, name === open.name ? '' : name);
      open = null;
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The name could not be saved.');
    }
  }

  async function syncOne(peer: LanPeer) {
    open = null;
    syncing = true;
    message = null;
    try {
      message = (await lan.sync(peer.key)).map((r) => `${shown(r.peer)}: ${r.text}`).join('\n');
    } catch (failure) {
      failed(failure);
    }
    syncing = false;
  }

  async function merge(keepMine: boolean) {
    const peer = mergeFor;
    mergeFor = null;
    if (!peer) return;
    if (!keepMine) {
      const yes = await overlays.ask({
        title: `Use ${shown(peer)}’s vault here?`,
        body: `This computer’s items move into ${shown(peer)}’s vault, then this computer uses that vault in place of its own. You unlock it with that vault’s master password from then on.`,
        action: 'Merge',
        danger: true,
      });
      if (!yes) return;
    }
    busy = true;
    message = null;
    try {
      message = await lan.merge(peer.key, keepMine);
    } catch (failure) {
      failed(failure);
    }
    busy = false;
  }

  async function forget(peer: LanPeer) {
    open = null;
    const yes = await overlays.ask({ title: `Forget ${shown(peer)}?`, body: 'It will need pairing again before it can sync with this computer.', action: 'Forget', danger: true });
    if (!yes) return;
    await lan.removePeer(peer.key).catch((failure: any) => overlays.toast(failure?.message ?? 'It could not be forgotten.'));
  }
</script>

<Group title="Your devices, on this network" help="group.sync.lan" anchor="group:sync.lan">
  <!-- Waiting, comparing, merging and how it went are said as they change. -->
  <div class="set-body stack" aria-live="polite">
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
          <Btn label="They differ" variant="danger" disabled={finishing} onclick={() => finish(false)} />
          <Btn label="They match" variant="primary" disabled={finishing} onclick={() => finish(true)} />
        </div>
      {:else if lan.merging}
        <p style="font-weight:500">Merging with {lan.merging}</p>
        <p class="dim">If it asks, accept on {lan.merging}. Keep both devices open until this is done.</p>
      {:else if lan.peers.length > 0}
        <div class="gap wrap">
          <Btn label={syncing ? 'Looking…' : 'Sync now'} icon="sync" variant="primary" disabled={syncing || busy} onclick={syncNow} />
          {#if !lan.listening}<Btn label="Wait for one" onclick={wait} />{/if}
        </div>
        {#if lan.listening}
          <p class="dim small">While this vault is open, this computer waits for your devices by itself. On your phone, choose Sync now.</p>
        {/if}
      {:else}
        <p class="dim">Pair a phone or another computer. Once paired, they sync directly with nothing in between.</p>
      {/if}
      {#if message}<p class="dim" style="white-space:pre-line">{message}</p>{/if}
    </NetGate>
  </div>
  {#if mode === 'idle' && host().network}
    {#each lan.peers as peer (peer.key)}
      <Row title={shown(peer)} sub={status(peer)} icon={peer.last && !peer.last.ok ? 'warning' : 'devices'} danger={!!peer.last && !peer.last.ok} chevron onclick={() => openPeer(peer)} />
    {/each}
    {#if canSend}
      <Row title="Add a new device" help="action.lan_join" sub="Send this vault to a phone or computer that has none yet" icon="devices" chevron onclick={() => (joining = true)} />
    {/if}
    <Row title="Pair a device" help="action.lan_pair" sub="A phone and this computer, or two computers" icon="add" chevron onclick={() => (adding = true)} />
  {/if}
</Group>

{#if open}
  {@const peer = open}
  <Dialog title={shown(peer)} onclose={() => (open = null)}>
    <div class="stack">
      <Field bind:value={nickname} label="Name on this computer" placeholder={peer.name} />
      {#if nickname.trim() && nickname.trim() !== shown(peer)}<div><Btn label="Save name" variant="primary" onclick={saveName} /></div>{/if}
      <p class="dim small">It calls itself {peer.name}. {status(peer)}.</p>
      {#if peer.last?.code === 'not_this_vault'}
        <p class="dim">The two hold different vaults, so they cannot sync. Merge them into one: you choose which vault stays, and the other device’s items move into it.</p>
      {/if}
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        <Row title="Sync with it" icon="sync" onclick={() => syncOne(peer)} />
        <Row title="Merge vaults" help="action.lan_merge" sub="Make two different vaults one, and keep it in step from then on" icon="swap_horiz" chevron onclick={() => { open = null; mergeFor = peer; }} />
        <Row title="Forget it" icon="delete" danger onclick={() => forget(peer)} />
      </div>
    </div>
  </Dialog>
{/if}
{#if mergeFor}
  {@const peer = mergeFor}
  <Dialog title="Which vault stays?" onclose={() => (mergeFor = null)}>
    <div class="stack">
      <p class="dim">The vault that stays takes every item from the other, and both devices use it from then on. Items both have, such as the same login, are merged rather than doubled.</p>
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        <Row title="Keep this computer’s vault" sub={`${shown(peer)}’s items move here, then it uses this vault and its master password`} icon="lock" chevron onclick={() => merge(true)} />
        <Row title={`Keep ${shown(peer)}’s vault`} sub="This computer’s items move there, then this computer uses that vault" icon="devices" chevron onclick={() => merge(false)} />
      </div>
      <p class="dim small">First, on {shown(peer)}, open Sync and accounts and choose Wait for one. It asks there before anything moves.</p>
    </div>
  </Dialog>
{/if}
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
    <Field bind:value={pasted} multiline rows={4} mono placeholder={pasteFor === 'join' ? 'The new device’s code' : 'The other device’s pairing code'} autofocus />
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (pasting = false)} />
      <Btn label={pasteFor === 'join' ? 'Send the vault' : 'Pair'} variant="primary" disabled={!pasted.trim()} onclick={usePasted} />
    {/snippet}
  </Dialog>
{/if}
