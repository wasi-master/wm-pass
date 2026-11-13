<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Qr from '../parts/Qr.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { bytesOf } from '../engine/frame';
  import { host } from '../engine/host';
  import { prefs } from '../engine/prefs.svelte';
  import { BUILT_IN, readTheme, themeToCode, themeToText, type ThemeFile } from '../theme/themes';
  import { copyPlainLine } from './clipboard';
  import ThemeEditor from './ThemeEditor.svelte';

  let editing = $state<{ source: ThemeFile | null } | null>(null);
  let sharing = $state<ThemeFile | null>(null);
  let importing = $state(false);
  let text = $state('');
  let problem = $state<string | null>(null);

  // Read again each time a theme reaches the page, or the cards keep the side they first showed.
  const dark = $derived.by(() => {
    void prefs.themeApplied;
    return document.documentElement.dataset.dark === '1';
  });

  function side(theme: ThemeFile) {
    return dark ? (theme.dark ?? theme.light)! : (theme.light ?? theme.dark)!;
  }

  function kinds(theme: ThemeFile) {
    return theme.light && theme.dark ? 'Light and dark' : theme.dark ? 'Dark only' : 'Light only';
  }

  function take(raw: string) {
    try {
      const theme = readTheme(raw);
      prefs.saveTheme(theme);
      prefs.set('theme_id', theme.id);
      importing = false;
      text = '';
      problem = null;
    } catch (failure: any) {
      problem = failure?.message ?? 'That is not a theme WM Pass can read.';
    }
  }

  async function fromFile() {
    try {
      const file = await host().pickFile('Choose a theme file', ['wmtheme', 'json']);
      if (file) take(new TextDecoder().decode(file.bytes));
    } catch (failure: any) {
      problem = failure?.message ?? 'That file could not be read.';
    }
  }

  async function saveFile(theme: ThemeFile) {
    // A name in another script has no letters left here, and a file called ".wmtheme" hides itself.
    const stem = theme.name.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-+|-+$/g, '').toLowerCase() || 'theme';
    try {
      await host().saveFile('Save the theme', `${stem}.wmtheme`, bytesOf(themeToText(theme)));
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The theme could not be saved.');
    }
  }

  async function more(event: MouseEvent, theme: ThemeFile) {
    const mine = prefs.customThemes.some((t) => t.id === theme.id);
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const choice = await overlays.popup(
      [
        { id: 'edit', label: mine ? 'Edit…' : 'Customize a copy…' },
        { id: 'share', label: 'Share…' },
        ...(mine ? [{ separator: true }, { id: 'delete', label: 'Delete…', danger: true }] : []),
      ],
      // Opened by keyboard, a click has no pointer position: the menu goes under the button.
      event.clientX || box.left,
      event.clientY || box.bottom,
    );
    if (choice === 'edit') editing = { source: theme };
    if (choice === 'share') sharing = theme;
    if (choice === 'delete') {
      const body = prefs.syncs('custom_themes') ? 'This removes the theme here and on your other devices that sync settings with this vault.' : 'This removes the theme from this computer.';
      const yes = await overlays.ask({ title: `Delete ${theme.name}?`, body, action: 'Delete', danger: true });
      if (yes) prefs.deleteTheme(theme.id);
    }
  }
</script>

{#snippet card(theme: ThemeFile)}
  {@const c = side(theme)}
  <div style="position:relative">
    <button type="button" class="theme-card" style="width:100%" aria-pressed={prefs.v.theme_id === theme.id} onclick={() => prefs.set('theme_id', theme.id)}>
      <span class="swatch" style="background:{c.background};padding:8px;gap:4px;align-items:flex-end">
        <i style="flex:none;width:14px;height:14px;border-radius:50%;background:{c.primary}"></i>
        <i style="flex:none;width:14px;height:14px;border-radius:50%;background:{c.text}"></i>
        <i style="flex:none;width:14px;height:14px;border-radius:50%;background:{c.danger}"></i>
        <i style="flex:none;width:14px;height:14px;border-radius:50%;background:{c.info}"></i>
      </span>
      <span class="name">
        <span class="ell" style="display:block;font-weight:500">{theme.name}</span>
        <span class="dim" style="display:block;font-size:var(--fs-xs)">{kinds(theme)}</span>
      </span>
    </button>
    {#if prefs.v.theme_id === theme.id}<span class="ok-text" style="position:absolute;top:6px;right:6px;display:flex;pointer-events:none"><Icon name="check_circle" size={16} /></span>{/if}
    <span style="position:absolute;bottom:4px;right:2px"><IconBtn icon="more_horiz" label="More for {theme.name}" size={16} class="tiny" onclick={(e) => more(e, theme)} /></span>
  </div>
{/snippet}

{#if prefs.customThemes.length > 0}
  <Group title="Yours" help="group.themes.yours" />
  <div class="theme-grid" style="margin:-12px 0 18px">{#each prefs.customThemes as theme (theme.id)}{@render card(theme)}{/each}</div>
{/if}
<p class="section-title" style="padding-top:0">Themes</p>
<div class="theme-grid" style="margin-bottom:18px">{#each BUILT_IN as theme (theme.id)}{@render card(theme)}{/each}</div>
<div class="gap">
  <Btn label="New theme" icon="add" variant="primary" onclick={() => (editing = { source: null })} />
  <Btn label="Import" icon="download" onclick={() => { problem = null; importing = true; }} />
</div>

{#if editing}
  <ThemeEditor source={editing.source} onclose={() => (editing = null)} />
{/if}

{#if sharing}
  {@const theme = sharing}
  {@const code = themeToCode(theme)}
  <Dialog title="Share {theme.name}" onclose={() => (sharing = null)}>
    <div class="stack">
      <p class="dim">A theme holds colors and a few settings. Nothing from your vault is in it.</p>
      <Qr text={code} />
    </div>
    {#snippet foot()}
      <Btn label="Close" onclick={() => (sharing = null)} />
      <Btn label="Save as a file" icon="download" onclick={() => saveFile(theme)} />
      <Btn label="Copy" icon="content_copy" variant="primary" onclick={() => copyPlainLine('Theme', code)} />
    {/snippet}
  </Dialog>
{/if}

{#if importing}
  <Dialog title="Import a theme" onclose={() => (importing = false)}>
    <div class="stack">
      <Field bind:value={text} multiline rows={4} mono placeholder="Paste a theme code" autofocus />
      {#if problem}<p class="danger-text small" role="alert">{problem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (importing = false)} />
      <Btn label="Choose a file" icon="folder" onclick={fromFile} />
      <Btn label="Import" variant="primary" disabled={!text.trim()} onclick={() => take(text)} />
    {/snippet}
  </Dialog>
{/if}
