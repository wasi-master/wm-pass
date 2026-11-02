<script lang="ts">
  import type { Snippet } from 'svelte';

  // A text box. Everything is off that would remember or send what is typed.
  let {
    value = $bindable(''),
    placeholder = '',
    type = 'text',
    mono = false,
    multiline = false,
    rows = 3,
    error = false,
    disabled = false,
    autofocus = false,
    label,
    lead,
    trail,
    onenter,
    oninput,
    onkeydown,
    ref = $bindable(),
  }: {
    value?: string;
    placeholder?: string;
    type?: string;
    mono?: boolean;
    multiline?: boolean;
    rows?: number;
    error?: boolean;
    disabled?: boolean;
    autofocus?: boolean;
    label?: string;
    lead?: Snippet;
    trail?: Snippet;
    onenter?: () => void;
    oninput?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    ref?: HTMLInputElement | HTMLTextAreaElement;
  } = $props();

  function keys(event: KeyboardEvent) {
    onkeydown?.(event);
    if (event.defaultPrevented) return;
    if (event.key === 'Enter' && onenter && !multiline) {
      event.preventDefault();
      onenter();
    }
  }

  $effect(() => {
    if (autofocus && ref) ref.focus();
  });
</script>

<div class="input" class:error>
  {#if lead}<span class="lead">{@render lead()}</span>{/if}
  {#if multiline}
    <textarea
      bind:this={ref}
      bind:value
      class:mono
      {rows}
      {placeholder}
      {disabled}
      aria-label={label ?? placeholder}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      {oninput}
      onkeydown={keys}
    ></textarea>
  {:else}
    <input
      bind:this={ref}
      bind:value
      class:mono
      {type}
      {placeholder}
      {disabled}
      aria-label={label ?? placeholder}
      autocomplete="off"
      autocapitalize="off"
      autocorrect="off"
      spellcheck="false"
      data-1p-ignore
      data-lpignore="true"
      {oninput}
      onkeydown={keys}
    />
  {/if}
  {#if trail}<span class="trail">{@render trail()}</span>{/if}
</div>
