<script lang="ts">
  import { brands } from '../engine/brands.svelte';
  import { faceOf, type Face } from '../engine/face';
  import { prefs } from '../engine/prefs.svelte';
  import Icon from './Icon.svelte';

  let {
    title = '',
    site = null,
    kind = 'login',
    label = null,
    icon = null,
    size = 32,
    style: forcedStyle,
    shape: forcedShape,
  }: {
    title?: string;
    site?: string | null;
    kind?: string;
    label?: string | null;
    icon?: string | null;
    size?: number;
    style?: string;
    shape?: string;
  } = $props();

  function token(name: string) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#808080';
  }

  const face: Face = $derived.by(() => {
    void brands.ready;
    void prefs.themeApplied;
    return faceOf(title, site, kind, label, icon, forcedStyle ?? prefs.v.site_icon_style, forcedShape ?? prefs.v.site_icon_shape, {
      ground: token('--bg'),
      neutral: token('--s3'),
      second: token('--second-c'),
      text: token('--text'),
      text2: token('--text2'),
    });
  });

  const shapeName = $derived(forcedShape ?? prefs.v.site_icon_shape);
  const radius = $derived(shapeName === 'rounded' ? size * 0.28 : shapeName === 'square' ? size * 0.12 : shapeName === 'none' ? 0 : size / 2);
  const bare = $derived(shapeName === 'none');
  const drawing = $derived.by(() => {
    void brands.ready;
    return face.brand ? brands.drawing(face.brand) : null;
  });
</script>

<span
  style="display:inline-flex;align-items:center;justify-content:center;flex:none;width:{size}px;height:{size}px;border-radius:{radius}px;background:{face.fill};color:{face.ink}"
>
  {#if drawing}
    <svg width={size * (bare ? 0.72 : 0.52)} height={size * (bare ? 0.72 : 0.52)} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d={drawing} /></svg>
  {:else if face.glyph}
    <Icon name={face.glyph} size={size * (bare ? 0.72 : 0.56)} />
  {:else}
    <span style="font-size:{size * (bare ? 0.56 : 0.42)}px;font-weight:500;line-height:1">{face.letter}</span>
  {/if}
</span>
