<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { IconName } from '../engine/icons.svelte';
  import Icon from './Icon.svelte';

  let {
    label,
    icon,
    variant = '',
    wide = false,
    disabled = false,
    title,
    onclick,
    type = 'button',
    children,
  }: {
    label?: string;
    icon?: IconName;
    variant?: '' | 'primary' | 'danger' | 'danger-fill' | 'quiet';
    wide?: boolean;
    disabled?: boolean;
    title?: string;
    onclick?: (event: MouseEvent) => void;
    type?: 'button' | 'submit';
    children?: Snippet;
  } = $props();
</script>

<button {type} class="btn {variant}" class:wide {disabled} {title} {onclick}>
  {#if icon}<Icon name={icon} size={16} />{/if}
  {#if label}{label}{/if}
  {@render children?.()}
</button>
