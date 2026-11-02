<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Strength from '../parts/Strength.svelte';
  import Shield from './Shield.svelte';
  import { pass, bytesOf, consumeText } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import type { NeededFactor, StrengthView } from '../engine/types';
  import { accounts, type AccountRef } from '../settings/accounts.svelte';
  import AddAccount from '../settings/AddAccount.svelte';
  import { hardwareTap, hardwareAvailable } from '../settings/hardware';

  let creating = $state(pass.state.kind === 'none');
  let password = $state('');
  let confirm = $state('');
  let message = $state<string | null>(null);
  let busy = $state(false);
  let refused = $state(false);
  let retryAt = $state(pass.retryAt());
  let now = $state(Date.now());
  let strength = $state<StrengthView | null>(null);
  let needed = $state<NeededFactor[] | null>(null);
  let source = $state<AccountRef | null>(null);
  let accountKeyFile = $state<Uint8Array | null>(null);
  let code = $state('');
  let asksCode = $state(false);
  let returning = $state(false);
  let sheet = $state<'account' | 'shares' | null>(null);
  let problem = $state<string | null>(null);
  let shares = $state('');
  let sharesFile = $state<Uint8Array | null>(null);
  let sharesPassword = $state('');

  const remaining = $derived(Math.max(0, retryAt - now));

  $effect(() => {
    if (retryAt <= Date.now()) return;
    const timer = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(timer);
  });

  function waitText(ms: number) {
    const seconds = Math.max(1, Math.floor(ms / 1000));
    return seconds >= 60 ? `${Math.floor(seconds / 60)} min ${seconds % 60} s` : `${seconds} s`;
  }

  let strengthTimer: ReturnType<typeof setTimeout>;
  function rate() {
    clearTimeout(strengthTimer);
    strengthTimer = setTimeout(async () => {
      if (!password) {
        strength = null;
        return;
      }
      const bytes = bytesOf(password);
      strength = await pass.view<StrengthView>('strength.assess', { context: [] }, { secret: bytes });
      bytes.fill(0);
    }, 120);
  }

  const canCreate = $derived(password.length > 0 && password === confirm && (strength?.score ?? 0) >= 3 && !busy);

  async function create() {
    if (!canCreate) return;
    busy = true;
    message = null;
    const bytes = bytesOf(password);
    try {
      await pass.create('Personal', bytes);
      password = '';
      confirm = '';
    } catch (failure: any) {
      message = failure.message;
    } finally {
      bytes.fill(0);
      busy = false;
    }
  }

  async function suggest() {
    const reply = await pass.engine('gen.generate', { Passphrase: { words: 5, separator: '-', capitalize: false, include_digit: false } });
    password = consumeText(reply.secret);
    confirm = '';
    rate();
  }

  function refuse(text: string) {
    message = text;
    refused = true;
    setTimeout(() => (refused = false), 400);
  }

  async function submitAccount(ref: AccountRef) {
    if (busy || !password || remaining > 0) return;
    busy = true;
    message = null;
    const bytes = bytesOf(password);
    try {
      await accounts.open(ref, bytes, { keyFile: accountKeyFile, code: code.trim() || null });
      password = '';
    } catch (failure: any) {
      if (failure.code === 'two_factor') {
        asksCode = true;
        message = 'This account also needs a code from your authenticator.';
      } else if (failure.code === 'network') {
        message = 'The server could not be reached.';
      } else {
        refuse(failure.message ?? 'Wrong password.');
        retryAt = await pass.noteWrongAccountPassword();
        now = Date.now();
      }
    } finally {
      bytes.fill(0);
      busy = false;
    }
  }

  async function submit(factors: { kind: string; bytes: Uint8Array }[] = []) {
    if (source) return submitAccount(source);
    if (busy || !password || remaining > 0) return;
    busy = true;
    message = null;
    const bytes = bytesOf(password);
    try {
      const outcome = await pass.unlock(bytes, factors);
      switch (outcome.kind) {
        case 'opened':
          password = '';
          needed = null;
          break;
        case 'wrong':
          refuse('Wrong password.');
          retryAt = outcome.retryAtMs;
          now = Date.now();
          break;
        case 'waiting':
          retryAt = outcome.retryAtMs;
          now = Date.now();
          break;
        case 'needs_factor':
          needed = outcome.factors;
          void tapHardware();
          break;
        case 'hard_locked':
          password = '';
          refuse('Wrong password.');
          retryAt = pass.retryAt();
          now = Date.now();
          break;
      }
    } catch (failure: any) {
      message = failure.message ?? 'The vault could not be read.';
    } finally {
      bytes.fill(0);
      factors.forEach((f) => f.bytes.fill(0));
      busy = false;
    }
  }

  async function tapHardware() {
    const hardware = needed?.filter((f) => f.kind !== 'key_file') ?? [];
    if (hardware.length === 0 || !hardwareAvailable()) return;
    for (const factor of hardware) {
      try {
        const output = await hardwareTap(factor);
        await submit([{ kind: factor.kind, bytes: output }]);
        return;
      } catch (failure: any) {
        message = failure.message;
      }
    }
  }

  async function chooseKeyFile() {
    const file = await host().pickFile('Choose the key file');
    if (file) await submit([{ kind: 'key_file', bytes: file.bytes }]);
  }

  async function restoreFile() {
    problem = null;
    const file = await host().pickFile('Choose a vault file', ['wmvault']);
    if (!file) return;
    try {
      await pass.restore(file.bytes);
      creating = false;
      returning = false;
    } catch (failure: any) {
      problem = failure.message ?? 'That file could not be used.';
    }
  }

  async function recover() {
    if (!sharesFile) return;
    busy = true;
    const secret = bytesOf(sharesPassword);
    try {
      await pass.recover(
        sharesFile,
        shares.split('\n').map((s) => s.trim()).filter(Boolean),
        secret,
      );
      sheet = null;
    } catch (failure: any) {
      problem = failure.message;
    } finally {
      secret.fill(0);
      busy = false;
    }
  }
</script>

<div class="door">
  {#if creating}
    <form class="door-card" onsubmit={(e) => { e.preventDefault(); void create(); }}>
      <Shield size={52} />
      <h1>Create your vault</h1>
      <p class="dim">Your master password encrypts everything on this device. It cannot be reset, so keep a copy somewhere safe.</p>
      <div style="height:6px"></div>
      <PasswordField bind:value={password} placeholder="Master password" autofocus disabled={busy} oninput={rate} />
      <Strength score={strength?.score ?? 0} active={password.length > 0} />
      <p class="dim small">
        {#if !password}Four or five random words work well.
        {:else if strength?.warning}{strength.warning}
        {:else if strength}Time to crack: {strength.crack_slow_hash}{/if}
      </p>
      <div><Btn label="Generate a passphrase" variant="quiet" onclick={suggest} disabled={busy} /></div>
      <PasswordField bind:value={confirm} placeholder="Confirm master password" disabled={busy} error={!!confirm && confirm !== password} onenter={create} />
      {#if confirm && confirm !== password}<p class="danger-text small">Passwords do not match.</p>{/if}
      {#if message}<p class="danger-text small">{message}</p>{/if}
      <Btn label={busy ? 'Creating vault…' : 'Create vault'} variant="primary" wide disabled={!canCreate} type="submit" />
      <Btn label="I already have a vault" variant="quiet" wide onclick={() => (returning = true)} />
      {#if pass.state.kind !== 'none' || accounts.list.length > 0}
        <Btn label="Back" variant="quiet" wide onclick={() => (creating = false)} />
      {/if}
    </form>
  {:else}
    <form class="door-card" onsubmit={(e) => { e.preventDefault(); void submit(); }}>
      <div class:shake={refused}><Shield size={52} {refused} /></div>
      <h1>{source?.label ?? 'WM Pass'}</h1>
      <p class:dim={remaining === 0} class:danger-text={remaining > 0}>
        {#if remaining > 0}Too many attempts. Try again in {waitText(remaining)}.
        {:else if busy}Unlocking…
        {:else}Enter your password to unlock.{/if}
      </p>
      <div style="height:6px"></div>
      {#if accounts.list.length > 0}
        <div class="gap wrap">
          {#if pass.state.kind !== 'none'}
            <button type="button" class="chip" aria-pressed={source === null} onclick={() => { source = null; message = null; needed = null; }}>This device</button>
          {:else}
            <button type="button" class="chip" onclick={() => (creating = true)}>New vault</button>
          {/if}
          {#each accounts.list as account}
            <button type="button" class="chip" aria-pressed={source?.id === account.id} onclick={() => { source = account; message = null; needed = null; asksCode = false; accountKeyFile = null; }}>{account.label}</button>
          {/each}
        </div>
      {/if}
      {#if needed?.some((f) => f.kind === 'key_file')}
        <p class="dim">This vault also needs its key file.</p>
        <Btn label="Choose the key file" icon="key" wide onclick={chooseKeyFile} disabled={busy} />
      {/if}
      {#if needed?.some((f) => f.kind !== 'key_file')}
        <p class="dim">
          {hardwareAvailable() ? 'Plug in your security key and touch it.' : 'This vault needs a security key, which this app cannot reach here.'}
        </p>
        {#if hardwareAvailable()}<Btn label="Try the key again" icon="security_key" wide onclick={tapHardware} disabled={busy} />{/if}
      {/if}
      {#if asksCode}<Field bind:value={code} placeholder="Two-factor code" mono />{/if}
      <PasswordField
        bind:value={password}
        placeholder={source ? 'Password' : 'Master password'}
        autofocus
        disabled={remaining > 0 || busy}
        error={message !== null}
        onenter={() => submit()}
      />
      {#if message}<p class="danger-text small">{message}</p>{/if}
      <Btn label={busy ? 'Unlocking…' : 'Unlock'} variant="primary" wide type="submit" disabled={!password || remaining > 0 || busy} />
      {#if source?.kind === 'kdbx'}
        <Btn
          label={accountKeyFile ? 'Key file added' : 'Add key file'}
          variant="quiet"
          wide
          onclick={async () => {
            const file = await host().pickFile('Choose the key file');
            if (file) accountKeyFile = file.bytes;
          }}
        />
      {/if}
      <Btn label="Add another vault or account" variant="quiet" wide onclick={() => (returning = true)} />
    </form>
  {/if}
</div>

{#if returning}
  <Dialog title="Existing vault" onclose={() => (returning = false)}>
    <div class="stack">
      <button type="button" class="set-row" onclick={restoreFile}>
        <span class="text">Restore from a file<span class="sub" style="display:block">A .wmvault file, from a backup or another device</span></span>
      </button>
      <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'account'; }}>
        <span class="text">Open a KeePass file or sign in<span class="sub" style="display:block">KeePass, Bitwarden, Vaultwarden or a WM Pass server</span></span>
      </button>
      <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'shares'; }}>
        <span class="text">Recover with emergency shares<span class="sub" style="display:block">When someone gave you part of their key</span></span>
      </button>
      {#if problem}<p class="danger-text">{problem}</p>{/if}
    </div>
    {#snippet foot()}<Btn label="Close" onclick={() => (returning = false)} />{/snippet}
  </Dialog>
{/if}

{#if sheet === 'account'}
  <AddAccount onclose={() => (sheet = null)} />
{/if}

{#if sheet === 'shares'}
  <Dialog title="Emergency shares" onclose={() => (sheet = null)}>
    <div class="stack">
      <p class="dim">The kit lists how many shares are needed and the agreed waiting period. WM Pass cannot enforce the wait.</p>
      <Btn
        label={sharesFile ? 'Vault copy chosen' : 'Choose a copy of the vault'}
        icon="folder"
        wide
        onclick={async () => {
          const file = await host().pickFile('Choose a copy of the vault');
          if (file) sharesFile = file.bytes;
        }}
      />
      <Field bind:value={shares} multiline rows={5} mono placeholder="Paste the shares, one on each line" />
      <PasswordField bind:value={sharesPassword} placeholder="A new password for it" />
      {#if problem}<p class="danger-text">{problem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label={busy ? 'Opening…' : 'Open it'} variant="primary" disabled={busy || !sharesFile || !shares.trim() || !sharesPassword} onclick={recover} />
    {/snippet}
  </Dialog>
{/if}
