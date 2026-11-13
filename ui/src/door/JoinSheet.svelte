<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Qr from '../parts/Qr.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { lan, type JoinReply } from '../settings/lan.svelte';

  // A device with no vault yet waits here for one that has it. That device connects, the person
  // vouches for the connection with the QR code or six digits, and the sealed vault comes over.
  // It still opens only with its master password.
  let {
    onclose,
    ondone,
    onarriving,
  }: {
    onclose: () => void;
    ondone: (from: string) => void;
    /**
     * Just before the vault is put in place, which may replace the screen this sheet is on. Told
     * null again if putting it in place fails, so the door does not say a vault came that did not.
     */
    onarriving?: (from: string | null) => void;
  } = $props();

  type Phase = 'starting' | 'showing' | 'compare' | 'receiving';
  let phase = $state<Phase>('starting');
  let qrText = $state('');
  let code = $state('');
  let from = $state('');
  let problem = $state<string | null>(null);
  const name = pass.freshVaultName();
  let blobDir = '';
  let finished = false;
  let declining = $state(false);

  $effect(() => {
    void start();
    return () => {
      if (!finished) void lan.stop();
    };
  });

  async function start() {
    problem = null;
    phase = 'starting';
    try {
      await lan.load();
      blobDir = await host().lan!.blobDir(pass.blobFolderFor(name));
      qrText = await lan.joinListen(await host().lan!.localAddress());
      phase = 'showing';
      await arrived(await lan.joinWait(blobDir));
    } catch (failure: any) {
      if (failure?.code !== 'cancelled') problem = failure?.message ?? 'That did not work.';
    }
  }

  async function arrived(reply: JoinReply) {
    if (!reply.done) {
      code = reply.code;
      from = reply.peer.name;
      phase = 'compare';
      return;
    }
    phase = 'receiving';
    // Done before the vault lands: on a device with none, its arrival swaps the door for a new
    // one, and this sheet goes with the old. Leaving must not stop the pairing still under way.
    finished = true;
    onarriving?.(reply.peer.name);
    try {
      await pass.adoptFile(reply.file, name);
    } catch (failure) {
      onarriving?.(null);
      // Nothing arrived, so leaving stops the pairing as before.
      finished = false;
      throw failure;
    }
    await lan.addPeer({ key: reply.peer.key, name: reply.peer.name });
    await lan.stop();
    ondone(reply.peer.name);
  }

  async function answer(accepted: boolean) {
    phase = 'receiving';
    try {
      const reply = await lan.joinFinish(accepted, blobDir);
      await arrived(reply);
    } catch (failure: any) {
      problem = accepted ? (failure?.message ?? 'That did not work.') : 'Stopped. Nothing was sent.';
    }
  }

  function again() {
    declining = false;
    void lan.stop().then(start);
  }
</script>

<Dialog title="Get it from another device" onclose={onclose}>
  <!-- Each step is said as it comes, as the screen changes under someone who cannot see it. -->
  <div class="stack" aria-live="polite">
    {#if problem}
      <p class="danger-text" role="alert">{problem}</p>
      <div><Btn label="Try again" variant="primary" onclick={again} /></div>
    {:else if phase === 'starting'}
      <p class="dim">Getting ready…</p>
    {:else if phase === 'showing'}
      <p>On the device that has your vault, open <strong>Settings, Sync and accounts</strong> and choose <strong>Add a new device</strong>.</p>
      {#if qrText}
        <div style="display:flex;justify-content:center"><Qr text={qrText} /></div>
        <p class="dim">There, scan this code with a phone, or choose <strong>Find it on this network</strong> and compare six digits, or choose <strong>Paste its code</strong> and paste this:</p>
        <Field value={qrText} mono label="Code to paste" />
      {:else}
        <p class="dim">This computer has no network address to show in a code. There, choose <strong>Find it on this network</strong>, then compare six digits on both screens.</p>
      {/if}
      <p class="dim small">Both devices need to be on the same network. The vault arrives encrypted and opens only with its master password.</p>
    {:else if phase === 'compare'}
      <p class="dim">Do both screens show this?</p>
      <p class="mono" style="font-size:36px;font-weight:600;line-height:1.2">{code}</p>
      <p class="dim">{from}</p>
      <div class="gap">
        <Btn label="They differ" variant="danger" onclick={() => { declining = true; void answer(false); }} />
        <Btn label="They match" variant="primary" onclick={() => answer(true)} />
      </div>
    {:else if declining}
      <p class="dim">Stopping…</p>
    {:else}
      <p class="dim">Receiving the vault…</p>
    {/if}
  </div>
  {#snippet foot()}<Btn label="Cancel" onclick={onclose} />{/snippet}
</Dialog>
