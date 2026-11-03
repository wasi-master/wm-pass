<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Icon from '../parts/Icon.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import { ui } from '../vault/ui.svelte';
  import Rich from './Rich.svelte';
  import { help } from './help.svelte';

  // Once, after the first vault opens on this install: where the everyday things are, and a tour.
  const wide = $derived(!ui.narrow);
  const rows: { icon: IconName; title: string; text: () => string }[] = [
    { icon: 'search', title: 'Find anything', text: () => (wide ? 'Type in the box above the list. {{key:search}} takes you there from anywhere, and {{key:palette}} opens quick search.' : 'Type in the box above the list.') },
    { icon: 'add', title: 'Add an item', text: () => 'The **+** button above the list adds a login, a card, a note, a one-time code and more.' },
    { icon: 'content_copy', title: 'Copy without showing it', text: () => 'Choose an item to see it. Copy buttons put a secret on the clipboard, and it is cleared again after a while.' },
    { icon: 'help', title: 'Ask what anything does', text: () => 'Every setting has a **?** beside it, {{key:help}} explains the page you are on, and Settings, Guide has the rest.' },
  ];

  async function tour() {
    help.dismissWelcome();
    help.startWindowTour();
  }
</script>

<Dialog title="Your vault is ready" onclose={() => help.dismissWelcome()}>
  <div class="stack" style="gap:14px">
    <p class="dim">A minute on where things are. You can come back to this in Settings, Guide.</p>
    {#each rows as row (row.title)}
      <div class="gap" style="align-items:flex-start;gap:12px">
        <span class="dim" style="display:flex;padding-top:1px"><Icon name={row.icon} size={20} /></span>
        <div class="grow">
          <div style="font-weight:500">{row.title}</div>
          <div class="dim"><Rich text={row.text()} /></div>
        </div>
      </div>
    {/each}
  </div>
  {#snippet foot()}
    <Btn label="Not now" variant="quiet" onclick={() => help.dismissWelcome()} />
    <Btn label="Show me around" variant="primary" onclick={tour} />
  {/snippet}
</Dialog>
