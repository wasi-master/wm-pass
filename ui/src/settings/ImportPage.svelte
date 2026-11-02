<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Icon from '../parts/Icon.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import type { ImportPreview } from '../engine/types';

  const FORMATS = '1Password, Apple Passwords, Bitwarden, Chrome, Dashlane, Enpass, Firefox, KeePass and KeePassXC, Keeper, LastPass, NordPass, Proton Pass, RoboForm and SafeInCloud. Authenticators: Aegis, 2FAS, andOTP, Ente, FreeOTP+, Raivo, and Google’s export QR code.';

  let preview = $state<ImportPreview | null>(null);
  let bytes = $state<Uint8Array | null>(null);
  let needsPassword = $state(false);
  let password = $state('');
  let error = $state<string | null>(null);
  let result = $state<string | null>(null);

  async function read() {
    if (!bytes) return;
    const secret = bytesOf(password);
    try {
      preview = (await pass.engine<ImportPreview>('import.read', {}, { handle: pass.handle, secret, blobs: [bytes] })).json;
      needsPassword = false;
      error = null;
    } catch (failure: any) {
      needsPassword = failure.code === 'needs_password' || failure.code === 'wrong_password' || /password/i.test(failure.message ?? '');
      error = needsPassword && !password ? null : failure.message;
    } finally {
      secret.fill(0);
    }
  }

  async function pick() {
    const file = await host().pickFile('Choose a file to import');
    if (!file) return;
    bytes = file.bytes;
    preview = null;
    result = null;
    password = '';
    await read();
  }

  async function apply() {
    const reply = await pass.change<{ added: number; duplicates: number }>('import.apply');
    result = `Added ${reply.json.added}. Skipped ${reply.json.duplicates} already here.`;
    bytes?.fill(0);
    bytes = null;
    preview = null;
  }

  async function another() {
    await pass.engine('import.cancel', {}, { handle: pass.handle });
    preview = null;
    await pick();
  }
</script>

<div class="stack" style="max-width:520px">
  {#if result}
    <div class="gap"><span class="ok-text" style="display:flex"><Icon name="check_circle" size={22} /></span><span>{result}</span></div>
    <div><Btn label="Import another file" onclick={pick} /></div>
  {:else if preview}
    <p style="font-size:var(--fs-l);font-weight:600">{preview.count} items from {preview.format}</p>
    <p class="dim">{preview.titles.slice(0, 6).join(', ')}{preview.count > 6 ? ' and more' : ''}</p>
    {#if preview.skipped.length > 0}<p class="warn-text">{preview.skipped.length} could not be read: {preview.skipped.slice(0, 3).join(', ')}</p>{/if}
    <div class="gap">
      <Btn label="Import {preview.count} items" icon="download" variant="primary" onclick={apply} />
      <Btn label="Choose another file" onclick={another} />
    </div>
  {:else if needsPassword}
    <p style="font-size:var(--fs-l);font-weight:600">This file has a password</p>
    <PasswordField bind:value={password} placeholder="The file’s password" autofocus onenter={read} />
    {#if error}<p class="danger-text">{error}</p>{/if}
    <div><Btn label="Open" variant="primary" disabled={!password} onclick={read} /></div>
  {:else}
    <p style="font-size:var(--fs-l);font-weight:600">Bring everything over</p>
    <p class="dim">{FORMATS}</p>
    {#if error}<p class="danger-text">{error}</p>{/if}
    <div><Btn label="Choose a file" icon="file_open" variant="primary" onclick={pick} /></div>
    <p class="dimmer small">Delete the exported file afterwards. It holds your passwords in plain text.</p>
  {/if}
</div>
