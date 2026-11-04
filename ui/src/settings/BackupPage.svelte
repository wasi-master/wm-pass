<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { shownName } from '../hosts/folders';
  import Choice from './Choice.svelte';
  import CopiesDialog from './CopiesDialog.svelte';
  import PlaceDialog from './PlaceDialog.svelte';
  import { kindOf } from './placeKinds';
  import { places, type Place } from './places.svelte';

  let adding = $state(false);
  let editing = $state<Place | null>(null);
  let acting = $state<Place | null>(null);
  let restoring = $state<Place | null>(null);
  let busy = $state<string | null>(null);
  let message = $state<string | null>(null);

  function whenShort(ms: number) {
    return new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
  }

  function subOf(place: Place) {
    const last = places.runs[place.id];
    if (last?.problem) return last.problem;
    if (last) return `Last copy ${whenShort(last.at_ms)}`;
    if (place.kind === 'folder') return shownName(place.path);
    if (place.kind === 'webdav') return place.url;
    if (place.kind === 's3') return place.bucket;
    return kindOf(place.kind)?.blurb ?? place.kind;
  }

  async function backUp(place: Place) {
    busy = place.id;
    const problem = await places.backupTo(place);
    busy = null;
    overlays.toast(problem ?? `Copied to ${place.label}.`);
  }

  async function saveFile() {
    await host().saveFile('Save a backup', 'wm-pass.wmbackup', await pass.bundle());
    message = 'The backup is saved. It opens with your vault’s password.';
  }

  function mergedText(merged: number) {
    return merged > 0 ? 'Merged into this vault. Anything newer here was kept, and older values moved to history.' : 'That file is under another key, so it holds some other vault.';
  }

  async function mergeFile() {
    const file = await host().pickFile('Choose a backup or vault file');
    if (file) message = mergedText(await pass.mergeIn(file.bytes));
  }

  function openCopies(place: Place) {
    acting = null;
    restoring = place;
  }

  async function bringBack(bytes: Uint8Array) {
    message = mergedText(await pass.mergeIn(bytes));
    restoring = null;
  }

  async function stopUsing(place: Place) {
    acting = null;
    const yes = await overlays.ask({ title: `Stop using ${place.label}?`, body: 'WM Pass forgets this place and its login. Copies already there stay where they are.', action: 'Stop using it', danger: true });
    if (yes) await places.remove(place.id);
  }
</script>

<p class="dim" style="padding-bottom:14px">A backup is the encrypted file itself, so it can sit anywhere and only your password opens it. It includes every vault on this computer, without showing how many there are.</p>

{#if places.available}
  <Group title="Where they go" help="group.backup.places" anchor="group:backup.places">
    {#each places.list as place (place.id)}
      <Row title={place.label} sub={subOf(place)} icon={place.kind === 'folder' ? 'folder' : place.kind === 'webdav' ? 'language' : 'database'} onclick={() => (acting = place)}>
        <IconBtn icon="backup" label="Back up to {place.label} now" disabled={busy === place.id} onclick={(event) => { event.stopPropagation(); void backUp(place); }} />
      </Row>
    {/each}
    <Row title="Add a place" help="action.add_place" icon="add" chevron onclick={() => (adding = true)} />
  </Group>
  <Group title="Schedule" anchor="group:backup.schedule">
    <Choice title="How often" help="action.backup_schedule" sub={places.everyDays > 0 ? 'Runs while the vault is locked, since it only moves ciphertext. The newest copy is read back and checked before any older one is removed. The app has to be running.' : null} icon="schedule" value={places.everyDays} options={[[0, 'By hand'], [1, 'Every day'], [7, 'Every week']] as [number, string][]} onchange={(d) => places.setSchedule(d)} />
    <Choice title="How many to keep" help="action.backup_keep" icon="layers" value={places.keep} options={[[3, '3'], [7, '7'], [14, '14'], [30, '30']] as [number, string][]} onchange={(n) => places.setKeep(n)} />
  </Group>
{/if}

<Group title="By hand" anchor="group:backup.manual">
  <div class="set-body gap wrap">
    <Btn label="Save a file" icon="upload" onclick={saveFile} />
    <Btn label="Merge a file" icon="download" onclick={mergeFile} />
  </div>
</Group>
{#if message}<p class="dim">{message}</p>{/if}

{#if adding}
  <PlaceDialog onclose={() => (adding = false)} onsaved={(place) => void backUp(place)} />
{/if}
{#if editing}
  <PlaceDialog existing={editing} onclose={() => (editing = null)} />
{/if}
{#if acting}
  {@const place = acting}
  <Dialog title={place.label} onclose={() => (acting = null)}>
    <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
      <Row title="Bring back a copy" help="action.restore_copy" sub="Merge an older copy into this vault" icon="history" chevron onclick={() => openCopies(place)} />
      <Row title="Change the details" icon="edit" chevron onclick={() => { editing = place; acting = null; }} />
      <Row title="Stop using it" help="action.place_stop" icon="delete" danger chevron onclick={() => stopUsing(place)} />
    </div>
  </Dialog>
{/if}
{#if restoring}<CopiesDialog place={restoring} onclose={() => (restoring = null)} onchoose={bringBack} />{/if}
