<script lang="ts">
  import HelpButton from '../help/HelpButton.svelte';
  import Hint from '../help/Hint.svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Fold from '../parts/Fold.svelte';
  import Icon from '../parts/Icon.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import Tile from '../parts/Tile.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import type { Summary } from '../engine/types';
  import { ui } from './ui.svelte';
  import { breachCounts } from '../settings/net.svelte';
  import { CHECK_NAMES, healthMemory, healthNow, ignoreHealth, ignoredChecks } from './health.svelte';

  interface Issue {
    key: string;
    title: string;
    why: string;
    lead: string;
    icon: IconName;
    ids: string[];
    serious: boolean;
  }

  /** The checks a password change on the site fixes. */
  const CHANGE_FIXES = ['breached', 'reused', 'weak', 'old', 'twofactor'];

  let merging = $state<string[] | null>(null);
  let acting = $state<{ issue: Issue; entry: Summary } | null>(null);
  let change = $state<string | null>(null);
  let reviewing = $state(false);
  let byCount = $state(true);

  const health = $derived(healthMemory.last);

  $effect(() => {
    void pass.revision;
    void breachCounts.counts;
    void prefs.v.health_ignored;
    void prefs.v.health_skip_local;
    void healthNow().catch(() => {});
  });

  // What the site offers for changing the password, looked up as the item's choices open.
  $effect(() => {
    const now = acting;
    change = null;
    if (!now || !CHANGE_FIXES.includes(now.issue.key)) return;
    void pass
      .changePasswordUrl(now.entry.site)
      .then((url) => {
        if (acting === now) change = url;
      })
      .catch(() => {});
  });

  const issues = $derived.by((): Issue[] => {
    const h = health;
    if (!h) return [];
    const hidden = prefs.listOf('health_hidden');
    return (
      [
        { key: 'breached', title: CHECK_NAMES.breached, why: 'These passwords have appeared in published leaks. Change them first.', lead: 'in a known breach', icon: 'dangerous', ids: h.breached.map((b) => b[0]), serious: true },
        { key: 'reused', title: CHECK_NAMES.reused, why: 'One leak opens every site that shares the password.', lead: 'used more than once', icon: 'swap_horiz', ids: h.reused.flat(), serious: true },
        { key: 'weak', title: CHECK_NAMES.weak, why: 'These would fall quickly to an attacker with a password list.', lead: 'that are easy to guess', icon: 'gpp_bad', ids: h.weak, serious: true },
        { key: 'insecure', title: CHECK_NAMES.insecure, why: 'These sites are saved with http, so a password could be read on the way.', lead: 'on unencrypted sites', icon: 'warning', ids: h.insecure_url, serious: false },
        { key: 'expiring', title: CHECK_NAMES.expiring, why: 'Their reminders or expiry dates are close.', lead: 'expiring soon', icon: 'schedule', ids: h.expiring, serious: false },
        { key: 'old', title: CHECK_NAMES.old, why: 'Worth changing if the site holds anything you care about.', lead: 'you have not changed in a year', icon: 'history', ids: h.old, serious: false },
        { key: 'twofactor', title: CHECK_NAMES.twofactor, why: 'These sites offer one-time codes and you have none saved.', lead: 'with no second factor', icon: 'pin', ids: h.missing_two_factor, serious: false },
        { key: 'duplicates', title: CHECK_NAMES.duplicates, why: 'The same login saved more than once. Merging keeps both histories.', lead: 'saved twice', icon: 'layers', ids: h.duplicates.flat(), serious: false },
      ] as Issue[]
    ).filter((issue) => issue.ids.length > 0 && !hidden.includes(issue.key));
  });

  const counts = $derived(Object.fromEntries(health?.breached ?? []) as Record<string, number>);
  const ignored = $derived.by(() => {
    void prefs.v.health_ignored;
    return Object.entries(ignoredChecks()).flatMap(([check, ids]) => ids.map((id) => ({ check, id })));
  });

  const line = $derived.by(() => {
    if (!health) return ' ';
    if (pass.entries.length === 0) return 'Nothing to check yet.';
    if (issues.length === 0) return 'No problems found.';
    return `Start with the ones ${issues[0].lead}.`;
  });

  const scoreColour = $derived((health?.score ?? 0) >= 80 ? 'var(--primary)' : (health?.score ?? 0) >= 50 ? 'var(--warn)' : 'var(--danger)');

  function entry(id: string) {
    return pass.entries.find((e) => e.id === id);
  }

  function breachedOrder(ids: string[]) {
    const list = [...new Set(ids)];
    return byCount
      ? list.sort((a, b) => (counts[b] ?? 0) - (counts[a] ?? 0))
      : list.sort((a, b) => (entry(a)?.title ?? '').toLowerCase().localeCompare((entry(b)?.title ?? '').toLowerCase()));
  }

  function seenText(seen: number) {
    return seen === 1 ? 'Seen once in breaches' : `Seen ${seen.toLocaleString()} times in breaches`;
  }

  function fixMostUrgent() {
    const first = entry(issues[0].ids[0]);
    if (first) editItem(first);
  }

  /** The editor shows beside the list, so the list comes first. */
  function editItem(e: Summary) {
    ui.open(e.id);
    ui.edit(e.id, e.kind);
  }

  function setOpen(key: string, open: boolean) {
    healthMemory.open = open ? [...healthMemory.open.filter((k) => k !== key), key] : healthMemory.open.filter((k) => k !== key);
  }

  function act(what: 'change' | 'open' | 'edit' | 'ignore') {
    const now = acting;
    if (!now) return;
    acting = null;
    if (what === 'change' && change) void host().openUrl(change);
    else if (what === 'open') ui.open(now.entry.id);
    else if (what === 'edit') editItem(now.entry);
    else if (what === 'ignore') ignoreHealth(now.issue.key, now.entry.id, true);
  }

  async function merge(keep: string) {
    const group = merging ?? [];
    merging = null;
    for (const other of group.filter((id) => id !== keep)) {
      await pass.change('health.merge_duplicates', { keep, remove: other });
    }
  }
</script>

{#snippet item(issue: Issue, id: string, indent = false)}
  {@const e = entry(id)}
  {#if e}
    {@const seen = counts[id]}
    <button type="button" class="row" style={indent ? 'padding-left:22px' : ''} onclick={() => (acting = { issue, entry: e })}>
      <Tile title={e.title} site={e.site} kind={e.kind} label={e.color} icon={e.icon} size={28} />
      <span class="grow">
        <span class="title ell" style="display:block">{e.title}</span>
        {#if issue.key === 'breached' && seen}<span class="sub ell danger-text" style="display:block">{seenText(seen)}</span>
        {:else if e.subtitle}<span class="sub ell" style="display:block">{e.subtitle}</span>{/if}
      </span>
      <span class="dimmer" style="display:flex"><Icon name="more_vert" /></span>
    </button>
  {/if}
{/snippet}

<div class="settings-pane scroll">
  <div class="settings-inner stack" style="gap:6px">
    <div class="page-title-row">
      <h1 class="page-title">Health</h1>
      <HelpButton id="screen.health" title="Health" />
    </div>
    <Hint id="health" topic="screen.health" tour="tour.health" />
    {#if prefs.v.health_score}
      <div class="gap" style="align-items:baseline" data-help-anchor="health.score">
        <span class="tnum" style="font-size:34px;font-weight:500;line-height:1.1">{health?.score ?? '-'}</span>
        <span class="dim">out of 100</span>
      </div>
      <div class="progress"><i style="width:{health?.score ?? 0}%;background:{scoreColour}"></i></div>
    {/if}
    <p class="dim" style="padding:8px 0 4px">{line}</p>
    {#if issues.length > 0}
      <div style="padding-bottom:6px"><Btn label="Fix the most urgent" icon="bolt" variant="primary" onclick={fixMostUrgent} /></div>
    {/if}

    <div class="stack" style="gap:6px" data-help-anchor="health.groups">
    {#each issues as issue (issue.key)}
      {@const ids = [...new Set(issue.ids)]}
      <Fold title="{issue.title}, {ids.length}" summary={issue.why} open={healthMemory.open.includes(issue.key)} ontoggle={(open) => setOpen(issue.key, open)}>
        <p class="dim small" style="padding:4px 0">{issue.why}</p>
        {#if issue.key === 'reused'}
          {#each health?.reused ?? [] as set}
            <p class="dim small" style="padding:8px 0 2px;font-weight:500">{set.length} items share one password</p>
            {#each set as id}{@render item(issue, id, true)}{/each}
          {/each}
        {:else if issue.key === 'breached'}
          <div style="padding:4px 0"><Segmented value={byCount ? 'count' : 'name'} options={[['count', 'Most seen'], ['name', 'Name']]} onchange={(v) => (byCount = v === 'count')} /></div>
          {#each breachedOrder(issue.ids) as id}{@render item(issue, id)}{/each}
        {:else}
          {#each ids as id}{@render item(issue, id)}{/each}
        {/if}
        {#if issue.key === 'duplicates'}
          {#each health?.duplicates ?? [] as group}
            <div style="padding-top:6px"><Btn label="Merge {group.length} copies of {entry(group[0])?.title ?? 'this item'}" icon="layers" onclick={() => (merging = group)} /></div>
          {/each}
        {/if}
      </Fold>
    {/each}
    </div>

    <div class="gap wrap" style="padding-top:14px">
      {#if ignored.length > 0}<Btn label="Ignored ({ignored.length})" icon="visibility_off" onclick={() => (reviewing = true)} />{/if}
      <Btn label="Check for breaches" icon="local_fire_department" onclick={() => ui.go('settings/breaches')} />
      <Btn label="Compare two passwords" icon="balance" onclick={() => ui.go('settings/compare')} />
    </div>
  </div>
</div>

{#if acting}
  {@const now = acting}
  <Dialog title={now.entry.title || 'Untitled'} onclose={() => (acting = null)}>
    <div class="stack">
      <p class="dim">{now.issue.why}</p>
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        {#if change}
          <button type="button" class="set-row" onclick={() => act('change')}>
            <Icon name="open_in_new" />
            <span class="text">{now.issue.key === 'twofactor' ? 'Turn on a second factor on the site' : 'Change the password on the site'}<span class="sub ell" style="display:block">{change.replace(/^https:\/\//, '')}</span></span>
          </button>
        {/if}
        <button type="button" class="set-row" onclick={() => act('open')}>
          <Icon name="key" />
          <span class="text">Open the item</span>
        </button>
        <button type="button" class="set-row" onclick={() => act('edit')}>
          <Icon name="edit" />
          <span class="text">Edit</span>
        </button>
        <button type="button" class="set-row" onclick={() => act('ignore')}>
          <Icon name="visibility_off" />
          <span class="text">Ignore for this check<span class="sub" style="display:block">It stops counting against your score. Health lists what you ignored</span></span>
        </button>
      </div>
    </div>
    {#snippet foot()}<Btn label="Cancel" onclick={() => (acting = null)} />{/snippet}
  </Dialog>
{/if}

{#if reviewing}
  <Dialog title="Ignored" onclose={() => (reviewing = false)}>
    <div class="stack">
      <p class="dim">Click one to count it again.</p>
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        {#each ignored as { check, id } (`${check}:${id}`)}
          <button
            type="button"
            class="set-row"
            onclick={() => {
              ignoreHealth(check, id, false);
              if (ignored.length <= 1) reviewing = false;
            }}
          >
            <Icon name="visibility" />
            <span class="text">{entry(id)?.title ?? 'A deleted item'}<span class="sub" style="display:block">{CHECK_NAMES[check] ?? check}</span></span>
          </button>
        {/each}
      </div>
    </div>
    {#snippet foot()}<Btn label="Done" onclick={() => (reviewing = false)} />{/snippet}
  </Dialog>
{/if}

{#if merging}
  <Dialog title="Which one stays?" onclose={() => (merging = null)}>
    <div class="stack">
      <p class="dim">The others fold into it. Their passwords move to its history and their extra fields come along, so nothing is lost.</p>
      <div style="border:1px solid var(--hair);border-radius:var(--r-m);overflow:hidden">
        {#each merging as id}
          {@const e = entry(id)}
          {#if e}
            <button type="button" class="set-row" onclick={() => merge(id)}>
              <span class="text">{e.title}{#if e.subtitle}<span class="sub" style="display:block">{e.subtitle}</span>{/if}</span>
            </button>
          {/if}
        {/each}
      </div>
    </div>
    {#snippet foot()}<Btn label="Cancel" onclick={() => (merging = null)} />{/snippet}
  </Dialog>
{/if}
