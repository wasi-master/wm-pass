<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { ui } from '../vault/ui.svelte';
  import { ROUTE_COLOURS } from './nav.svelte';
  import { hubSections } from './shown';

  // A page of pages, as on the phone: the settings list names what a thing is about, and this page
  // opens each of its pages.
  let { id }: { id: string } = $props();
</script>

{#each hubSections(id) as [section, pages] (section)}
  <Group title={section || undefined}>
    {#each pages as info (info.id)}
      <Row title={info.title} sub={info.sub} icon={info.icon} accent={ROUTE_COLOURS[info.id]} chevron onclick={() => ui.go(`settings/${info.id}`)} />
    {/each}
  </Group>
{/each}
