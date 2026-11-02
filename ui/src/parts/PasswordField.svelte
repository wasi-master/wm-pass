<script lang="ts">
  import type { Snippet } from 'svelte';
  import Field from './Field.svelte';
  import IconBtn from './IconBtn.svelte';

  let {
    value = $bindable(''),
    placeholder = 'Password',
    error = false,
    disabled = false,
    autofocus = false,
    onenter,
    oninput,
    extra,
    ref = $bindable(),
    showing = $bindable(false),
  }: {
    value?: string;
    placeholder?: string;
    error?: boolean;
    disabled?: boolean;
    autofocus?: boolean;
    onenter?: () => void;
    oninput?: () => void;
    extra?: Snippet;
    ref?: HTMLInputElement | HTMLTextAreaElement;
    showing?: boolean;
  } = $props();
</script>

<Field bind:value bind:ref type={showing ? 'text' : 'password'} mono {placeholder} {error} {disabled} {autofocus} {onenter} {oninput}>
  {#snippet trail()}
    <IconBtn icon={showing ? 'visibility_off' : 'visibility'} label={showing ? 'Hide password' : 'Show password'} size={16} onclick={() => (showing = !showing)} />
    {@render extra?.()}
  {/snippet}
</Field>
