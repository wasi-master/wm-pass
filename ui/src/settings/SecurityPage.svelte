<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Row from '../parts/Row.svelte';
  import Select from '../parts/Select.svelte';
  import Switch from '../parts/Switch.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { bytesOf, consumeText, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import type { NeededFactor, SlotView } from '../engine/types';
  import { ui } from '../vault/ui.svelte';
  import Choice from './Choice.svelte';
  import Toggle from './Toggle.svelte';
  import { hardwareAvailable, hardwareTap } from './hardware';
  import { passwordsDiffer } from '../engine/text';

  let slots = $state<SlotView[]>([]);
  let refresh = $state(0);
  let sheet = $state<null | 'change' | 'rekey' | 'decoy' | 'kit' | { action: string; title: string; body: string; keyFile?: Uint8Array }>(null);
  let oldPassword = $state('');
  let newPassword = $state('');
  let newAgain = $state('');
  let label = $state('');
  let name = $state('Personal');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let total = $state(3);
  let threshold = $state(2);
  let shares = $state<string[] | null>(null);
  /** Whether the shares just made have gone to a file: the vault already counts on them. */
  let kitSaved = $state(false);
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

  const quick = host().quick;

  $effect(() => {
    const file = pass.fileName;
    if (!quick || !file) return;
    void quick
      .available()
      .then(async (ready) => {
        quickReady = ready;
        quickOn = ready && (await quick.enrolled(file));
      })
      .catch(() => (quickReady = false));
  });

  function openSheet(next: typeof sheet) {
    if (sheet && typeof sheet !== 'string' && sheet !== next) sheet.keyFile?.fill(0);
    oldPassword = '';
    newPassword = '';
    newAgain = '';
    label = '';
    error = null;
    busy = false;
    shares = null;
    kitSaved = false;
    againTried = false;
    dropFactors();
    sheet = next;
  }

  // A password whose way in also needs a key file or a security key: what it needs, and the
  // answers gathered so far. Asked for as at the door, and wiped when the sheet closes.
  let needed = $state<NeededFactor[] | null>(null);
  let factors: { kind: string; bytes: Uint8Array }[] = [];
  let factorChosen = $state(false);

  function dropFactors() {
    factors.forEach((f) => f.bytes.fill(0));
    factors = [];
    needed = null;
    factorChosen = false;
  }

  function useFactor(kind: string, bytes: Uint8Array) {
    factors.forEach((f) => f.bytes.fill(0));
    factors = [{ kind, bytes }];
    factorChosen = true;
  }

  async function chooseFactorFile(rekey: boolean) {
    if (busy) return;
    try {
      const file = await host().pickFile('Choose the key file');
      if (!file) return;
      useFactor('key_file', file.bytes);
    } catch (failure: any) {
      error = failure?.message ?? 'That file could not be read.';
      return;
    }
    await changePassword(rekey);
  }

  async function tapFactor(rekey: boolean) {
    const hardware = needed?.filter((f) => f.kind !== 'key_file') ?? [];
    if (hardware.length === 0 || !hardwareAvailable()) return;
    for (const factor of hardware) {
      try {
        useFactor(factor.kind, await hardwareTap(factor));
        await changePassword(rekey);
        return;
      } catch (failure: any) {
        error = failure?.message ?? 'The security key did not answer.';
      }
    }
  }

  function closeSheet() {
    openSheet(null);
  }

  async function changePassword(rekey: boolean) {
    againTried = true;
    if (busy || !oldPassword || !newPassword || newPassword !== newAgain) return;
    busy = true;
    error = null;
    const current = bytesOf(oldPassword);
    const next = bytesOf(newPassword);
    let tapNext = false;
    try {
      // The same bar as a new vault's password: this one replaces it. Rated first, so the check
      // of the current password comes straight before the change that relies on it.
      const probe = bytesOf(newPassword);
      const rated = await pass.strength(probe).finally(() => probe.fill(0));
      if (rated.score < 3) {
        error = rated.warning || 'That password is too easy to guess. Add a word or two.';
        return;
      }
      // The slot the old password opens is the one replaced, not simply the first, with its key
      // file or security key, which the engine keeps on it.
      const found = await pass.passwordSlot(current, factors);
      if (found === null) {
        error = factorChosen ? 'That is not the current password, or not its key file or security key.' : 'That is not the current password.';
        dropFactors();
        return;
      }
      if ('needs' in found) {
        const wrong = factorChosen;
        dropFactors();
        needed = found.needs;
        error = wrong ? 'That key file or security key does not go with this password.' : null;
        // A security key is asked for at once, as at the door; a key file waits to be chosen.
        tapNext = !wrong && found.needs.some((f) => f.kind !== 'key_file');
        return;
      }
      const slot = found.slot;
      let quickOff = false;
      if (rekey) quickOff = (await pass.rekey(next)).quickOff;
      else await pass.change('vault.change_password', { index: slot }, { secret: next });
      if (quickOff) {
        quickOn = false;
        overlays.toast(`Password changed. ${quick?.title ?? 'Quick unlock'} is off, since it kept the old key: turn it on again here.`, { ms: 10000 });
      } else overlays.toast('Password changed.');
      closeSheet();
    } catch (failure: any) {
      error = failure?.message ?? 'The password could not be changed.';
    } finally {
      current.fill(0);
      next.fill(0);
      busy = false;
    }
    if (tapNext) await tapFactor(rekey);
  }

  async function addPassword() {
    againTried = true;
    if (busy || !newPassword || newPassword !== newAgain || !sheet || typeof sheet === 'string') return;
    busy = true;
    error = null;
    const secret = bytesOf(newPassword);
    const file = sheet.keyFile;
    try {
      await pass.change('slot.add', { label, action: sheet.action, ...(file ? { factor: { kind: 'key_file' } } : {}) }, { secret, blobs: file ? [file] : [] });
      // The key file is zeroed when the sheet closes, not before: a failed add can be tried again.
      closeSheet();
      refresh++;
    } catch (failure: any) {
      error = failure?.message ?? 'That could not be added.';
    } finally {
      secret.fill(0);
      busy = false;
    }
  }

  async function chooseKeyFile() {
    const file = await host()
      .pickFile('Choose a key file')
      .catch((failure: any) => {
        overlays.toast(failure?.message ?? 'That file could not be read.');
        return null;
      });
    if (file) openSheet({ action: 'open', title: 'Password for the key file', body: 'This password and the file together will open the vault.', keyFile: file.bytes });
  }

  /** Which of two passwords changed at once this is: when, and on which device if it said. */
  function conflictSub(slot: SlotView) {
    const when = slot.changed_ms ? new Date(slot.changed_ms).toLocaleString() : null;
    const where = slot.changed_on ? ` on ${slot.changed_on}` : '';
    return `Changed at the same time as another${when ? `: ${when}` : ''}${where}`;
  }

  async function removeSlot(slot: SlotView) {
    const yes = await overlays.ask({ title: 'Remove this way in?', body: 'It will stop opening the vault. The file keeps at least one other way in.', action: 'Remove', danger: true });
    if (!yes) return;
    try {
      await pass.change('slot.remove', { index: slot.index });
      refresh++;
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be removed.');
    }
  }

  async function makeDecoy() {
    againTried = true;
    if (busy || !newPassword || newPassword !== newAgain) return;
    const secret = bytesOf(newPassword);
    busy = true;
    error = null;
    try {
      await pass.createHidden(name.trim() || 'Personal', secret);
      closeSheet();
      overlays.toast('The decoy vault is ready.');
    } catch (failure: any) {
      error = failure?.message ?? 'The decoy vault could not be made.';
    } finally {
      secret.fill(0);
      busy = false;
    }
  }

  async function makeShares() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const reply = await pass.change('emergency.kit', { threshold, total });
      shares = consumeText(reply.secret).split('\n');
    } catch (failure: any) {
      error = failure?.message ?? 'The shares could not be made.';
    } finally {
      busy = false;
    }
  }

  async function saveKit() {
    const body =
      `WM Pass emergency kit\n\nGive each person one share. Any ${threshold} of them, together with a copy of the vault file, can open it. They can do so at once: any waiting period is a promise between you, not something the software can enforce.\n\n` +
      (shares ?? []).map((share, i) => `Share ${i + 1}\n${share}\n\n`).join('');
    const bytes = bytesOf(body);
    try {
      if (await host().saveFile('Save the emergency kit', 'wm-pass-emergency-kit.txt', bytes)) kitSaved = true;
    } catch (failure: any) {
      error = failure?.message ?? 'The kit could not be saved.';
    } finally {
      bytes.fill(0);
    }
  }

  // Shares made and never saved are gone, while the vault opens with them from now on.
  async function closeKit() {
    if (shares && !kitSaved) {
      const yes = await overlays.ask({ title: 'Close without saving the kit?', body: 'These shares are not shown again. Make new ones later if you need them.', action: 'Close', danger: true });
      if (!yes) return;
    }
    closeSheet();
  }

  let quickBusy = $state(false);
  async function setQuick(on: boolean) {
    const file = pass.fileName;
    if (!quick || !file || quickBusy) return;
    // Enrolling may wait on the system's prompt: a second press must not start another.
    quickBusy = true;
    try {
      if (on) await quick.enroll(pass.handle, file);
      else await quick.forget(file);
      quickOn = on;
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be changed.');
    } finally {
      quickBusy = false;
    }
  }

  async function panicNow() {
    const yes = await overlays.ask({ title: 'Lock and refuse passwords for an hour?', body: 'Nothing is lost. Every password is refused for an hour, the right one included.', action: 'Lock it', danger: true });
    if (!yes) return;
    await pass.panic().catch((failure: any) => overlays.toast(failure?.message ?? 'The vault could not be locked.'));
  }

  /** A second password is judged as at the door. */
  let againTried = $state(false);
  const mismatch = $derived(passwordsDiffer(newAgain, newPassword, againTried));

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
  <!-- Only the desktop app has a tray to close to; the switch to block screenshots is only where
       the window can refuse them. -->
  <Toggle title="Lock when the window is hidden" sub={host().kind === 'desktop' ? 'Also when it is closed to the tray.' : null} icon="visibility_off" pref="lock_on_hide" />
  {#if host().setScreenProtection}
    <Toggle title="Block screenshots" sub={host().os === 'linux' ? 'Keeps other programs from capturing this window. Linux desktops may ignore it.' : 'Keeps other programs from capturing this window.'} icon="screen_lock_portrait" pref="screen_protection" />
  {/if}
  <Choice
    title="Clear the clipboard after"
    icon="content_paste"
    pref="clipboard_seconds"
    options={[[15, '15 seconds'], [30, '30 seconds'], [60, '1 minute'], [120, '2 minutes'], [300, '5 minutes'], [0, 'Never']] as [number, string][]}
  />
  {#if quick && quickReady}
    <Row title={quick.title} help={quick.help} sub={quick.sub} icon="key">
      <Switch checked={quickOn} label={quick.title} disabled={quickBusy} onchange={setQuick} />
    </Row>
  {/if}
</Group>

{#if !inside}
  <Group title="Master password" anchor="group:security.master">
    <Row title="Change it" help="action.change_password" sub="Old backups still open with the old password" icon="password" chevron onclick={() => openSheet('change')} />
    <Row title="Change it and rekey" help="action.rekey" sub="A new vault key too, so old copies of the file cannot follow new changes" icon="lock_reset" chevron onclick={() => openSheet('rekey')} />
  </Group>

  <Group title="Unlock methods" anchor="group:security.methods" help="group.security.methods">
    {#if pass.notices.password_conflict}
      {@const conflict = pass.notices.password_conflict}
      <div class="banner warn" role="status">
        <span style="flex:1">{conflict.both_open ? 'Your master password was changed on two devices at about the same time, so both new passwords open this vault. Remove the one you don\'t want; the one you keep goes on working.' : conflict.message}</span>
        {#if !conflict.both_open}<Btn label="Got it" onclick={() => void pass.passwordLossSeen()} />{/if}
      </div>
    {/if}
    {#each slots as slot (slot.index)}
      <!-- A click removes it, after asking: a remove button says so, where a chevron promised a page. -->
      <Row title={slot.label || `Password ${slot.index + 1}`} sub={slot.conflict ? conflictSub(slot) : null} icon="key">
        <IconBtn icon="delete" label="Remove {slot.label || `password ${slot.index + 1}`}" size={16} onclick={() => removeSlot(slot)} />
      </Row>
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
  <Dialog title={rekey ? 'Change it and rekey' : 'Change your password'} onclose={closeSheet}>
    <div class="stack">
      <p class="dim">{rekey ? 'The vault gets a new key and is encrypted again. Any other way in stops working until you add it back.' : 'The vault key stays the same, so a backup made earlier still opens with the old password.'}</p>
      <PasswordField bind:value={oldPassword} placeholder="Current password" autofocus />
      <PasswordField bind:value={newPassword} placeholder="New password" />
      <PasswordField bind:value={newAgain} placeholder="New password again" error={mismatch} describedby={mismatch ? "mismatch-note" : undefined} onenter={() => changePassword(rekey)} />
      {#if mismatch}<p class="danger-text small" role="alert" id="mismatch-note">Passwords do not match.</p>{/if}
      {#if needed?.some((f) => f.kind === 'key_file')}
        <p class="dim">The current password also needs its key file.</p>
        <div><Btn label={factorChosen ? 'Key file chosen' : 'Choose the key file'} icon="key" disabled={busy} onclick={() => chooseFactorFile(rekey)} /></div>
      {/if}
      {#if needed?.some((f) => f.kind !== 'key_file')}
        <p class="dim">{hardwareAvailable() ? 'The current password also needs its security key. Plug it in and touch it.' : 'The current password also needs its security key, which this app cannot reach here.'}</p>
        {#if hardwareAvailable()}<div><Btn label="Try the key again" icon="security_key" disabled={busy} onclick={() => tapFactor(rekey)} /></div>{/if}
      {/if}
      {#if error}<p class="danger-text" role="alert">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={closeSheet} />
      <Btn label={busy ? 'Working…' : 'Change'} variant="primary" disabled={busy || !oldPassword || !newPassword || newPassword !== newAgain} onclick={() => changePassword(rekey)} />
    {/snippet}
  </Dialog>
{:else if sheet === 'decoy'}
  <Dialog title="A decoy vault" onclose={closeSheet}>
    <div class="stack">
      <p class="dim">A second vault, opened by its own password. The door looks the same either way and nothing in the file says it exists. This resists a quick look under pressure. It does not stand up to forensics, or to someone who has an older copy of your files.</p>
      <Field bind:value={name} placeholder="Its name" />
      <PasswordField bind:value={newPassword} placeholder="Its password" autofocus />
      <PasswordField bind:value={newAgain} placeholder="Its password again" error={mismatch} describedby={mismatch ? "mismatch-note" : undefined} onenter={makeDecoy} />
      {#if mismatch}<p class="danger-text small" role="alert" id="mismatch-note">Passwords do not match.</p>{/if}
      {#if error}<p class="danger-text" role="alert">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={closeSheet} />
      <Btn label={busy ? 'Making it…' : 'Make it'} variant="primary" disabled={busy || !newPassword || newPassword !== newAgain} onclick={makeDecoy} />
    {/snippet}
  </Dialog>
{:else if sheet === 'kit'}
  <Dialog title="Emergency kit" onclose={closeKit}>
    <div class="stack">
      <p class="dim">Your key is split into shares. Any {threshold} of the {total} people can open your vault together, and fewer learn nothing. They can do it at once: any waiting period is a promise between you, not something software can enforce.</p>
      {#if shares}
        <p>Save this file, print it, and delete the file once each person has their share.</p>
      {:else}
        <div class="form-row">
          <span class="label">People</span>
          <Select
            label="People"
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
          <Select label="Needed together" value={threshold} options={Array.from({ length: total - 1 }, (_, i) => [i + 2, String(i + 2)] as [number, string])} onchange={(n) => (threshold = n)} />
        </div>
      {/if}
      {#if error}<p class="danger-text" role="alert">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Close" onclick={closeKit} />
      {#if shares}
        <Btn label="Save the kit" icon="download" variant="primary" onclick={saveKit} />
      {:else}
        <Btn label={busy ? 'Making them…' : 'Make the shares'} variant="primary" disabled={busy} onclick={makeShares} />
      {/if}
    {/snippet}
  </Dialog>
{:else if sheet}
  <Dialog title={sheet.title} onclose={closeSheet}>
    <div class="stack">
      <p class="dim">{sheet.body}</p>
      <Field bind:value={label} placeholder="A name only you will see" autofocus />
      <PasswordField bind:value={newPassword} placeholder="Password" />
      <PasswordField bind:value={newAgain} placeholder="Password again" error={mismatch} describedby={mismatch ? "mismatch-note" : undefined} onenter={addPassword} />
      {#if mismatch}<p class="danger-text small" role="alert" id="mismatch-note">Passwords do not match.</p>{/if}
      {#if error}<p class="danger-text" role="alert">{error}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={closeSheet} />
      <Btn label={busy ? 'Adding…' : 'Add'} variant="primary" disabled={busy || !newPassword || newPassword !== newAgain} onclick={addPassword} />
    {/snippet}
  </Dialog>
{/if}
