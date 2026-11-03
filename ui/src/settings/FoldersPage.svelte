<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import IconBtn from '../parts/IconBtn.svelte';

  async function remove(id: string, name: string) {
    const yes = await overlays.ask({ title: `Remove “${name}”?`, body: 'What is in it stays in the vault, in no folder.', action: 'Remove', danger: true });
    if (yes) await pass.change('folder.remove', { id });
  }
</script>

<Group>
  {#each pass.folders as folder (folder.id)}
    <Row title={folder.name} icon="folder" value={String(pass.entries.filter((e) => e.folder === folder.id).length)}>
      <IconBtn icon="edit" label="Rename {folder.name}" onclick={() => (ui.renamingFolder = { id: folder.id, name: folder.name })} />
      <IconBtn icon="delete" label="Remove {folder.name}" onclick={() => remove(folder.id, folder.name)} />
    </Row>
  {/each}
  <Row title="New folder" help="action.new_folder" icon="add" chevron onclick={() => (ui.renamingFolder = { id: '', name: '' })} />
</Group>
