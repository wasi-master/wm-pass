<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Fold from '../parts/Fold.svelte';
  import Icon from '../parts/Icon.svelte';
  import Tile from '../parts/Tile.svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import type { IconName } from '../engine/icons.svelte';
  import type { HealthView } from '../engine/types';
  import { ui } from './ui.svelte';
  import { breachCounts } from '../settings/net.svelte';

  interface Issue {
    key: string;
    title: string;
    why: string;
    lead: string;
    icon: IconName;
    ids: string[];
    serious: boolean;
  }

  let health = $state<HealthView | null>(null);
  let merging = $state<string[] | null>(null);

  $effect(() => {
    void pass.revision;
    void pass.health({ ...breachCounts.counts }).then((h) => (health = h));
  });

  const issues = $derived.by((): Issue[] => {
    const h = health;
    if (!h) return [];
    const hidden = prefs.listOf('health_hidden');
    return (
      [
        { key: 'breached', title: 'In a known breach', why: 'These passwords have appeared in published leaks. Change them first.', lead: 'in a known breach', icon: 'dangerous', ids: h.breached.map((b) => b[0]), serious: true },
        { key: 'reused', title: 'Used more than once', why: 'One leak opens every site that shares the password.', lead: 'used more than once', icon: 'swap_horiz', ids: h.reused.flat(), serious: true },
        { key: 'weak', title: 'Easy to guess', why: 'These would fall quickly to an attacker with a password list.', lead: 'that are easy to guess', icon: 'gpp_bad', ids: h.weak, serious: true },
        { key: 'insecure', title: 'Unencrypted sites', why: 'These sites are saved with http, so a password could be read on the way.', lead: 'on unencrypted sites', icon: 'warning', ids: h.insecure_url, serious: false },
        { key: 'expiring', title: 'Expiring soon', why: 'Their reminders or expiry dates are close.', lead: 'expiring soon', icon: 'schedule', ids: h.expiring, serious: false },
        { key: 'old', title: 'Not changed in a year', why: 'Worth changing if the site holds anything you care about.', lead: 'you have not changed in a year', icon: 'history', ids: h.old, serious: false },
        { key: 'twofactor', title: 'No second factor', why: 'These sites offer one-time codes and you have none saved.', lead: 'with no second factor', icon: 'pin', ids: h.missing_two_factor, serious: false },
        { key: 'duplicates', title: 'Duplicate items', why: 'The same login saved more than once. Merging keeps both histories.', lead: 'saved twice', icon: 'layers', ids: h.duplicates.flat(), serious: false },
      ] as Issue[]
    ).filter((issue) => issue.ids.length > 0 && !hidden.includes(issue.key));
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

  async function merge(keep: string) {
    const group = merging ?? [];
    merging = null;
    for (const other of group.filter((id) => id !== keep)) {
      await pass.change('health.merge_duplicates', { keep, remove: other });
    }
  }
</script>

<div class="settings-pane scroll">
  <div class="settings-inner stack" style="gap:6px">
    <h1 class="page-title">Health</h1>
    {#if prefs.v.health_score}
      <div class="gap" style="align-items:baseline">
        <span class="tnum" style="font-size:34px;font-weight:500;line-height:1.1">{health?.score ?? '-'}</span>
        <span class="dim">out of 100</span>
      </div>
      <div class="progress"><i style="width:{health?.score ?? 0}%;background:{scoreColour}"></i></div>
    {/if}
    <p class="dim" style="padding:8px 0 4px">{line}</p>

    {#each issues as issue (issue.key)}
      {@const ids = [...new Set(issue.ids)]}
      <Fold title="{issue.title}, {ids.length}" summary={issue.why}>
        <p class="dim small" style="padding:4px 0">{issue.why}</p>
        {#each ids as id}
          {@const e = entry(id)}
          {#if e}
            <button type="button" class="row" onclick={() => ui.open(id)}>
              <Tile title={e.title} site={e.site} kind={e.kind} label={e.color} icon={e.icon} size={28} />
              <span class="grow"><span class="title ell" style="display:block">{e.title}</span>{#if e.subtitle}<span class="sub ell" style="display:block">{e.subtitle}</span>{/if}</span>
              <span class="dimmer" style="display:flex"><Icon name="chevron_right" /></span>
            </button>
          {/if}
        {/each}
        {#if issue.key === 'duplicates'}
          {#each health?.duplicates ?? [] as group}
            <div style="padding-top:6px"><Btn label="Merge {group.length} copies of {entry(group[0])?.title ?? 'this item'}" icon="layers" onclick={() => (merging = group)} /></div>
          {/each}
        {/if}
      </Fold>
    {/each}

    <div class="gap wrap" style="padding-top:14px">
      <Btn label="Check for breaches" icon="local_fire_department" onclick={() => ui.go('settings/breaches')} />
      <Btn label="Compare two passwords" icon="balance" onclick={() => ui.go('settings/compare')} />
    </div>
  </div>
</div>

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
