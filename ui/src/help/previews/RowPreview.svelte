<script lang="ts">
  import Icon from '../../parts/Icon.svelte';
  import Tile from '../../parts/Tile.svelte';
  import { prefs } from '../../engine/prefs.svelte';
  import { SAMPLE_ROWS, type SampleRow } from './sample';

  // The vault list as yours looks now, with the one setting being explained set to the choice shown.
  let { value, pref }: { value?: unknown; pref?: string } = $props();

  function pick<T>(key: string, current: T): T {
    return (pref === key && value !== undefined ? value : current) as T;
  }
  const layout = $derived(pick('layout', prefs.v.layout));
  const icons = $derived(pick('row_icons', prefs.v.row_icons));
  const subtitle = $derived(pick('row_subtitle', prefs.v.row_subtitle));
  const marks = $derived(pick('row_badges', prefs.v.row_badges));
  const button = $derived(pick('row_button', prefs.v.row_button));
  const fold = $derived(pick('group_sites', prefs.v.group_sites));
  const compact = $derived(layout === 'compact');

  type Item = { one: SampleRow } | { stack: SampleRow[] };
  const items = $derived.by<Item[]>(() => {
    if (!fold) return SAMPLE_ROWS.map((one) => ({ one }));
    return [{ stack: SAMPLE_ROWS.slice(0, 2) }, ...SAMPLE_ROWS.slice(2).map((one) => ({ one }))];
  });
  const copies: Record<string, string> = { password: 'Copy password', username: 'Copy username', code: 'Copy code' };
  function canCopy(row: SampleRow) {
    return button === 'password' ? row.has_password : button === 'code' ? row.has_otp : button === 'username' ? row.kind === 'login' : false;
  }
</script>

{#if layout === 'grid'}
  <div class="grid" style="padding:0">
    {#each SAMPLE_ROWS as row (row.id)}
      <div class="card">
        {#if icons}<Tile title={row.title} site={row.site} kind={row.kind} size={40} />{/if}
        <span class="title ell small">{row.title}</span>
      </div>
    {/each}
  </div>
{:else}
  <div style="display:flex;flex-direction:column">
    {#each items as item, i (i)}
      {@const row = 'one' in item ? item.one : item.stack[0]}
      <div class="row" class:compact style="cursor:default">
        {#if icons}<Tile title={row.title} site={row.site} kind={row.kind} size={compact ? 20 : 32} />{/if}
        <span class="grow">
          <span class="gap" style="gap:4px">
            <span class="title ell">{row.title}</span>
            {#if marks}
              <span class="badges">
                {#if row.favourite}<span style="color:var(--warn);display:flex"><Icon name="star_fill" size={13} /></span>{/if}
                {#if row.has_otp}<Icon name="pin" size={13} />{/if}
                {#if row.attachments > 0}<Icon name="attach_file" size={13} />{/if}
              </span>
            {/if}
          </span>
          {#if !compact && subtitle}
            <span class="sub ell" style="display:block">{'stack' in item ? `${item.stack.length} logins` : row.subtitle}</span>
          {/if}
        </span>
        {#if 'stack' in item}<span class="dimmer" style="display:flex"><Icon name="expand_more" size={18} /></span>{/if}
        {#if 'one' in item && canCopy(row)}
          <span class="icon-btn" title={copies[button]} style="width:24px;height:24px"><Icon name="content_copy" size={16} /></span>
        {/if}
      </div>
    {/each}
  </div>
{/if}
<p class="preview-caption">Sample items. {button !== 'none' && layout !== 'grid' ? 'The copy button shows when the pointer is over a row.' : ''}</p>
