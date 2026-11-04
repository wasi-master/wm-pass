<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Group from '../parts/Group.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { consumeText } from '../engine/frame';
  import { printPage } from '../engine/print';

  let asking = $state(false);
  let password = $state('');

  async function sealed() {
    await host().saveFile('Save the vault', 'vault.wmvault', await pass.sealedFile());
  }

  async function exportKdbx() {
    asking = false;
    const secret = bytesOf(password);
    try {
      const reply = await pass.engine('export.file', { format: 'kdbx' }, { handle: pass.handle, secret });
      await host().saveFile('Save the KeePass database', 'wm-pass.kdbx', reply.blobs[0]);
      password = '';
    } catch (failure: any) {
      overlays.toast(failure.message);
    } finally {
      secret.fill(0);
    }
  }

  async function plain(format: string, label: string, name: string) {
    const yes = await overlays.ask({ title: 'Export without encryption?', body: `${label} holds every password in plain text. Keep it off shared storage and delete it when you are done.`, action: 'Export' });
    if (!yes) return;
    const reply = await pass.engine('export.file', { format }, { handle: pass.handle });
    await host().saveFile(`Save ${label}`, name, reply.secret!);
    reply.secret!.fill(0);
  }

  // The printable page, handed to the system's print dialog with no file written. The dialog
  // saves a PDF, or sends it to a printer, wherever the person picks.
  let clearPrint: (() => void) | null = null;
  $effect(() => () => clearPrint?.());

  async function print() {
    const yes = await overlays.ask({ title: 'Export without encryption?', body: 'PDF or printer holds every password in plain text. Keep it off shared storage and delete it when you are done.', action: 'Export' });
    if (!yes) return;
    try {
      const reply = await pass.engine('export.file', { format: 'html' }, { handle: pass.handle });
      const page = reply.blobs.length > 0 ? new TextDecoder().decode(reply.blobs[0]) : consumeText(reply.secret);
      clearPrint?.();
      clearPrint = await printPage(page);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The print dialog could not be opened.');
    }
  }
</script>

<Group title="Stays encrypted">
  <Row title="WM Pass vault" help="action.export_wmvault" sub="The vault file itself, sealed. Opens with your password." icon="encrypted" chevron onclick={sealed} />
  <Row title="KeePass database" help="action.export_kdbx" sub="A new .kdbx file with a password you choose." icon="database" chevron onclick={() => (asking = true)} />
</Group>
<Group title="Plain text" help="group.export.plain">
  <Row title="Bitwarden JSON" help="action.export_bitwarden" sub="Anyone who opens this file can read every password." icon="warning" chevron onclick={() => plain('bitwarden', 'Bitwarden JSON', 'wm-pass-bitwarden.json')} />
  <Row title="CSV" help="action.export_csv" sub="Anyone who opens this file can read every password." icon="warning" chevron onclick={() => plain('csv', 'CSV', 'wm-pass.csv')} />
  <Row title="Printable page" help="action.export_html" sub="Anyone who opens this file can read every password." icon="warning" chevron onclick={() => plain('html', 'Printable page', 'wm-pass.html')} />
  <Row title="PDF or printer" help="action.export_pdf" sub="Anyone who opens this file can read every password." icon="warning" chevron onclick={print} />
</Group>

{#if asking}
  <Dialog title="Password for the new file" onclose={() => (asking = false)}>
    <PasswordField bind:value={password} placeholder="Password" autofocus onenter={exportKdbx} />
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (asking = false)} />
      <Btn label="Export" variant="primary" disabled={!password} onclick={exportKdbx} />
    {/snippet}
  </Dialog>
{/if}
