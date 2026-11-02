<script lang="ts">
  import Row from '../parts/Row.svelte';
  import Switch from '../parts/Switch.svelte';
  import { prefs, type PrefKey } from '../engine/prefs.svelte';
  import type { IconName } from '../engine/icons.svelte';

  let { title, sub, icon, pref, onchange }: { title: string; sub?: string | null; icon?: IconName; pref: PrefKey; onchange?: (value: boolean) => void } = $props();
</script>

<Row {title} {sub} {icon}>
  <Switch
    checked={prefs.v[pref] === true}
    label={title}
    onchange={(value) => {
      prefs.set(pref, value as never);
      onchange?.(value);
    }}
  />
</Row>
