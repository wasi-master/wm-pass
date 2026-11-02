<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { host } from '../engine/host';
  import { blankPlace, places, type Place } from './places.svelte';

  // Add or change a place to keep copies. Works at the door too, where no vault is open yet, so a
  // computer can be restored from a place that was set up elsewhere.
  let { existing = null, onclose, onsaved }: { existing?: Place | null; onclose: () => void; onsaved?: (place: Place) => void } = $props();

  const NAMES: Record<string, string> = { folder: 'Folder', webdav: 'WebDAV', s3: 'S3' };
  const kinds = host().places?.kinds ?? [];
  let place = $state<Place>(existing ? { ...existing } : blankPlace(kinds[0] ?? 'folder'));
  let problem = $state<string | null>(null);
  let busy = $state(false);

  const ready = $derived(
    place.kind === 'folder'
      ? !!place.path
      : place.kind === 'webdav'
        ? !!place.url.trim() && !!place.user.trim()
        : !!place.bucket.trim() && !!place.access_key.trim() && !!place.secret_key,
  );

  async function chooseFolder() {
    const folder = await host().places!.chooseFolder();
    if (folder) place.path = folder;
  }

  async function save() {
    busy = true;
    problem = null;
    const saved: Place = {
      ...$state.snapshot(place),
      label: place.label.trim() || (NAMES[place.kind] ?? place.kind),
      url: place.url.trim(),
      user: place.user.trim(),
      endpoint: place.endpoint.trim(),
      region: place.region.trim() || 'us-east-1',
      bucket: place.bucket.trim(),
      prefix: place.prefix.trim(),
      access_key: place.access_key.trim(),
    };
    try {
      await places.put(saved);
      onsaved?.(saved);
      onclose();
    } catch (failure: any) {
      problem = failure.message;
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title={existing ? existing.label : 'A place for copies'} {onclose}>
  <div class="stack">
    {#if !existing}
      <Segmented value={place.kind} options={kinds.map((k) => [k, NAMES[k] ?? k] as [string, string])} onchange={(k) => (place.kind = k)} />
    {/if}
    <p class="dim">
      {place.kind === 'folder'
        ? 'Pick a folder. Syncthing, Nextcloud or a cloud provider’s own app can carry it from there.'
        : 'Only the encrypted file goes there, so the server cannot read it. Its login is kept on this computer in the system’s keychain.'}
    </p>
    <Field bind:value={place.label} placeholder="A name for it" />
    {#if place.kind === 'folder'}
      <Btn label={place.path ? place.path : 'Choose a folder'} icon="folder" onclick={chooseFolder} wide />
    {:else if place.kind === 'webdav'}
      <Field bind:value={place.url} placeholder="https://example.com/remote.php/dav/files/you/" />
      <Field bind:value={place.user} placeholder="User name" />
      <PasswordField bind:value={place.password} placeholder="Password" />
    {:else}
      <Field bind:value={place.endpoint} placeholder="Endpoint, empty for AWS" />
      <Field bind:value={place.region} placeholder="Region" />
      <Field bind:value={place.bucket} placeholder="Bucket" />
      <Field bind:value={place.prefix} placeholder="Folder inside it, if any" />
      <Field bind:value={place.access_key} placeholder="Access key" />
      <PasswordField bind:value={place.secret_key} placeholder="Secret key" />
    {/if}
    {#if problem}<p class="danger-text">{problem}</p>{/if}
  </div>
  {#snippet foot()}
    <Btn label="Cancel" onclick={onclose} />
    <Btn label="Save" variant="primary" disabled={!ready || busy} onclick={save} />
  {/snippet}
</Dialog>
