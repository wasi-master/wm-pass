<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Icon from '../parts/Icon.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import ScanDialog from '../parts/ScanDialog.svelte';
  import { qrInPicture } from '../parts/qr';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import type { ImportPreview, LinkedView } from '../engine/types';
  import GuideSheet from './GuideSheet.svelte';

  const FORMATS = '1Password, Apple Passwords, Avast, Avira, Bitwarden, Buttercup, Chrome and other Chromium browsers, Dashlane, Enpass, Firefox, Kaspersky, KeePass and KeePassXC, Keeper, LastPass, mSecure, NordPass, Password Safe, Proton Pass, Psono, RoboForm, SafeInCloud, Sticky Password and Zoho Vault. Authenticators: Aegis, 2FAS, andOTP, Authenticator Pro, Bitwarden Authenticator, Ente Auth, FreeOTP+, LastPass Authenticator, Proton Authenticator, Raivo, and Google’s export QR code.';

  let guides = $state(false);
  let preview = $state<ImportPreview | null>(null);
  let bytes = $state<Uint8Array | null>(null);
  let needsPassword = $state(false);
  let password = $state('');
  // A KeePass database may also need its key file, or only that.
  let keyFile = $state<{ name: string; bytes: Uint8Array } | null>(null);
  const isKeePass = $derived(!!bytes && bytes.length > 4 && bytes[0] === 0x03 && bytes[1] === 0xd9 && bytes[2] === 0xa2 && bytes[3] === 0x9a);
  let error = $state<string | null>(null);
  let result = $state<string | null>(null);

  // Where the items go: this vault, or a nested one stepped into for the import and left after.
  // A shared vault lives at a place of its own, set up from its page, so it is not offered.
  const insideAlready = pass.child !== null || pass.account !== null;
  const home = pass.state.kind === 'unlocked' ? pass.state.name : '';
  let nested = $state<LinkedView[]>([]);
  let enteredFor = $state<string | null>(null);
  if (!insideAlready) {
    void pass
      .view<LinkedView[]>('linked.list')
      .then((list) => (nested = list.filter((l) => !l.shared)))
      .catch(() => (nested = []));
  }

  // Leaving the page part-way puts the vault back as it was.
  $effect(() => () => {
    if (enteredFor !== null) void pass.leaveChild();
  });

  async function importInto(file: string | null) {
    if (file === enteredFor) return;
    if (preview) await pass.engine('import.cancel', {}, { handle: pass.handle });
    preview = null;
    error = null;
    try {
      if (enteredFor !== null) {
        await pass.leaveChild();
        enteredFor = null;
      }
      if (file !== null) {
        const sealed = await pass.readLinked(file);
        const reply = await pass.engine('linked.open', { file }, { handle: pass.handle, blobs: [sealed] });
        await pass.enterChild(file, reply, (data) => pass.writeLinked(file, data));
        enteredFor = file;
      }
      if (bytes) await read();
    } catch (failure: any) {
      error = failure?.message ?? 'That vault could not be opened.';
    }
  }

  // Google Authenticator's export links, scanned or read from pictures. A long list comes as
  // several codes, and one import takes them all.
  let codes = $state<string[]>([]);
  let scanning = $state(false);

  async function takeCodes(found: string[]) {
    const links = found.filter((text) => text.startsWith('otpauth'));
    if (links.length === 0) {
      error = found.length === 0 ? 'No QR code was found in that picture.' : 'That code is not an export from an authenticator app.';
      return;
    }
    codes = [...new Set([...codes, ...links])];
    bytes?.fill(0);
    bytes = new TextEncoder().encode(codes.join('\n'));
    preview = null;
    result = null;
    await read();
  }

  function scanCode() {
    error = null;
    scanning = true;
  }

  async function read() {
    if (!bytes) return;
    const secret = bytesOf(password);
    try {
      const blobs = keyFile && isKeePass ? [bytes, keyFile.bytes] : [bytes];
      preview = (await pass.engine<ImportPreview>('import.read', {}, { handle: pass.handle, secret, blobs })).json;
      needsPassword = false;
      error = null;
    } catch (failure: any) {
      needsPassword = failure.code === 'needs_password' || failure.code === 'wrong_password' || /password/i.test(failure.message ?? '');
      error = needsPassword && !password && !keyFile ? null : failure.message;
    } finally {
      secret.fill(0);
    }
  }

  const PICTURE = /\.(png|jpe?g|gif|webp|bmp|heic|heif|avif)$/i;

  async function pick() {
    const file = await host().pickFile('Choose a file to import');
    if (!file) return;
    error = null;
    // A screenshot or photo of an export code, rather than an export file.
    if (PICTURE.test(file.name)) {
      let found: string[] = [];
      try {
        found = await qrInPicture(new Blob([file.bytes as BlobPart]));
      } catch {
        error = 'That picture could not be read.';
      }
      file.bytes.fill(0);
      if (!error) await takeCodes(found);
      return;
    }
    codes = [];
    bytes?.fill(0);
    bytes = file.bytes;
    preview = null;
    result = null;
    password = '';
    keyFile?.bytes.fill(0);
    keyFile = null;
    await read();
  }

  async function chooseKeyFile() {
    const file = await host().pickFile('Choose the key file');
    if (!file) return;
    keyFile?.bytes.fill(0);
    keyFile = { name: file.name, bytes: file.bytes };
    error = null;
  }

  async function apply() {
    const reply = await pass.change<{ added: number; duplicates: number }>('import.apply');
    const into = nested.find((n) => n.file === enteredFor)?.name;
    result = `Added ${reply.json.added}${into ? ` to ${into}` : ''}. Skipped ${reply.json.duplicates} already there.`;
    bytes?.fill(0);
    bytes = null;
    keyFile?.bytes.fill(0);
    keyFile = null;
    codes = [];
    preview = null;
  }

  async function another() {
    await pass.engine('import.cancel', {}, { handle: pass.handle });
    preview = null;
    codes = [];
    await pick();
  }
</script>

<div class="stack" style="max-width:520px">
  {#if (result === null && !insideAlready && nested.length > 0) || enteredFor !== null}
    <div class="stack" style="gap:6px">
      <span class="dim small" style="font-weight:600">Import into</span>
      <div class="gap" style="flex-wrap:wrap;gap:6px">
        <button type="button" class="chip" aria-pressed={enteredFor === null} onclick={() => void importInto(null)}><Icon name="lock" size={14} />{home || 'This vault'}</button>
        {#each nested as link (link.file)}
          <button type="button" class="chip" aria-pressed={enteredFor === link.file} onclick={() => void importInto(link.file)}><Icon name="layers" size={14} />{link.name}</button>
        {/each}
      </div>
    </div>
  {:else if pass.child && result === null}
    <p class="dim">Importing into {pass.child}, the nested vault you are inside.</p>
  {/if}
  {#if result}
    <div class="gap"><span class="ok-text" style="display:flex"><Icon name="check_circle" size={22} /></span><span>{result}</span></div>
    <div><Btn label="Import another file" onclick={pick} /></div>
  {:else if preview}
    <p style="font-size:var(--fs-l);font-weight:600">{preview.count} items from {preview.format}</p>
    <p class="dim">{preview.titles.slice(0, 6).join(', ')}{preview.count > 6 ? ' and more' : ''}</p>
    {#if preview.skipped.length > 0}<p class="warn-text">{preview.skipped.length} could not be read: {preview.skipped.slice(0, 3).join(', ')}</p>{/if}
    {#if codes.length > 0}
      <p class="dim">{codes.length} {codes.length === 1 ? 'code' : 'codes'} read. Google splits a long list across several codes, so scan each one before importing.</p>
      {#if error}<p class="danger-text">{error}</p>{/if}
    {/if}
    <div class="gap" style="flex-wrap:wrap">
      <Btn label="Import {preview.count} items" icon="download" variant="primary" onclick={apply} />
      {#if codes.length > 0}<Btn label="Scan the next code" icon="qr_code_scanner" onclick={scanCode} />{/if}
      <Btn label="Choose another file" onclick={another} />
    </div>
  {:else if needsPassword}
    <p style="font-size:var(--fs-l);font-weight:600">{isKeePass ? 'This database is locked' : 'This file has a password'}</p>
    {#if isKeePass}<p class="dim">Enter its password. If it also uses a key file, add that too. A database locked by a key file alone needs no password.</p>{/if}
    <PasswordField bind:value={password} placeholder={keyFile ? 'The password, if it has one' : 'The file’s password'} autofocus onenter={read} />
    {#if error}<p class="danger-text">{error}</p>{/if}
    <div class="gap" style="flex-wrap:wrap">
      <Btn label="Open" variant="primary" disabled={!password && !keyFile} onclick={read} />
      {#if isKeePass}<Btn label={keyFile ? `Key file: ${keyFile.name}` : 'Add key file'} icon="key" onclick={chooseKeyFile} />{/if}
    </div>
  {:else}
    <p style="font-size:var(--fs-l);font-weight:600">Bring everything over</p>
    <p class="dim">{FORMATS}</p>
    {#if error}<p class="danger-text">{error}</p>{/if}
    <div class="gap" style="flex-wrap:wrap">
      <Btn label="Choose a file" icon="file_open" variant="primary" onclick={pick} />
      <Btn label="Scan Google Authenticator’s code" icon="qr_code_scanner" onclick={scanCode} />
    </div>
    <p class="dimmer small">Delete the exported file afterwards. It holds your passwords in plain text.</p>
    <div><Btn label="How to export from your old app" icon="menu_book" onclick={() => (guides = true)} /></div>
  {/if}
</div>

{#if guides}
  <GuideSheet onclose={() => (guides = false)} />
{/if}

{#if scanning}
  <ScanDialog
    title="Scan Google Authenticator’s code"
    hint="In Google Authenticator, choose Transfer codes, then Export codes. A screenshot of each code works too."
    onfound={(texts) => {
      scanning = false;
      void takeCodes(texts);
    }}
    onclose={() => (scanning = false)}
  />
{/if}
