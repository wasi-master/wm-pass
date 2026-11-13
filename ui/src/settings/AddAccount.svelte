<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { accounts, type AccountRef } from './accounts.svelte';
  import { online } from './net.svelte';

  // Adding an account stores only where it is. Signing in happens at the door, since an account
  // opens in place of the vault: added from Settings, it offers to lock this vault and go there.
  // Added from the door, `onadded` has the door choose it.
  let { onclose, onadded }: { onclose: () => void; onadded?: (ref: AccountRef) => void } = $props();

  let added = $state<AccountRef | null>(null);

  let kind = $state<'bitwarden' | 'eu' | 'server' | 'kdbx'>(host().network ? 'bitwarden' : 'kdbx');
  let server = $state('');
  let email = $state('');
  let label = $state('');
  let path = $state('');
  let chosenName = $state('');
  let chosenBytes = $state<Uint8Array | null>(null);

  const options = $derived<[typeof kind, string][]>(
    host().network
      ? [['bitwarden', 'Bitwarden'], ['eu', 'Bitwarden EU'], ['server', 'Own server'], ['kdbx', 'KeePass file']]
      : [['kdbx', 'KeePass file']],
  );

  async function choose() {
    problem = null;
    try {
      await chooseFile();
    } catch (failure: any) {
      problem = failure?.message ?? 'That file could not be chosen.';
    }
  }

  async function chooseFile() {
    const outside = host().outside;
    if (outside) {
      const file = await outside.choose('Choose the KeePass file', ['kdbx']);
      if (file) {
        path = file.path;
        chosenName = file.name;
      }
      return;
    }
    const file = await host().pickFile('Choose the KeePass file', ['kdbx']);
    if (file) {
      chosenBytes = file.bytes;
      chosenName = file.name;
    }
  }

  const ready = $derived(kind === 'kdbx' ? !!chosenName : !!email.trim() && (kind !== 'server' || !!server.trim()));

  let saving = $state(false);
  let checking = $state(false);

  /** The engine's words as a sentence: "that address does not answer…" reads as one. */
  function sentence(text: string | undefined) {
    if (!text) return undefined;
    const first = text.charAt(0).toUpperCase() + text.slice(1);
    return /[.!?]$/.test(first) ? first : `${first}.`;
  }
  let problem = $state<string | null>(null);

  async function add() {
    // A double click would add the same account twice.
    if (saving) return;
    saving = true;
    let fallback = 'Bitwarden';
    if (kind === 'kdbx') fallback = 'KeePass';
    if (kind === 'server') {
      try {
        fallback = new URL(server.trim()).host;
      } catch {
        fallback = 'Server';
      }
    }
    if (kind !== 'kdbx') prefs.set('network_allowed', true);
    const id = accounts.newId();
    const ref: AccountRef = {
      id,
      kind: kind === 'kdbx' ? 'kdbx' : 'bitwarden',
      label: label.trim() || fallback,
      server: kind === 'server' ? server.trim() : '',
      region: kind === 'eu' ? 'eu' : '',
      email: email.trim(),
      uri: path,
    };
    problem = null;
    try {
      // A server account is kept only once its address answers as a Bitwarden server does, as on
      // the phone: a typo is said here rather than at the door, after the password.
      if (kind !== 'kdbx') {
        checking = true;
        try {
          await pass.net('bitwarden.check', online(), { server: ref.server || null, region: ref.region || null, email: ref.email });
        } finally {
          checking = false;
        }
      }
      if (chosenBytes) await accounts.keepFile(id, chosenBytes);
      await accounts.add(ref);
    } catch (failure: any) {
      problem = sentence(failure?.message) ?? 'The account could not be added.';
      return;
    } finally {
      saving = false;
    }
    if (pass.state.kind === 'unlocked' && !onadded) {
      added = ref;
      return;
    }
    if (onadded) onadded(ref);
    else accounts.signInNext(ref.id);
    onclose();
  }

  function signInNow() {
    if (!added) return;
    accounts.signInNext(added.id);
    onclose();
    void pass.lock().catch(() => undefined);
  }
</script>

{#if added}
  <Dialog title="Sign in to {added.label}" {onclose}>
    <p class="dim">An account opens in place of this vault. Signing in locks this one, and the unlock screen asks for the account's password.</p>
    {#snippet foot()}
      <Btn label="Later" onclick={onclose} />
      <Btn label="Sign in now" variant="primary" onclick={signInNow} />
    {/snippet}
  </Dialog>
{:else}
  <Dialog title="Add an account" {onclose}>
    <div class="stack">
      <Segmented label="Kind of account" value={kind} {options} onchange={(k) => (kind = k)} />
      {#if kind === 'kdbx'}
        <p class="dim">The file stays where it is and WM Pass writes back to it, keeping one earlier copy beside its own files. Syncthing or a cloud app can carry it.</p>
        <Btn label={chosenName || 'Choose the file'} icon="folder" wide onclick={choose} />
      {:else}
        {#if kind === 'server'}<Field bind:value={server} placeholder="https://vault.example.com" />{/if}
        <Field bind:value={email} placeholder="Email" />
        <p class="dim">This needs the network. Your password is never sent: a key derived from it proves who you are, as with any Bitwarden client.</p>
      {/if}
      <Field bind:value={label} placeholder="A name for the door, such as Work" />
      {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={onclose} />
      <Btn label={checking ? 'Checking the server…' : 'Add'} variant="primary" disabled={!ready || saving} onclick={add} />
    {/snippet}
  </Dialog>
{/if}
