<script lang="ts">
  import type { IconName } from '../engine/icons.svelte';
  import Icon from './Icon.svelte';

  let {
    icon,
    label,
    on = false,
    disabled = false,
    size = 18,
    menu = false,
    class: extra = '',
    onclick,
  }: {
    icon: IconName;
    label: string;
    on?: boolean;
    disabled?: boolean;
    size?: number;
    /** It opens a menu, which a screen reader says before it is pressed. */
    menu?: boolean;
    class?: string;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button type="button" class="icon-btn {extra}" class:on title={label} aria-label={label} aria-haspopup={menu ? 'menu' : undefined} {disabled} {onclick}>
  <Icon name={icon} {size} />
</button>
