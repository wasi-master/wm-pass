<script lang="ts">
  import Field from '../parts/Field.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { BUFFER, type GenState } from './gen.svelte';

  let { state: gen }: { state: GenState } = $props();

  const reroll = () => void gen.roll();
  let patternTimer: ReturnType<typeof setTimeout>;
  function patternTyped() {
    clearTimeout(patternTimer);
    patternTimer = setTimeout(reroll, 200);
  }
</script>

{#snippet slider(label: string, value: number, min: number, max: number, set: (v: number) => void, settle: () => void)}
  <div class="form-row">
    <span class="label">{label}</span>
    <span class="gap">
      <input type="range" {min} {max} {value} aria-label={label} oninput={(e) => set(Number(e.currentTarget.value))} onchange={settle} />
      <span class="tnum" style="width:24px;text-align:right">{value}</span>
    </span>
  </div>
{/snippet}

{#snippet flag(label: string, on: boolean, flip: () => void)}
  <button type="button" class="chip" aria-pressed={on} onclick={() => { flip(); reroll(); }}>{label}</button>
{/snippet}

<div class="stack" style="gap:6px" data-help-anchor="generator.controls">
  <div class="form-row" data-help-anchor="generator.modes">
    <span class="label">Type</span>
    <span>
      <Segmented
        value={gen.mode}
        options={[['password', 'Random'], ['passphrase', 'Words'], ['pronounceable', 'Pronounceable'], ['pin', 'PIN'], ['pattern', 'Pattern']]}
        onchange={(mode) => { gen.mode = mode; reroll(); }}
      />
    </span>
  </div>
  {#if gen.mode === 'password'}
    {@render slider('Length', gen.length, 8, BUFFER, (v) => (gen.length = v), () => void gen.settle())}
    <div class="form-row">
      <span class="label">Characters</span>
      <span class="gap wrap">
        {@render flag('a-z', gen.lower, () => (gen.lower = !gen.lower))}
        {@render flag('A-Z', gen.upper, () => (gen.upper = !gen.upper))}
        {@render flag('0-9', gen.digits, () => (gen.digits = !gen.digits))}
        {@render flag('!@#', gen.symbols, () => (gen.symbols = !gen.symbols))}
        {@render flag('Avoid look-alikes', gen.noAmbiguous, () => (gen.noAmbiguous = !gen.noAmbiguous))}
      </span>
    </div>
  {:else if gen.mode === 'passphrase'}
    {@render slider('Words', gen.words, 3, 12, (v) => (gen.words = v), reroll)}
    <div class="form-row">
      <span class="label">Between words</span>
      <span class="gap wrap">
        {#each [['-', 'Dash'], [' ', 'Space'], ['.', 'Dot'], ['_', 'Underscore']] as [sep, name]}
          <button type="button" class="chip" aria-pressed={gen.separator === sep} onclick={() => { gen.separator = sep; reroll(); }}>{name}</button>
        {/each}
      </span>
    </div>
    <div class="form-row">
      <span class="label">Also</span>
      <span class="gap wrap">
        {@render flag('Capitals', gen.capitalize, () => (gen.capitalize = !gen.capitalize))}
        {@render flag('Number', gen.includeDigit, () => (gen.includeDigit = !gen.includeDigit))}
      </span>
    </div>
  {:else if gen.mode === 'pronounceable'}
    {@render slider('Syllables', gen.syllables, 3, 14, (v) => (gen.syllables = v), reroll)}
  {:else if gen.mode === 'pin'}
    {@render slider('Digits', gen.pin, 4, 12, (v) => (gen.pin = v), reroll)}
  {:else}
    <div class="form-row">
      <span class="label">Pattern</span>
      <Field bind:value={gen.pattern} mono placeholder="Cvcv-####" oninput={patternTyped} />
    </div>
    <p class="dim small">C is a consonant, v a vowel, # a digit, ! a symbol. Anything else stays as typed.</p>
  {/if}
</div>
