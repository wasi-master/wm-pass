<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import { accounts } from './accounts.svelte';

  // Adding an account stores only where it is. Signing in happens at the door.
  let { onclose }: { onclose: () => void } = $props();

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

  async function add() {
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
    if (chosenBytes) await accounts.keepFile(id, chosenBytes);
    await accounts.add({
      id,
      kind: kind === 'kdbx' ? 'kdbx' : 'bitwarden',
      label: label.trim() || fallback,
      server: kind === 'server' ? server.trim() : '',
      region: kind === 'eu' ? 'eu' : '',
      email: email.trim(),
      uri: path,
    });
    onclose();
  }
</script>

<Dialog title="Add an account" {onclose}>
  <div class="stack">
    <Segmented value={kind} {options} onchange={(k) => (kind = k)} />
    {#if kind === 'kdbx'}
      <p class="dim">The file stays where it is and WM Pass writes back to it, keeping one earlier copy beside its own files. Syncthing or a cloud app can carry it.</p>
      <Btn label={chosenName || 'Choose the file'} icon="folder" wide onclick={choose} />
    {:else}
      {#if kind === 'server'}<Field bind:value={server} placeholder="https://vault.example.com" />{/if}
      <Field bind:value={email} placeholder="Email" />
      <p class="dim">This needs the network. Your password is never sent: a key derived from it proves who you are, as with any Bitwarden client.</p>
    {/if}
    <Field bind:value={label} placeholder="A name for the door, such as Work" />
  </div>
  {#snippet foot()}
    <Btn label="Cancel" onclick={onclose} />
    <Btn label="Add" variant="primary" disabled={!ready} onclick={add} />
  {/snippet}
</Dialog>
