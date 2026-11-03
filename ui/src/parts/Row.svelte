<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { TopicId } from '$help';
  import type { IconName } from '../engine/icons.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import Icon from './Icon.svelte';
  import { overlays } from './floating.svelte';
  import { settingsAccent, highlight } from '../settings/nav.svelte';
  import HelpButton from '../help/HelpButton.svelte';
  import { help as guide } from '../help/help.svelte';

  // One line of settings: a coloured tile, words, a "?" when the guide explains it, and a control
  // on the right. A row that does something when clicked is still a plain box: a button stretched
  // across it takes the click, so the controls and the "?" inside are not buttons in a button.
  let {
    title,
    sub,
    icon,
    accent,
    value,
    onclick,
    chevron,
    danger = false,
    help,
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
    /** The guide's topic for this row. */
    help?: TopicId | null;
    children?: Snippet;
  } = $props();

  const tint = $derived(accent ?? settingsAccent());
  const style = $derived(prefs.v.settings_icons);
  let element = $state<HTMLElement>();
  const flash = $derived(highlight.title === title);
  $effect(() => {
    if (flash && element) element.scrollIntoView({ block: 'center' });
  });

  async function menu(event: MouseEvent) {
    if (!help) return;
    event.preventDefault();
    const choice = await overlays.popup([{ id: 'help', label: 'What is this?' }], event.clientX, event.clientY);
    if (choice === 'help') void guide.open(help);
  }
</script>

<div bind:this={element} class="set-row" class:flash class:act={!!onclick} role="presentation" oncontextmenu={menu}>
  {#if onclick}<button type="button" class="hit" aria-label={sub ? `${title}. ${sub}` : title} {onclick}></button>{/if}
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
  {#if help}<HelpButton id={help} {title} />{/if}
  {#if value}<span class="dim ell" style="max-width:45%">{value}</span>{/if}
  {@render children?.()}
  {#if chevron}<span class="dimmer" style="display:flex"><Icon name="chevron_right" size={18} /></span>{/if}
</div>
