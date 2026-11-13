<script module lang="ts">
  // Opened by itself once per page, so locking here leads to the door and stays there.
  let triedDesktop = false;
  // Who a vault just came from. The door is made afresh when the vault lands, so the note is
  // handed across here.
  let joinedFrom: string | null = null;
</script>

<script lang="ts">
  import { help } from '../help/help.svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Strength from '../parts/Strength.svelte';
  import Shield from './Shield.svelte';
  import JoinSheet from './JoinSheet.svelte';
  import DesktopLinkPage from '../settings/DesktopLinkPage.svelte';
  import { pass, bytesOf, consumeText } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { overlays } from '../parts/floating.svelte';
  import type { NeededFactor, StrengthView } from '../engine/types';
  import { accounts, nothingToUnlock, type AccountRef } from '../settings/accounts.svelte';
  import AddAccount from '../settings/AddAccount.svelte';
  import CopiesDialog from '../settings/CopiesDialog.svelte';
  import PlaceDialog from '../settings/PlaceDialog.svelte';
  import { PLACE_KINDS } from '../settings/placeKinds';
  import { places, type Place } from '../settings/places.svelte';
  import { hardwareTap, hardwareAvailable, yubikeyAvailable } from '../settings/hardware';
  import SecurityKeySlot from '../parts/SecurityKeySlot.svelte';

  // A KeePass file or a server account is something to unlock too, with no vault of its own here.
  let creating = $state(nothingToUnlock());
  let password = $state('');
  let confirm = $state('');
  let showNew = $state(false);
  let message = $state<string | null>(null);
  let busy = $state(false);
  let refused = $state(false);
  let retryAt = $state(pass.retryAt());
  let now = $state(Date.now());
  let strength = $state<StrengthView | null>(null);
  let needed = $state<NeededFactor[] | null>(null);
  // An account just added comes chosen, with its password asked for straight away. With no vault
  // here to unlock, the account opened last does.
  let source = $state<AccountRef | null>(accounts.takeSignInNext() ?? (pass.state.kind === 'none' ? accounts.preferred() : null));
  let accountKeyFile = $state<Uint8Array | null>(null);
  // A YubiKey's challenge-response slot, for a KeePass database locked with one. Zero is none.
  let accountSlot = $state<0 | 1 | 2>(0);
  let code = $state('');
  let asksCode = $state(false);
  // What to do next rather than what went wrong, so not shown as an error.
  const EMAILED_CODE = 'Bitwarden has emailed you a code to confirm this device. Enter it with your password.';
  const AUTHENTICATOR_CODE = 'This account also needs a code from your authenticator.';
  const messageIsStep = $derived(message === EMAILED_CODE || message === AUTHENTICATOR_CODE);
  // Bitwarden's check on a device it has not seen before, rather than a two-step code.
  let emailedCode = $state(false);
  let returning = $state(false);
  let sheet = $state<'account' | 'shares' | 'join' | 'link' | 'place' | 'copies' | null>(null);
  /** The backup place a vault is being brought back from. */
  let fromPlace = $state<Place | null>(null);
  /** Said once a vault has come from another device, above the password field. */
  let arrivedFrom = $state<string | null>(joinedFrom);
  joinedFrom = null;
  let problem = $state<string | null>(null);
  let shares = $state('');
  let sharesFile = $state<Uint8Array | null>(null);
  let sharesPassword = $state('');
  let sharesAgain = $state('');
  let sharesProblem = $state<string | null>(null);

  function closeShares() {
    sheet = null;
    // The shares are parts of someone's key: they do not wait at the door once the sheet is closed.
    shares = '';
    sharesPassword = '';
    sharesAgain = '';
    sharesProblem = null;
    sharesTried = false;
    sharesFile = null;
  }

  // A second password is wrong once it cannot become the first however it goes on, or once it is
  // given as it is: not while it is still being typed.
  let confirmTried = $state(false);
  let sharesTried = $state(false);
  const differs = (again: string, first: string, tried: boolean) => again !== '' && again !== first && (tried || !first.startsWith(again));

  const remaining = $derived(Math.max(0, retryAt - now));

  // Connected to the desktop app, its open vault is one choice away, or none on first sight.
  let desktopLinked = $state(false);
  $effect(() => {
    const link = host().link;
    if (!link) return;
    const check = async () => {
      const shown = await link.state().catch(() => null);
      desktopLinked = shown?.status === 'ready';
      if (desktopLinked && !triedDesktop) {
        triedDesktop = true;
        void openDesktop(true);
      }
    };
    void check();
    return link.onChange(() => void check());
  });

  async function openDesktop(quietly = false) {
    if (busy) return;
    busy = true;
    message = null;
    try {
      if (!(await pass.openDesktop()) && !quietly) message = 'The vault in the WM Pass app is locked. Unlock it there first.';
    } catch (failure: any) {
      if (!quietly) message = failure.message;
    }
    busy = false;
  }

  // Offered only where the user turned it on, for a vault they enrolled.
  let quick = $state(false);
  $effect(() => {
    if (!creating) void pass.quickFiles().then((files) => (quick = files.length > 0), () => (quick = false));
  });

  async function unlockQuick() {
    busy = true;
    message = null;
    try {
      if (!(await pass.unlockQuick())) quick = false;
    } catch (failure: any) {
      message = failure.message;
      quick = false;
    }
    busy = false;
  }

  $effect(() => {
    if (retryAt <= Date.now()) return;
    const timer = setInterval(() => {
      now = Date.now();
      // Done counting once the wait is over, rather than drawing the door four times a second.
      if (now >= retryAt) clearInterval(timer);
    }, 250);
    return () => clearInterval(timer);
  });

  function waitText(ms: number) {
    const seconds = Math.max(1, Math.floor(ms / 1000));
    // "15 min", not "15 min 0 s".
    return seconds >= 60 ? `${Math.floor(seconds / 60)} min${seconds % 60 ? ` ${seconds % 60} s` : ''}` : `${seconds} s`;
  }

  let strengthTimer: ReturnType<typeof setTimeout>;
  /** What the rating shown is for: an edit after it waits for its own before a vault can be made. */
  let ratedFor = $state('');
  function rate() {
    clearTimeout(strengthTimer);
    strengthTimer = setTimeout(async () => {
      const typed = password;
      if (!typed) {
        strength = null;
        return;
      }
      const bytes = bytesOf(typed);
      try {
        const rated = await pass.view<StrengthView>('strength.assess', { context: [] }, { secret: bytes });
        // A rating for what was in the box a moment ago must not stand for what is there now.
        if (password === typed) {
          strength = rated;
          ratedFor = typed;
        }
      } catch {
        if (password === typed) strength = null;
      } finally {
        bytes.fill(0);
      }
    }, 120);
  }

  const canCreate = $derived(password.length > 0 && password === confirm && ratedFor === password && (strength?.score ?? 0) >= 3 && !busy);

  async function create() {
    confirmTried = true;
    if (!canCreate) return;
    busy = true;
    message = null;
    const bytes = bytesOf(password);
    try {
      await pass.create('Personal', bytes);
      password = '';
      confirm = '';
      ratedFor = '';
    } catch (failure: any) {
      message = failure.message;
    } finally {
      bytes.fill(0);
      busy = false;
    }
  }

  async function suggest() {
    try {
      const reply = await pass.engine('gen.generate', { Passphrase: { words: 5, separator: '-', capitalize: false, include_digit: false } });
      password = consumeText(reply.secret);
      confirm = '';
      // Shown, so the words can be written down and typed again below.
      showNew = true;
      rate();
    } catch (failure: any) {
      message = failure?.message ?? 'A passphrase could not be made.';
    }
  }

  /** Between creating a vault and unlocking one. What was typed or said for one is not for the other. */
  function switchTo(create: boolean) {
    creating = create;
    message = null;
    password = '';
    confirm = '';
    confirmTried = false;
    strength = null;
    ratedFor = '';
  }

  function refuse(text: string) {
    message = text;
    refused = true;
    setTimeout(() => (refused = false), 400);
  }

  /** Between this device's vault and an account. A password typed for one is not sent to another. */
  function chooseSource(next: AccountRef | null) {
    if (busy) return;
    if ((source?.id ?? null) !== (next?.id ?? null)) {
      source = next;
      password = '';
      asksCode = false;
      code = '';
      emailedCode = false;
      accountKeyFile = null;
      accountSlot = 0;
    }
    message = null;
    needed = null;
  }

  // A KeePass database may be locked by its key file or security key alone, with no password.
  const keyFileOnly = $derived(source?.kind === 'kdbx' && (accountKeyFile !== null || accountSlot !== 0));

  /** The engine's words as a sentence: it writes "the password was refused", not a line to show. */
  function sentence(text: string) {
    const first = text.charAt(0).toUpperCase() + text.slice(1);
    return /[.!?]$/.test(first) ? first : `${first}.`;
  }

  async function submitAccount(ref: AccountRef) {
    if (busy || (!password && !keyFileOnly) || remaining > 0) return;
    busy = true;
    message = null;
    const bytes = bytesOf(password);
    try {
      await accounts.open(ref, bytes, { keyFile: accountKeyFile, slot: accountSlot, code: code.trim() || null, deviceCode: emailedCode });
      password = '';
    } catch (failure: any) {
      if (failure.code === 'two_factor' || failure.code === 'new_device') {
        asksCode = true;
        emailedCode = failure.code === 'new_device';
        code = '';
        message = emailedCode ? EMAILED_CODE : AUTHENTICATOR_CODE;
      } else if (failure.code === 'bad_code') {
        // Not a guess at the password, and neither are the two below.
        message = 'That code was not accepted.';
      } else if (failure.code === 'network') {
        message = 'The server could not be reached.';
      } else if (failure.code === 'bitwarden') {
        message = failure.message ? sentence(failure.message) : 'The server refused the sign-in.';
      } else if (ref.kind === 'bitwarden' && !ref.server) {
        // Which account and which server were tried: an account made on bitwarden.eu is unknown to
        // bitwarden.com, which answers just as it does to a wrong password.
        const here = ref.region === 'eu' ? 'bitwarden.eu' : 'bitwarden.com';
        const other = ref.region === 'eu' ? 'bitwarden.com' : 'bitwarden.eu';
        refuse(`${here} did not accept the password for ${ref.email}. If the account is on ${other}, add it again with that choice.`);
        retryAt = await pass.noteWrongAccountPassword().catch(() => pass.retryAt());
        now = Date.now();
      } else {
        refuse(failure.message ? sentence(failure.message) : 'Wrong password.');
        retryAt = await pass.noteWrongAccountPassword().catch(() => pass.retryAt());
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

  /** A file chosen from disk, or null with the reason said where `say` puts it. */
  async function pick(title: string, say: (text: string) => void) {
    try {
      return await host().pickFile(title);
    } catch (failure: any) {
      say(failure?.message ?? 'That file could not be read.');
      return null;
    }
  }

  async function chooseKeyFile() {
    if (busy) return;
    const file = await pick('Choose the key file', (text) => (message = text));
    if (file) await submit([{ kind: 'key_file', bytes: file.bytes }]);
  }

  async function restoreFile() {
    if (busy) return;
    problem = null;
    busy = true;
    try {
      const file = await host().pickFile('Choose a vault file or backup', ['wmvault', 'wmbackup']);
      if (!file) return;
      const { held } = await pass.restore(file.bytes);
      creating = false;
      returning = false;
      if (held) overlays.toast(HELD_NOTE, { ms: 10_000 });
    } catch (failure: any) {
      problem = failure.message ?? 'That file could not be used.';
    } finally {
      busy = false;
    }
  }

  /** What this build can restore from, named briefly: the first few kinds it offers. */
  function placeKindsText() {
    const offered = new Set(host().places?.kinds ?? []);
    const names = PLACE_KINDS.filter((k) => offered.has(k.id)).map((k) => k.name);
    return names.length > 4 ? `${names.slice(0, 4).join(', ')} and more` : names.join(', ');
  }

  function openPlace(place: Place) {
    returning = false;
    fromPlace = place;
    sheet = 'copies';
  }

  // An older backup must not replace newer items on this device, so it waits to be merged in.
  const HELD_NOTE = 'This device has a vault already. The backup merges into it when you unlock it, and newer items here are kept.';

  async function restorePlaceCopy(bytes: Uint8Array) {
    const { held } = await pass.restore(bytes);
    if (held) overlays.toast(HELD_NOTE, { ms: 10_000 });
    sheet = null;
    fromPlace = null;
    creating = false;
  }

  async function recover() {
    sharesTried = true;
    if (!sharesFile || busy || !shares.trim() || !sharesPassword || sharesPassword !== sharesAgain) return;
    busy = true;
    sharesProblem = null;
    const secret = bytesOf(sharesPassword);
    try {
      // The same bar as a new vault's password: this one replaces it for good.
      const probe = bytesOf(sharesPassword);
      const rated = await pass.view<StrengthView>('strength.assess', { context: [] }, { secret: probe }).finally(() => probe.fill(0));
      if (rated.score < 3) {
        sharesProblem = rated.warning || 'That password is too easy to guess. Add a word or two.';
        return;
      }
      await pass.recover(
        sharesFile,
        shares.split('\n').map((s) => s.trim()).filter(Boolean),
        secret,
      );
      closeShares();
    } catch (failure: any) {
      sharesProblem = failure.message;
    } finally {
      secret.fill(0);
      busy = false;
    }
  }
</script>

<main class="door">
  {#if creating}
    <form class="door-card" onsubmit={(e) => { e.preventDefault(); void create(); }}>
      <Shield size={52} />
      <h1>Create your vault</h1>
      <p class="dim">Your master password encrypts everything on this device. It cannot be reset, so keep a copy somewhere safe. <button type="button" class="link" onclick={() => help.open('concept.master_password')}>What makes a good one?</button></p>
      <div style="height:6px"></div>
      <PasswordField bind:value={password} bind:showing={showNew} placeholder="Master password" describedby="new-password-rating" autofocus disabled={busy} oninput={rate} />
      <Strength score={strength?.score ?? 0} active={password.length > 0} />
      <p class="dim small" id="new-password-rating">
        {#if !password}Four or five random words work well.
        {:else if strength?.warning}{strength.warning}
        {:else if strength}Time to crack: {strength.crack_slow_hash}{/if}
        {#if password && strength && strength.score < 3} Too easy to guess for a vault yet: add a word or two.{/if}
      </p>
      <div><Btn label="Generate a passphrase" variant="quiet" onclick={suggest} disabled={busy} /></div>
      <PasswordField bind:value={confirm} placeholder="Confirm master password" disabled={busy} error={differs(confirm, password, confirmTried)} describedby={differs(confirm, password, confirmTried) ? 'confirm-problem' : undefined} onenter={create} />
      {#if differs(confirm, password, confirmTried)}<p class="danger-text small" role="alert" id="confirm-problem">Passwords do not match.</p>{/if}
      {#if message}<p class="danger-text small" role="alert">{message}</p>{/if}
      <Btn label={busy ? 'Creating vault…' : 'Create vault'} variant="primary" wide disabled={!canCreate} type="submit" />
      {#if desktopLinked}<Btn label="Use the vault open in WM Pass" icon="computer" wide onclick={() => openDesktop()} disabled={busy} />{/if}
      <Btn label="I already have a vault" variant="quiet" wide onclick={() => (returning = true)} />
      {#if !nothingToUnlock()}
        <Btn label="Back" variant="quiet" wide onclick={() => switchTo(false)} />
      {/if}
    </form>
  {:else}
    <form class="door-card" onsubmit={(e) => { e.preventDefault(); void submit(); }}>
      <div class:shake={refused}><Shield size={52} {refused} /></div>
      <h1>{source?.label ?? 'WM Pass'}</h1>
      <p class:dim={remaining === 0} class:danger-text={remaining > 0} role="status">
        {#if remaining > 0}Too many attempts. Try again in {waitText(remaining)}.
        {:else if busy}Unlocking…
        {:else}Enter your password to unlock.{/if}
      </p>
      <div style="height:6px"></div>
      {#if accounts.list.length > 0}
        <div class="gap wrap" role="group" aria-label="Vault to unlock">
          {#if pass.state.kind !== 'none'}
            <button type="button" class="chip" aria-pressed={source === null} onclick={() => chooseSource(null)}>This device</button>
          {:else}
            <button type="button" class="chip" onclick={() => switchTo(true)}>New vault</button>
          {/if}
          {#each accounts.list as account (account.id)}
            <button type="button" class="chip" aria-pressed={source?.id === account.id} onclick={() => chooseSource(account)}>{account.label}</button>
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
      {#if asksCode}<Field bind:value={code} placeholder={emailedCode ? 'Code from the email' : 'Two-factor code'} mono autofocus disabled={busy} onenter={() => submit()} />{/if}
      <!-- The code box takes the focus once it is asked for, or the digits would replace the password. -->
      <PasswordField
        bind:value={password}
        placeholder={source ? (keyFileOnly ? 'Password, if it has one' : 'Password') : 'Master password'}
        autofocus={!asksCode}
        disabled={remaining > 0 || busy}
        error={message !== null && !messageIsStep}
        describedby={message ? 'unlock-message' : undefined}
        onenter={() => submit()}
      />
      {#if message}<p class={messageIsStep ? 'dim small' : 'danger-text small'} role={messageIsStep ? 'status' : 'alert'} id="unlock-message">{message}</p>{/if}
      {#if arrivedFrom && !message}<p class="dim small">Your vault came from {arrivedFrom}. Unlock it with its master password. The two are paired, so they can sync on this network.</p>{/if}
      {#if quick}
        <Btn label={host().quick?.title ?? 'Quick unlock'} wide onclick={unlockQuick} disabled={busy} />
      {/if}
      <Btn label={busy ? 'Unlocking…' : 'Unlock'} variant="primary" wide type="submit" disabled={(!password && !keyFileOnly) || remaining > 0 || busy} />
      {#if desktopLinked}<Btn label="Use the vault open in WM Pass" icon="computer" wide onclick={() => openDesktop()} disabled={busy} />{/if}
      {#if source?.kind === 'kdbx'}
        <Btn
          label={accountKeyFile ? 'Key file added' : 'Add key file'}
          variant="quiet"
          wide
          onclick={async () => {
            const file = await pick('Choose the key file', (text) => (message = text));
            if (file) accountKeyFile = file.bytes;
          }}
        />
        {#if yubikeyAvailable()}<SecurityKeySlot bind:slot={accountSlot} />{/if}
      {/if}
      <Btn label="Add another vault or account" variant="quiet" wide onclick={() => (returning = true)} />
      <p class="small" style="text-align:center"><button type="button" class="link" onclick={() => help.open('screen.door')}>Help with unlocking</button></p>
    </form>
  {/if}
</main>

{#if returning}
  <Dialog title="Existing vault" onclose={() => (returning = false)}>
    <div class="stack">
      {#if host().link}
        <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'link'; }}>
          <span class="text">Use the vault in the desktop app<span class="sub" style="display:block">WM Pass on this computer lends this browser its vault</span></span>
        </button>
      {/if}
      {#if host().lan}
        <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'join'; }}>
          <span class="text">Get it from another device<span class="sub" style="display:block">Your phone or another computer sends it over this network</span></span>
        </button>
      {/if}
      <button type="button" class="set-row" onclick={restoreFile} disabled={busy}>
        <span class="text">{busy ? 'Restoring…' : 'Restore from a file'}<span class="sub" style="display:block">A .wmvault or .wmbackup file, from a backup or another device</span></span>
      </button>
      {#if host().places}
        {#each places.list as place (place.id)}
          <button type="button" class="set-row" onclick={() => openPlace(place)}>
            <span class="text">Restore from {place.label}<span class="sub" style="display:block">The newest copy kept there, or an older one</span></span>
          </button>
        {/each}
        <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'place'; }}>
          <span class="text">Restore from a backup location<span class="sub" style="display:block">{placeKindsText()}</span></span>
        </button>
      {/if}
      <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'account'; }}>
        <span class="text">Open a KeePass file or sign in<span class="sub" style="display:block">KeePass, Bitwarden, Vaultwarden or a WM Pass server</span></span>
      </button>
      <button type="button" class="set-row" onclick={() => { returning = false; sheet = 'shares'; }}>
        <span class="text">Recover with emergency shares<span class="sub" style="display:block">When someone gave you part of their key</span></span>
      </button>
      {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
    </div>
    {#snippet foot()}<Btn label="Close" onclick={() => (returning = false)} />{/snippet}
  </Dialog>
{/if}

{#if sheet === 'link'}
  <Dialog title="Use the vault in the desktop app" onclose={() => (sheet = null)}>
    <p class="dim" style="padding-bottom:10px">A browser cannot reach your other devices itself, so the WM Pass app on this computer lends it the vault it has open. In the app, open <strong>Settings, Browser extension</strong> and turn on <strong>Let the extension connect</strong>, then choose <strong>Connect</strong> here. To get the vault onto this computer first, use <strong>Get it from another device</strong> in the app.</p>
    <DesktopLinkPage />
    {#snippet foot()}<Btn label="Done" onclick={() => { sheet = null; if (desktopLinked) void openDesktop(); }} />{/snippet}
  </Dialog>
{/if}
{#if sheet === 'join'}
  <JoinSheet
    onclose={() => (sheet = null)}
    onarriving={(from) => (joinedFrom = from)}
    ondone={(from) => {
      sheet = null;
      creating = false;
      arrivedFrom = from;
      joinedFrom = null;
    }}
  />
{/if}
{#if sheet === 'account'}
  <AddAccount
    onclose={() => (sheet = null)}
    onadded={(ref) => {
      creating = false;
      source = ref;
      password = '';
      message = null;
      needed = null;
      asksCode = false;
      accountKeyFile = null;
      accountSlot = 0;
    }}
  />
{/if}

{#if sheet === 'place'}
  <PlaceDialog
    onclose={() => {
      if (sheet === 'place') sheet = null;
    }}
    onsaved={(place) => {
      fromPlace = place;
      sheet = 'copies';
    }}
  />
{/if}
{#if sheet === 'copies' && fromPlace}
  <CopiesDialog place={fromPlace} onclose={() => { sheet = null; fromPlace = null; }} onchoose={restorePlaceCopy} />
{/if}

{#if sheet === 'shares'}
  <Dialog title="Emergency shares" onclose={closeShares}>
    <div class="stack">
      <p class="dim">The emergency kit says how many shares open the vault. Choose a copy of the vault file, paste at least that many shares, and give it a new password. Nothing makes you wait: enough shares open it at once.</p>
      <Btn
        label={sharesFile ? 'Vault copy chosen' : 'Choose a copy of the vault'}
        icon="folder"
        wide
        onclick={async () => {
          const file = await pick('Choose a copy of the vault', (text) => (sharesProblem = text));
          if (file) sharesFile = file.bytes;
        }}
      />
      <Field bind:value={shares} multiline rows={5} mono placeholder="Paste the shares, one on each line" />
      <PasswordField bind:value={sharesPassword} placeholder="A new password for it" />
      <PasswordField bind:value={sharesAgain} placeholder="The new password again" error={differs(sharesAgain, sharesPassword, sharesTried)} describedby={differs(sharesAgain, sharesPassword, sharesTried) ? 'shares-problem' : undefined} onenter={recover} />
      {#if differs(sharesAgain, sharesPassword, sharesTried)}<p class="danger-text small" role="alert" id="shares-problem">Passwords do not match.</p>{/if}
      {#if sharesProblem}<p class="danger-text" role="alert">{sharesProblem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={closeShares} />
      <Btn label={busy ? 'Opening…' : 'Open it'} variant="primary" disabled={busy || !sharesFile || !shares.trim() || !sharesPassword || sharesPassword !== sharesAgain} onclick={recover} />
    {/snippet}
  </Dialog>
{/if}
