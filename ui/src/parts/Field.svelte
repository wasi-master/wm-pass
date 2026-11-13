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
    describedby,
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
    /** Ids of the words that explain the box, such as a hint under it or what is wrong with it. */
    describedby?: string;
    lead?: Snippet;
    trail?: Snippet;
    onenter?: () => void;
    oninput?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    ref?: HTMLInputElement | HTMLTextAreaElement;
  } = $props();

  function keys(event: KeyboardEvent) {
    // While an input method composes a word (Japanese, Chinese, Korean), Enter accepts the word and
    // Escape drops it: neither is meant for the form. 229 is how Safari marks such a key.
    if (event.isComposing || event.keyCode === 229) return;
    onkeydown?.(event);
    if (event.defaultPrevented) return;
    if (event.key === 'Enter' && onenter && !multiline) {
      event.preventDefault();
      onenter();
    }
  }

  // Disabling a field drops its focus, so an autofocused one takes it back when it comes back on,
  // with what is in it selected: after a wrong password, typing again replaces it.
  let wasDisabled = false;
  $effect(() => {
    const off = disabled;
    if (autofocus && ref && !off) {
      ref.focus();
      if (wasDisabled) ref.select();
    }
    wasDisabled = off;
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
      aria-invalid={error || undefined}
      aria-describedby={describedby || undefined}
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
      aria-invalid={error || undefined}
      aria-describedby={describedby || undefined}
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
