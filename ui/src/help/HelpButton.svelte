<script lang="ts">
  import IconBtn from '../parts/IconBtn.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { help } from './help.svelte';

  // The "?" beside a setting, a page title or a screen. It opens the guide's page about it.
  let { id, title, always = false }: { id: string; title: string; always?: boolean } = $props();
</script>

{#if always || prefs.v.help_buttons !== 'off'}
  <IconBtn
    icon="help"
    label="About {title}"
    size={16}
    class="tiny help-btn"
    onclick={(event) => {
      event.stopPropagation();
      void help.open(id);
    }}
  />
{/if}
