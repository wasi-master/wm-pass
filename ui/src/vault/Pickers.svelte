<script lang="ts" module>
  export type PickerKind = 'folder' | 'tags' | 'colour' | 'icon' | 'reminder' | 'destruct' | 'link';
</script>

<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Icon from '../parts/Icon.svelte';
  import Tile from '../parts/Tile.svelte';
  import { brands } from '../engine/brands.svelte';
  import { GLYPH_CHOICES, kindIcon, oklch } from '../engine/face';
  import { pass } from '../engine/pass.svelte';
  import type { Detail } from '../engine/types';

  // The small questions an entry asks: which folder, what colour, when to remind. With `onpick`,
  // the answer goes to the editor rather than straight to the vault.
  let {
    kind,
    entry,
    current,
    onclose,
    onpick,
  }: {
    kind: PickerKind;
    entry?: Detail;
    current?: any;
    onclose: () => void;
    onpick?: (value: any) => void;
  } = $props();

  const HUES = [25, 55, 85, 120, 150, 180, 210, 240, 270, 300, 330, 355];
  let text = $state('');

  const folder = $derived(current !== undefined ? current : entry?.folder);
  const tags = $derived<string[]>(current !== undefined ? current : (entry?.tags ?? []));
  const colour = $derived(current !== undefined ? current : entry?.color);
  const iconNow = $derived(current !== undefined ? current : entry?.icon);

  const allTags = $derived([...new Set(pass.entries.flatMap((e) => e.tags))].sort());
  const matches = $derived.by(() => {
    const q = text.trim().toLowerCase();
    return brands
      .all()
      .filter((b) => !q || b.name.toLowerCase().includes(q) || b.otherNames.some((n) => n.toLowerCase().includes(q)))
      .slice(0, 60);
  });
  const others = $derived.by(() => {
    const q = text.trim().toLowerCase();
    return pass.entries.filter((e) => e.id !== entry?.id && (!q || e.title.toLowerCase().includes(q))).slice(0, 60);
  });

  async function setCommon(args: Record<string, unknown>) {
    await pass.change('entry.common', { id: entry!.id, ...args });
  }

  function choose(field: string, value: unknown) {
    if (onpick) onpick(value);
    else void setCommon({ [field]: value });
    onclose();
  }

  function toggleTag(tag: string) {
    const next = tags.includes(tag) ? tags.filter((t) => t !== tag) : [...tags, tag];
    if (onpick) onpick(next);
    else void setCommon({ tags: next });
  }

  async function addFolder() {
    const name = text.trim();
    if (!name) return;
    const reply = await pass.change<{ id: string }>('folder.add', { name });
    choose('folder', reply.json.id);
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
    colour: 'Colour',
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
        <button type="button" class="set-row" onclick={() => choose('folder', null)}>
          <Icon name="inventory_2" /><span class="text">No folder</span>{#if !folder}<Icon name="check" />{/if}
        </button>
        {#each pass.folders as f (f.id)}
          <button type="button" class="set-row" onclick={() => choose('folder', f.id)}>
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
      {#if allTags.length}
        <div class="gap wrap">
          {#each allTags as tag}
            <button type="button" class="chip" aria-pressed={tags.includes(tag)} onclick={() => toggleTag(tag)}>{tag}</button>
          {/each}
        </div>
      {/if}
      <div class="gap">
        <div class="grow"><Field bind:value={text} placeholder="New tag" onenter={() => { if (text.trim()) { toggleTag(text.trim()); text = ''; } }} /></div>
        <Btn label="Add" disabled={!text.trim()} onclick={() => { toggleTag(text.trim()); text = ''; }} />
      </div>
    </div>
  {:else if kind === 'colour'}
    <div class="stack">
      <div class="gap wrap">
        {#each HUES as hue}
          <button
            type="button"
            aria-label="Colour {hue}"
            style="width:30px;height:30px;border-radius:50%;padding:0;border:{colour === String(hue) ? '2px solid var(--text)' : '0'};background:{oklch(0.58, 0.11, hue)}"
            onclick={() => choose('color', String(hue))}
          ></button>
        {/each}
      </div>
      <div><Btn label="Use the site's own colour" onclick={() => choose('color', null)} /></div>
    </div>
  {:else if kind === 'icon'}
    <div class="stack">
      <div><Btn label="Automatic" onclick={() => choose('icon', null)} /></div>
      <div class="gap wrap">
        {#each Object.entries(GLYPH_CHOICES) as [name, glyph]}
          <button
            type="button"
            class="icon-btn"
            class:on={iconNow === `glyph:${name}`}
            title={name}
            aria-label={name}
            style="width:34px;height:34px;background:var(--s1)"
            onclick={() => choose('icon', `glyph:${name}`)}
          ><Icon name={glyph} /></button>
        {/each}
      </div>
      <Field bind:value={text} placeholder="Search {brands.all().length} brands" />
      <div class="gap wrap">
        {#each matches as brand (brand.slug)}
          <button type="button" title={brand.name} aria-label={brand.name} style="padding:0;border:0;background:none" onclick={() => choose('icon', `brand:${brand.slug}`)}>
            <Tile title={brand.name} icon="brand:{brand.slug}" size={34} />
          </button>
        {/each}
      </div>
    </div>
  {:else if kind === 'reminder'}
    <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
      <button type="button" class="set-row" onclick={() => setReminder(null)}><span class="text">Never</span>{#if !entry?.reminders.change_every_days}<Icon name="check" />{/if}</button>
      {#each [30, 90, 180, 365] as days}
        <button type="button" class="set-row" onclick={() => setReminder(days)}><span class="text">Every {days} days</span>{#if entry?.reminders.change_every_days === days}<Icon name="check" />{/if}</button>
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
        {#each others as other (other.id)}
          {@const on = entry?.links.includes(other.id)}
          <button
            type="button"
            class="set-row"
            onclick={() => setCommon({ links: on ? entry!.links.filter((l) => l !== other.id) : [...entry!.links, other.id] })}
          >
            <Icon name={kindIcon(other.kind)} />
            <span class="text ell">{other.title}</span>
            {#if on}<Icon name="check" />{/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#snippet foot()}<Btn label="Done" onclick={onclose} />{/snippet}
</Dialog>
