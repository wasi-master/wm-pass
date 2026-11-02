<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { BUILT_IN, applyTheme, contrast, rgb, type ThemeColors, type ThemeFile } from '../theme/themes';

  // Starts from any theme. Saving a built-in one makes a copy, so the original stays as it was.
  let { source, onclose }: { source: ThemeFile | null; onclose: () => void } = $props();

  const base = source ?? BUILT_IN[0];
  const isMine = !!source && prefs.customThemes.some((t) => t.id === source.id);
  let name = $state(isMine ? base.name : source ? `${base.name} copy` : 'My theme');
  let light = $state<ThemeColors | null>(base.light ? { ...base.light } : null);
  let dark = $state<ThemeColors | null>(base.dark ? { ...base.dark } : null);
  let editingDark = $state(document.documentElement.dataset.dark === '1' ? !!base.dark : !base.light);
  let corners = $state(base.corners);
  let font = $state(base.font);
  let preview = $state<HTMLElement>();

  type Slot = { label: string; key: keyof ThemeColors; accent?: boolean };
  const SLOTS: Slot[] = [
    { label: 'Accent', key: 'primary', accent: true },
    { label: 'Background', key: 'background' },
    { label: 'Surface', key: 'surface' },
    { label: 'Raised surface', key: 'surfaceHigh' },
    { label: 'Highest surface', key: 'surfaceHighest' },
    { label: 'Text', key: 'text' },
    { label: 'Secondary text', key: 'textMid' },
    { label: 'Faint text', key: 'textLow' },
    { label: 'Danger', key: 'danger' },
    { label: 'Warning', key: 'warn' },
    { label: 'Information', key: 'info' },
  ];

  const current = $derived(editingDark ? dark : light);

  function draft(): ThemeFile {
    return {
      id: isMine ? base.id : `custom_${crypto.randomUUID().slice(0, 8)}`,
      name: name.trim().slice(0, 40),
      light: $state.snapshot(light),
      dark: $state.snapshot(dark),
      corners,
      font,
      format: 'wmpass-theme',
      version: 1,
    };
  }

  $effect(() => {
    if (!preview) return;
    applyTheme(draft(), editingDark, false, preview);
  });

  function setSlot(slot: Slot, value: string) {
    if (!current) return;
    current[slot.key] = value;
    if (slot.accent) {
      // What was derived from the old accent follows the new one.
      current.primaryContainer = '';
      current.secondaryContainer = '';
      current.onPrimary = '';
    }
  }

  function ratio(value: string) {
    return contrast(rgb(value), rgb(current!.background)).toFixed(1);
  }

  function save() {
    const saved = draft();
    prefs.saveTheme(saved);
    prefs.set('theme_id', saved.id);
    onclose();
  }
</script>

<Dialog title={isMine ? 'Edit theme' : 'New theme'} wide {onclose}>
  <div class="stack">
    <Field bind:value={name} placeholder="Name" />
    <div>
      <Segmented
        value={editingDark ? 'dark' : 'light'}
        options={[['light', 'Light'], ['dark', 'Dark']]}
        onchange={(side) => {
          if (side === 'light' && !light) light = dark ? { ...dark } : null;
          if (side === 'dark' && !dark) dark = light ? { ...light } : null;
          editingDark = side === 'dark';
        }}
      />
    </div>
    <div bind:this={preview} style="border:1px solid var(--hair);border-radius:var(--r-l);padding:12px 14px;background:var(--bg);color:var(--text)">
      <div class="gap" style="gap:12px">
        <span style="width:32px;height:32px;border-radius:50%;background:var(--second-c);display:flex;align-items:center;justify-content:center;font-weight:500">G</span>
        <span class="grow"><span style="display:block;font-weight:500">GitHub</span><span class="small" style="display:block;color:var(--text2)">name@example.com</span></span>
        <span class="small" style="color:var(--text3)">3 days</span>
      </div>
      <p class="mono secret" style="padding:10px 0">hT<span class="d">7</span><span class="s">#</span>kQ<span class="d">2</span><span class="s">!</span>mZ</p>
      <div class="gap">
        <span class="btn primary">Copy</span>
        <span style="color:var(--warn);font-weight:500">Weak</span>
        <span style="color:var(--danger);font-weight:500">Breached</span>
      </div>
    </div>
    {#if current}
      <p class="section-title">Colours</p>
      <div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:2px 20px">
        {#each SLOTS as slot (slot.key)}
          {@const hex = (current[slot.key] as string) || '#000000'}
          <label class="color-field" style="min-height:30px">
            <input type="color" value={hex} aria-label={slot.label} oninput={(e) => setSlot(slot, e.currentTarget.value)} />
            <span class="grow">{slot.label}</span>
            <span class="mono dim small" title="Contrast against the background">{hex.toUpperCase()} · {ratio(hex)}</span>
          </label>
        {/each}
      </div>
    {/if}
    <p class="section-title">Corners</p>
    <div class="gap">
      <input type="range" min="0" max="28" value={corners} aria-label="Corners" oninput={(e) => (corners = Number(e.currentTarget.value))} />
      <span class="dim small" style="width:42px;text-align:right">{corners} px</span>
    </div>
    <p class="section-title">Font</p>
    <div><Segmented value={font} options={[['system', 'System'], ['inter', 'Inter'], ['serif', 'Serif'], ['mono', 'Mono']]} onchange={(f) => (font = f)} /></div>
  </div>
  {#snippet foot()}
    <Btn label="Cancel" onclick={onclose} />
    <Btn label="Save and use" variant="primary" disabled={!name.trim()} onclick={save} />
  {/snippet}
</Dialog>
