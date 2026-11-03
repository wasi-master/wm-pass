<script lang="ts">
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import { prefs, type PrefKey } from '../engine/prefs.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import type { HelpedPref, TopicId } from '$help';

  let {
    title,
    sub,
    icon,
    pref,
    help,
    onchange,
  }: {
    title: string;
    sub?: string | null;
    icon?: IconName;
    /** Only a setting the guide explains: a switch for one it does not is a type error. */
    pref: Extract<PrefKey, HelpedPref>;
    help?: TopicId | null;
    onchange?: (value: boolean) => void;
  } = $props();
  // A setting's topic is named after its key, so every switch explains itself.
  const topic = $derived(help === undefined ? (`setting.${pref}` as TopicId) : help);
</script>

<Row {title} {sub} {icon} help={topic}>
  <Switch
    checked={prefs.v[pref] === true}
    label={title}
    onchange={(value) => {
      prefs.set(pref, value as never);
      onchange?.(value);
    }}
  />
</Row>
