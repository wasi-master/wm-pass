<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Row from '../parts/Row.svelte';
  import Select from '../parts/Select.svelte';
  import Switch from '../parts/Switch.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { bytesOf, consumeText, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import type { SlotView } from '../engine/types';
  import { ui } from '../vault/ui.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';
  import { hardwareAvailable } from './hardware';

  let slots = $state<SlotView[]>([]);
  let refresh = $state(0);
  let sheet = $state<null | 'change' | 'rekey' | 'decoy' | 'kit' | { action: string; title: string; body: string; keyFile?: Uint8Array }>(null);
  let oldPassword = $state('');
  let newPassword = $state('');
  let label = $state('');
  let name = $state('Personal');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let total = $state(3);
  let threshold = $state(2);
  let shares = $state<string[] | null>(null);
  let quickReady = $state(false);
  let quickOn = $state(false);
  const inside = $derived(pass.account !== null || pass.child !== null);

  $effect(() => {
    void pass.revision;
    void refresh;
    void pass
      .slots()
      .then((s) => (slots = s))
      .catch(() => (slots = []));
  });

  $effect(() => {
    const desktop = host().desktop;
    const file = pass.fileName;
    if (!desktop || !file) return;
    void desktop.quickAvailable().then(async (ready) => {
      quickReady = ready;
      quickOn = ready && (await desktop.quickEnrolled(file));
    });
  });

  function openSheet(next: typeof sheet) {
    oldPassword = '';
    newPassword = '';
    label = '';
    error = null;
    busy = false;
    shares = null;
    sheet = next;
  }

  async function changePassword(rekey: boolean) {
    busy = true;
    error = null;
    const current = bytesOf(oldPassword);
    const next = bytesOf(newPassword);
    try {
      if (!(await pass.verify(current))) {
        error = 'That is not the current password.';
        return;
      }
      if (rekey) await pass.rekey(next);
      else await pass.change('vault.change_password', { index: (await pass.slots())[0]?.index ?? 0 }, { secret: next });
      overlays.toast('Password changed.');
      sheet = null;
    } catch (failure: any) {
      error = failure.message;
    } finally {
      current.fill(0);
      next.fill(0);
      busy = false;
    }
  }

  async function addPassword() {
    if (!sheet || typeof sheet === 'string') return;
    const secret = bytesOf(newPassword);
    const file = sheet.keyFile;
    try {
      await pass.change('slot.add', { label, action: sheet.action, ...(file ? { factor: { kind: 'key_file' } } : {}) }, { secret, blobs: file ? [file] : [] });
      sheet = null;
      refresh++;
    } catch (failure: any) {
      error = failure.message;
    } finally {
      secret.fill(0);
      file?.fill(0);
    }
  }

  async function chooseKeyFile() {
    const file = await host().pickFile('Choose a key file');
    if (file) openSheet({ action: 'open', title: 'Password for the key file', body: 'This password and the file together will open the vault.', keyFile: file.bytes });
  }

  async function removeSlot(slot: SlotView) {
    const yes = await overlays.ask({ title: 'Remove this way in?', body: 'It will stop opening the vault. The file keeps at least one other way in.', action: 'Remove', danger: true });
    if (!yes) return;
    try {
      await pass.change('slot.remove', { index: slot.index });
      refresh++;
    } catch (failure: any) {
      overlays.toast(failure.message);
    }
  }

  async function makeDecoy() {
    const secret = bytesOf(newPassword);
    busy = true;
    try {
      await pass.createHidden(name.trim() || 'Personal', secret);
      sheet = null;
      overlays.toast('The decoy vault is ready.');
    } catch (failure: any) {
      error = failure.message;
    } finally {
      secret.fill(0);
      busy = false;
    }
  }

  async function makeShares() {
    const reply = await pass.change('emergency.kit', { threshold, total });
    shares = consumeText(reply.secret).split('\n');
  }

  async function saveKit() {
    const body =
      `WM Pass emergency kit\n\nGive each person one share. Any ${threshold} of them, together with a copy of the vault file, can open it. They can do so at once: the waiting period is a promise between you, not something the software can enforce.\n\n` +
      (shares ?? []).map((share, i) => `Share ${i + 1}\n${share}\n\n`).join('');
    const bytes = bytesOf(body);
    await host().saveFile('Save the emergency kit', 'wm-pass-emergency-kit.txt', bytes);
    bytes.fill(0);
  }

  async function setQuick(on: boolean) {
    const desktop = host().desktop!;
    const file = pass.fileName;
    if (!file) return;
    try {
      if (on) await desktop.quickEnroll(pass.handle, file);
      else await desktop.quickForget(file);
      quickOn = on;
    } catch (failure: any) {
      overlays.toast(failure.message);
    }
  }

  async function panicNow() {
    const yes = await overlays.ask({ title: 'Lock and refuse passwords for an hour?', body: 'Nothing is lost. Every password is refused for an hour, the right one included.', action: 'Lock it', danger: true });
    if (yes) await pass.panic();
  }

  const LOCK_OPTIONS: [number, string][] = [
    [60, '1 minute'],
    [300, '5 minutes'],
    [900, '15 minutes'],
    [1800, '30 minutes'],
    [3600, '1 hour'],
    [-1, 'Never'],
  ];
</script>

<Group title="Locking" anchor="group:security.locking">
  <Choice title="Lock after no activity" icon="lock_clock" pref="lock_after" options={LOCK_OPTIONS} />
  <Toggle title="Lock when the computer sleeps" icon="dark_mode" pref="lock_on_sleep" />
  <Toggle title="Lock when the window is hidden" sub="Also when it is closed to the tray." icon="visibility_off" pref="lock_on_hide" />
  <Toggle title="Block screenshots" sub="Keeps other programs from capturing this window. Linux desktops may ignore it." icon="screen_lock_portrait" pref="screen_protection" />
  <Choice
    title="Clear the clipboard after"
    icon="content_paste"
    pref="clipboard_seconds"
    options={[[15, '15 seconds'], [30, '30 seconds'], [60, '1 minute'], [120, '2 minutes'], [0, 'Never']] as [number, string][]}
  />
  {#if host().desktop && quickReady}
    <Row title="Unlock from the system keyring" help="action.keyring_unlock" sub="A convenience, not protection: any program in your session can ask the keyring for the key." icon="key">
      <Switch checked={quickOn} label="Unlock from the system keyring" onchange={setQuick} />
    </Row>
  {/if}
</Group>

{#if !inside}
  <Group title="Master password" anchor="group:security.master">
    <Row title="Change it" help="action.change_password" sub="Old backups still open with the old password" icon="password" chevron onclick={() => openSheet('change')} />
    <Row title="Change it and rekey" help="action.rekey" sub="A new vault key too, so old copies of the file cannot follow new changes" icon="lock_reset" chevron onclick={() => openSheet('rekey')} />
  </Group>

  <Group title="Unlock methods" anchor="group:security.methods" help="group.security.methods">
    {#each slots as slot (slot.index)}
      <Row title={slot.label || `Password ${slot.index + 1}`} icon="key" onclick={() => removeSlot(slot)} chevron />
    {/each}
    <Row title="Add a password" help="action.add_password" icon="add" chevron onclick={() => openSheet({ action: 'open', title: 'Add a password', body: 'Opens the same vault.' })} />
    <Row title="Add a key file" help="action.add_key_file" sub="Both the password and the file will be needed" icon="file_open" chevron onclick={chooseKeyFile} />
    {#if hardwareAvailable()}
      <Row title="Add a security key" help="action.add_security_key" sub="YubiKey or another FIDO2 key, by USB" icon="security_key" chevron onclick={() => ui.go('settings/hardware')} />
    {/if}
  </Group>

  <Group title="Duress" anchor="group:security.duress" help="feature.duress">
    <Row
      title="Wipe password" help="action.wipe_password"
      sub="Erases the vault on this device. Looks like a wrong password."
      icon="dangerous"
      chevron
      onclick={() => openSheet({ action: 'wipe', title: 'Wipe password', body: 'Entering this erases the vault on this device. Disks may keep traces, and backups and synced copies are not affected.' })}
    />
    <Row title="Lockout password" help="action.lockout_password" sub="Refuses every attempt for an hour." icon="lock" chevron onclick={() => openSheet({ action: 'lock', title: 'Lockout password', body: 'Refuses every attempt for an hour.' })} />
    <Row title="Decoy vault" help="action.decoy_vault" sub="A separate vault with its own password." icon="layers" chevron onclick={() => openSheet('decoy')} />
    <Row title="Lock and refuse passwords now" help="action.panic_now" sub="Every password is refused for an hour." icon="block" chevron onclick={panicNow} />
  </Group>

  <Group title="Emergency access" anchor="group:security.emergency">
    <Row title="Emergency kit" help="action.emergency_kit" sub="Split your key between trusted people" icon="family_restroom" chevron onclick={() => openSheet('kit')} />
  </Group>
{/if}

{#if host().kind === 'desktop' && host().os === 'mac'}
  <Group title="Widgets">
    <Toggle title="Show on widgets" sub="While the vault is open, the Codes and Favorites widgets show codes and the names of favorites. Locked, they show nothing." icon="widgets" pref="widget_access" />
  </Group>
{/if}
<Group title="Network" anchor="group:security.network">
  <Toggle title="Allow network features" sub="Breach checks, aliases, syncing and accounts. Off means nothing leaves this device." icon="language" pref="network_allowed" />
</Group>

{#if sheet === 'change' || sheet === 'rekey'}
  {@const rekey = sheet === 'rekey'}
  <Dialog title={rekey ? 'Change it and rekey' : 'Change your password'} onclose={() => (sheet = null)}>
    <div class="stack">
      <p class="dim">{rekey ? 'The vault gets a new key and is encrypted again. Any other way in stops working until you add it back.' : 'The vault key stays the same, so a backup made earlier still opens with the old password.'}</p>
      <PasswordField bind:value={oldPassword} placeholder="Current password" autofocus />
      <PasswordField bind:value={newPassword} placeholder="New password" onenter={() => changePassword(rekey)} />
      {#if error}<p class="danger-text">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label={busy ? 'Working…' : 'Change'} variant="primary" disabled={busy || !oldPassword || !newPassword} onclick={() => changePassword(rekey)} />
    {/snippet}
  </Dialog>
{:else if sheet === 'decoy'}
  <Dialog title="A decoy vault" onclose={() => (sheet = null)}>
    <div class="stack">
      <p class="dim">A second vault, opened by its own password. The door looks the same either way and nothing in the file says it exists. This resists a quick look under pressure. It does not stand up to forensics, or to someone who has an older copy of your files.</p>
      <Field bind:value={name} placeholder="Its name" />
      <PasswordField bind:value={newPassword} placeholder="Its password" autofocus onenter={makeDecoy} />
      {#if error}<p class="danger-text">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label={busy ? 'Making it…' : 'Make it'} variant="primary" disabled={busy || !newPassword} onclick={makeDecoy} />
    {/snippet}
  </Dialog>
{:else if sheet === 'kit'}
  <Dialog title="Emergency kit" onclose={() => (sheet = null)}>
    <div class="stack">
      <p class="dim">Your key is split into shares. Any {threshold} of the {total} people can open your vault together, and fewer learn nothing. They can do it at once: any waiting period is a promise between you, not something software can enforce.</p>
      {#if shares}
        <p>Save this file, print it, and delete the file once each person has their share.</p>
      {:else}
        <div class="form-row">
          <span class="label">People</span>
          <Select
            value={total}
            options={[2, 3, 4, 5, 6, 7].map((n) => [n, String(n)] as [number, string])}
            onchange={(n) => {
              total = n;
              threshold = Math.min(threshold, n);
            }}
          />
        </div>
        <div class="form-row">
          <span class="label">Needed together</span>
          <Select value={threshold} options={Array.from({ length: total - 1 }, (_, i) => [i + 2, String(i + 2)] as [number, string])} onchange={(n) => (threshold = n)} />
        </div>
      {/if}
    </div>
    {#snippet foot()}
      <Btn label="Close" onclick={() => (sheet = null)} />
      {#if shares}
        <Btn label="Save the kit" icon="download" variant="primary" onclick={saveKit} />
      {:else}
        <Btn label="Make the shares" variant="primary" onclick={makeShares} />
      {/if}
    {/snippet}
  </Dialog>
{:else if sheet}
  <Dialog title={sheet.title} onclose={() => (sheet = null)}>
    <div class="stack">
      <p class="dim">{sheet.body}</p>
      <Field bind:value={label} placeholder="A name only you will see" autofocus />
      <PasswordField bind:value={newPassword} placeholder="Password" onenter={addPassword} />
      {#if error}<p class="danger-text">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label="Add" variant="primary" disabled={!newPassword} onclick={addPassword} />
    {/snippet}
  </Dialog>
{/if}
