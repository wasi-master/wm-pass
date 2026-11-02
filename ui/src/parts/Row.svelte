<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { IconName } from '../engine/icons.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Icon from './Icon.svelte';
  import { settingsAccent, highlight } from '../settings/nav.svelte';

  // One line of settings: a coloured tile, words, and a control on the right.
  let {
    title,
    sub,
    icon,
    accent,
    value,
    onclick,
    chevron,
    danger = false,
    children,
  }: {
    title: string;
    sub?: string | null;
    icon?: IconName;
    accent?: string;
    value?: string | null;
    onclick?: () => void;
    chevron?: boolean;
    danger?: boolean;
    children?: Snippet;
  } = $props();

  const tint = $derived(accent ?? settingsAccent());
  const style = $derived(prefs.v.settings_icons);
  let element = $state<HTMLElement>();
  const flash = $derived(highlight.title === title);
  $effect(() => {
    if (flash && element) element.scrollIntoView({ block: 'center' });
  });
</script>

{#snippet inside()}
  {#if icon && style !== 'off'}
    {#if style === 'bare'}
      <span class="tile-glyph bare" style="color:{tint ?? 'var(--text2)'}"><Icon name={icon} size={18} /></span>
    {:else if style === 'mono' || !tint}
      <span class="tile-glyph mono"><Icon name={icon} size={16} /></span>
    {:else}
      <span class="tile-glyph" style="background:{tint}"><Icon name={icon} size={16} /></span>
    {/if}
  {/if}
  <span class="text">
    <span class:danger-text={danger} style="display:block">{title}</span>
    {#if sub}<span class="sub" style="display:block">{sub}</span>{/if}
  </span>
  {#if value}<span class="dim ell" style="max-width:45%">{value}</span>{/if}
  {@render children?.()}
  {#if chevron}<span class="dimmer" style="display:flex"><Icon name="chevron_right" size={18} /></span>{/if}
{/snippet}

{#if onclick}
  <button bind:this={element} type="button" class="set-row" class:flash {onclick}>{@render inside()}</button>
{:else}
  <div bind:this={element} class="set-row" class:flash>{@render inside()}</div>
{/if}
