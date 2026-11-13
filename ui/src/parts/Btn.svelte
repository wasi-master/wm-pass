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
    name,
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
    /** What a screen reader calls it, when the words on it say too little by themselves. It starts with them. */
    name?: string;
    onclick?: (event: MouseEvent) => void;
    type?: 'button' | 'submit';
    children?: Snippet;
  } = $props();
</script>

<button {type} class="btn {variant}" class:wide {disabled} {title} aria-label={name} {onclick}>
  {#if icon}<Icon name={icon} size={16} />{/if}
  {#if label}{label}{/if}
  {@render children?.()}
</button>
