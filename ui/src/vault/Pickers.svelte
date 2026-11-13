<script lang="ts" module>
  export type PickerKind = 'folder' | 'tags' | 'colour' | 'icon' | 'reminder' | 'destruct' | 'link';
</script>

<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import TagChip from '../parts/TagChip.svelte';
  import Tile from '../parts/Tile.svelte';
  import { brands } from '../engine/brands.svelte';
  import { GLYPH_CHOICES, kindIcon, oklch } from '../engine/face';
  import { glyphName } from '../engine/glyphs';
  import { nameText } from '../engine/text';
  import { tags as tagStore } from '../engine/tags.svelte';
  import { ui } from './ui.svelte';
  import { pass } from '../engine/pass.svelte';
  import { overlays } from '../parts/floating.svelte';
  import type { Detail } from '../engine/types';

  // The small questions an entry asks: which folder, what colour, when to remind. With `onpick`,
  // the answer goes to the editor rather than straight to the vault.
  let {
    kind,
    entry,
    current,
    look,
    suggested = [],
    onclose,
    onpick,
  }: {
    kind: PickerKind;
    entry?: Detail;
    current?: any;
    /** How the item looks, for previewing Automatic and the site's own colour. */
    look?: { title: string; site: string | null; kind: string; color: string | null; icon: string | null };
    /** Tags the item's site and kind suggest. */
    suggested?: string[];
    onclose: () => void;
    onpick?: (value: any) => void;
  } = $props();

  const HUES = [25, 55, 85, 120, 150, 180, 210, 240, 270, 300, 330, 355];
  /** What each hue is called, for a screen reader and the pointer's tooltip. */
  const HUE_NAMES: Record<number, string> = {
    25: 'Red',
    55: 'Orange',
    85: 'Amber',
    120: 'Lime',
    150: 'Green',
    180: 'Teal',
    210: 'Cyan',
    240: 'Blue',
    270: 'Indigo',
    300: 'Purple',
    330: 'Magenta',
    355: 'Pink',
  };
  let text = $state('');

  const folder = $derived(current !== undefined ? current : entry?.folder);
  // What was just chosen, until the item comes back with it: two quick clicks build on each other
  // rather than the second undoing the first.
  let localTags = $state<string[] | null>(null);
  let localLinks = $state<string[] | null>(null);
  const same = (a: string[], b: string[]) => a.length === b.length && a.every((x) => b.includes(x));
  $effect(() => {
    if (localTags && entry && same(entry.tags, localTags)) localTags = null;
    if (localLinks && entry && same(entry.links, localLinks)) localLinks = null;
  });
  const tags = $derived<string[]>(current !== undefined ? current : (localTags ?? entry?.tags ?? []));
  const links = $derived<string[]>(localLinks ?? entry?.links ?? []);
  const colour = $derived(current !== undefined ? current : entry?.color);
  const iconNow = $derived(current !== undefined ? current : entry?.icon);

  const seen = $derived(look ?? (entry ? { title: entry.title, site: entry.site, kind: entry.kind, color: entry.color, icon: entry.icon } : { title: '', site: null, kind: 'login', color: null, icon: null }));

  /** How many tags show before the "more" chip, past the item's own. */
  const TAGS_AT_FIRST = 12;
  let everyTag = $state(false);
  const allTags = $derived(tagStore.mostUsed());
  const shownTags = $derived.by(() => {
    const q = text.trim().toLowerCase();
    const matching = allTags.filter((t) => !q || t.toLowerCase().includes(q));
    if (everyTag || q) return matching;
    return [...new Set([...tags.filter((t) => matching.includes(t)), ...matching.filter((t) => !tags.includes(t)).slice(0, TAGS_AT_FIRST)])];
  });
  const hiddenTags = $derived(allTags.filter((t) => !text.trim() || t.toLowerCase().includes(text.trim().toLowerCase())).length - shownTags.length);
  const offeredTags = $derived(suggested.filter((t) => !tags.includes(t) && !allTags.includes(t)));

  const glyphs = $derived.by(() => {
    const q = text.trim().toLowerCase();
    return Object.entries(GLYPH_CHOICES).filter(([id]) => !q || id.includes(q) || glyphName(id).toLowerCase().includes(q));
  });
  const matches = $derived.by(() => {
    const q = text.trim().toLowerCase();
    return brands.all().filter((b) => !q || b.name.toLowerCase().includes(q) || b.otherNames.some((n) => n.toLowerCase().includes(q)));
  });
  // Brands draw a page at a time as the list scrolls, so all of them are there without searching.
  let brandLimit = $state(96);
  $effect(() => {
    void text;
    brandLimit = 96;
  });
  function more(node: HTMLElement) {
    const watcher = new IntersectionObserver((seen) => {
      if (seen.some((s) => s.isIntersecting)) brandLimit += 96;
    });
    watcher.observe(node);
    return { destroy: () => watcher.disconnect() };
  }
  const others = $derived.by(() => {
    const q = text.trim().toLowerCase();
    return pass.entries.filter((e) => e.id !== entry?.id && (!q || e.title.toLowerCase().includes(q))).slice(0, 60);
  });

  async function setCommon(args: Record<string, unknown>) {
    try {
      await pass.change('entry.common', { id: entry!.id, ...args });
    } catch (failure: any) {
      localTags = null;
      localLinks = null;
      overlays.toast(failure?.message ?? 'That could not be changed.');
    }
  }

  function choose(field: string, value: unknown) {
    if (onpick) onpick(value);
    else void setCommon({ [field]: value });
    onclose();
  }

  function toggleTag(tag: string) {
    const next = tags.includes(tag) ? tags.filter((t) => t !== tag) : [...tags, tag];
    if (onpick) onpick(next);
    else {
      localTags = next;
      void setCommon({ tags: next });
    }
  }

  function toggleLink(id: string) {
    const next = links.includes(id) ? links.filter((l) => l !== id) : [...links, id];
    localLinks = next;
    void setCommon({ links: next });
  }

  // Typing a tag the item already has keeps it, rather than taking it away.
  function addTyped() {
    const tag = nameText(text);
    if (!tag) return;
    text = '';
    if (tags.some((t) => t.toLowerCase() === tag.toLowerCase())) return;
    // A tag the vault already has, typed in other capitals, is that tag rather than a second one,
    // as a folder typed again is.
    const same = (t: string) => t.toLowerCase() === tag.toLowerCase();
    toggleTag(allTags.find(same) ?? suggested.find(same) ?? tag);
  }

  let adding = false;
  async function addFolder() {
    const name = nameText(text);
    if (!name || adding) return;
    // A folder of that name already there is the one meant, rather than a second of the same name.
    const existing = pass.folders.find((f) => f.name.toLowerCase() === name.toLowerCase());
    if (existing) return choose('folder', existing.id);
    adding = true;
    try {
      const reply = await pass.change<{ id: string }>('folder.add', { name });
      choose('folder', reply.json.id);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The folder could not be made.');
    } finally {
      adding = false;
    }
  }

  function setReminder(days: number | null) {
    const r = entry!.reminders;
    void setCommon({ reminders: { change_every_days: days, expires_ms: r.expires_ms, warn_days: r.warn_days } });
    onclose();
  }

  function destructIn(after: number | null) {
    void setCommon({ destroy_at_ms: after === null ? null : Date.now() + after });
    onclose();
  }

  const HOUR = 3_600_000;
  const titles: Record<PickerKind, string> = {
    folder: 'Move to',
    tags: 'Tags',
    colour: 'Color',
    icon: 'Icon',
    reminder: 'Remind me to change it',
    destruct: 'Destroy itself',
    link: 'Link to',
  };
</script>

<Dialog title={titles[kind]} wide={kind === 'icon'} {onclose}>
  {#if kind === 'folder'}
    <div class="stack">
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        <button type="button" class="set-row" aria-current={!folder} onclick={() => choose('folder', null)}>
          <Icon name="inventory_2" /><span class="text">No folder</span>{#if !folder}<Icon name="check" />{/if}
        </button>
        {#each pass.folders as f (f.id)}
          <button type="button" class="set-row" aria-current={folder === f.id} onclick={() => choose('folder', f.id)}>
            <Icon name="folder" /><span class="text">{f.name}</span>{#if folder === f.id}<Icon name="check" />{/if}
          </button>
        {/each}
      </div>
      <div class="gap">
        <div class="grow"><Field bind:value={text} placeholder="New folder" onenter={addFolder} /></div>
        <Btn label="Add" disabled={!text.trim()} onclick={addFolder} />
      </div>
    </div>
  {:else if kind === 'tags'}
    <div class="stack">
      {#if offeredTags.length}
        <div>
          <p class="dim small" style="padding-bottom:6px">Suggested</p>
          <div class="gap wrap">
            {#each offeredTags as tag (tag)}
              <button type="button" class="chip" title={tag} onclick={() => toggleTag(tag)}><Icon name="add" size={14} /><span class="ell">{tag}</span></button>
            {/each}
          </div>
        </div>
      {/if}
      {#if allTags.length}
        <div>
          <p class="dim small" style="padding-bottom:6px">Click to add or take off. Right-click one to rename or delete it.</p>
          <div class="gap wrap">
            {#each shownTags as tag (tag)}
              {@const on = tags.includes(tag)}
              <TagChip {tag} selected={on} onclick={() => toggleTag(tag)} onremove={on ? () => toggleTag(tag) : undefined} onmanage={() => (ui.managingTag = tag)} />
            {/each}
            {#if hiddenTags > 0}
              <button type="button" class="chip" onclick={() => (everyTag = true)}><Icon name="expand_more" size={14} />{hiddenTags} more</button>
            {/if}
          </div>
        </div>
      {/if}
      <div class="gap">
        <div class="grow"><Field bind:value={text} placeholder="New tag" onenter={addTyped} /></div>
        <Btn label="Add" disabled={!text.trim()} onclick={addTyped} />
      </div>
    </div>
  {:else if kind === 'colour'}
    <div class="stack">
      <div class="gap wrap" role="group" aria-label="Colors">
        {#each HUES as hue}
          <button
            type="button"
            aria-label={HUE_NAMES[hue]}
            title={HUE_NAMES[hue]}
            aria-pressed={colour === String(hue)}
            style="width:30px;height:30px;border-radius:50%;padding:0;border:{colour === String(hue) ? '2px solid var(--text)' : '0'};background:{oklch(0.58, 0.11, hue)}"
            onclick={() => choose('color', String(hue))}
          ></button>
        {/each}
      </div>
      <button type="button" class="set-row" aria-pressed={colour == null} style="border:1px solid {colour == null ? 'var(--primary)' : 'var(--hair)'};border-radius:var(--r-m)" onclick={() => choose('color', null)}>
        <Tile title={seen.title} site={seen.site} kind={seen.kind} label={null} icon={seen.icon} size={28} />
        <span class="text">Use the site's own color</span>
        {#if colour == null}<Icon name="check" />{/if}
      </button>
    </div>
  {:else if kind === 'icon'}
    <div class="stack">
      <button type="button" class="set-row" aria-pressed={!iconNow} style="border:1px solid {iconNow ? 'var(--hair)' : 'var(--primary)'};border-radius:var(--r-m)" onclick={() => choose('icon', null)}>
        <Tile title={seen.title} site={seen.site} kind={seen.kind} label={seen.color} icon={null} size={36} />
        <span class="text"><span style="display:block">Automatic</span><span class="sub" style="display:block">The site's own mark when it is known, or a letter</span></span>
        {#if !iconNow}<Icon name="check" />{/if}
      </button>
      <Field bind:value={text} placeholder="Search icons and {brands.all().length} brands" />
      <div class="scroll" style="max-height:420px;padding-right:4px">
        {#if glyphs.length}
          <p class="dim small gap" style="padding:4px 2px 6px"><span class="grow">Symbols</span><span>{glyphs.length}</span></p>
          <div class="gap wrap" style="gap:6px">
            {#each glyphs as [id, glyph] (id)}
              <button
                type="button"
                class="icon-btn"
                class:on={iconNow === `glyph:${id}`}
                title={glyphName(id)}
                aria-label={glyphName(id)}
                aria-pressed={iconNow === `glyph:${id}`}
                style="width:40px;height:40px;background:var(--s1)"
                onclick={() => choose('icon', `glyph:${id}`)}
              ><Icon name={glyph} size={20} /></button>
            {/each}
          </div>
        {/if}
        {#if matches.length}
          <p class="dim small gap" style="padding:14px 2px 6px"><span class="grow">Brands</span><span>{matches.length}</span></p>
          <div class="gap wrap" style="gap:6px">
            {#each matches.slice(0, brandLimit) as brand (brand.slug)}
              <button
                type="button"
                title={brand.name}
                aria-label={brand.name}
                aria-pressed={iconNow === `brand:${brand.slug}`}
                style="padding:2px;border:2px solid {iconNow === `brand:${brand.slug}` ? 'var(--primary)' : 'transparent'};border-radius:10px;background:none"
                onclick={() => choose('icon', `brand:${brand.slug}`)}
              >
                <Tile title={brand.name} icon="brand:{brand.slug}" size={34} />
              </button>
            {/each}
            <!-- Made anew for each page: a page too short to scroll leaves it in view, and only a new watcher asks again. -->
            {#if brandLimit < matches.length}{#key brandLimit}<span use:more style="width:1px;height:1px"></span>{/key}{/if}
          </div>
        {/if}
        {#if !glyphs.length && !matches.length}<p class="dim" style="padding:12px">Nothing called “{text.trim()}”.</p>{/if}
      </div>
      <p class="dim small">Hover over an icon to see its name. The brand marks come from Simple Icons (CC0) and belong to their owners.</p>
    </div>
  {:else if kind === 'reminder'}
    <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
      <button type="button" class="set-row" aria-current={!entry?.reminders.change_every_days} onclick={() => setReminder(null)}><span class="text">Never</span>{#if !entry?.reminders.change_every_days}<Icon name="check" />{/if}</button>
      {#each [30, 90, 180, 365] as days}
        <button type="button" class="set-row" aria-current={entry?.reminders.change_every_days === days} onclick={() => setReminder(days)}><span class="text">Every {days} days</span>{#if entry?.reminders.change_every_days === days}<Icon name="check" />{/if}</button>
      {/each}
    </div>
  {:else if kind === 'destruct'}
    <div class="stack">
      <p class="dim">On the day, its secrets and attachments are erased on every device that syncs. A copy taken before then still holds them.</p>
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        {#if entry?.destroy_at_ms}<button type="button" class="set-row" onclick={() => destructIn(null)}><span class="text">Keep it after all</span></button>{/if}
        <button type="button" class="set-row" onclick={() => destructIn(HOUR)}><span class="text">In an hour</span></button>
        <button type="button" class="set-row" onclick={() => destructIn(24 * HOUR)}><span class="text">Tomorrow</span></button>
        <button type="button" class="set-row" onclick={() => destructIn(7 * 24 * HOUR)}><span class="text">In a week</span></button>
        <button type="button" class="set-row" onclick={() => destructIn(30 * 24 * HOUR)}><span class="text">In a month</span></button>
      </div>
    </div>
  {:else if kind === 'link'}
    <div class="stack">
      <Field bind:value={text} placeholder="Search" />
      <div class="scroll" style="max-height:320px;border:1px solid var(--hair);border-radius:var(--r-m)">
        {#if others.length === 0}<p class="dim" style="padding:12px">{text.trim() ? `Nothing called “${text.trim()}”.` : 'There is nothing else to link to yet.'}</p>{/if}
        {#each others as other (other.id)}
          {@const on = links.includes(other.id)}
          <button
            type="button"
            class="set-row"
            aria-pressed={on}
            onclick={() => toggleLink(other.id)}
          >
            <Icon name={kindIcon(other.kind)} />
            <span class="text ell">{other.title || 'Untitled'}</span>
            {#if on}<Icon name="check" />{/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#snippet foot()}<Btn label="Done" onclick={onclose} />{/snippet}
</Dialog>
