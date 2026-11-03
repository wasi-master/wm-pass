<script lang="ts" generics="T extends string | number">
  import Row from '../parts/Row.svelte';
  import Select from '../parts/Select.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { prefs, type PrefKey } from '../engine/prefs.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import type { HelpedPref, TopicId } from '$help';

  // A setting with a few answers: side by side when there are few, a menu when there are many.
  let {
    title,
    sub,
    icon,
    pref,
    value,
    options,
    help,
    onchange,
  }: {
    title: string;
    sub?: string | null;
    icon?: IconName;
    /** Only a setting the guide explains: a choice for one it does not is a type error. */
    pref?: Extract<PrefKey, HelpedPref>;
    value?: T;
    options: [T, string][];
    help?: TopicId | null;
    onchange?: (value: T) => void;
  } = $props();

  const topic = $derived(help !== undefined ? help : pref ? (`setting.${pref}` as TopicId) : null);

  const current = $derived((pref ? prefs.v[pref] : value) as T);
  function picked(next: T) {
    if (pref) prefs.set(pref, next as never);
    onchange?.(next);
  }
</script>

<Row {title} {sub} {icon} help={topic}>
  {#if options.length <= 3}
    <Segmented value={current} {options} onchange={picked} />
  {:else}
    <Select value={current} {options} label={title} onchange={picked} />
  {/if}
</Row>
