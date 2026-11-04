<script lang="ts">
  import { untrack } from 'svelte';
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Spinner from '../parts/Spinner.svelte';
  import Tile from '../parts/Tile.svelte';
  import { host, type UsernameSite } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import NetGate from './NetGate.svelte';
  import { networkExists, online } from './net.svelte';

  // `check` sites are asked by the engine, `page` sites are read in a hidden window, and `link`
  // sites are only opened. Every one comes with where a name's page lives there and its mark.
  type Answer = 'checking' | 'free' | 'taken' | 'unknown' | 'open' | { failed: string };

  let name = $state('');
  let known = $state.raw<UsernameSite[]>([]);
  let answers = $state<Record<string, Answer>>({});
  // The name the answers are for, which may no longer be what the field holds.
  let asked = $state('');
  let problem = $state<string | null>(null);

  // The extension has no network of its own, so the desktop app makes its checks.
  const remote = host().usernames;
  // Where no hidden window can be opened, a page site can only be looked at by hand.
  const pagesHere = !!remote || !!host().checkUsernamePage;

  async function loadSites() {
    try {
      known = remote ? await remote.siteList() : (await pass.engine<UsernameSite[]>('usernames.site_list', {}, {})).json;
      problem = null;
    } catch (failure: any) {
      problem = sentence(failure.message);
    }
  }

  $effect(() => {
    if (remote || networkExists()) untrack(() => void loadSites());
  });

  const sites = $derived(known.filter((s) => s.kind !== 'link'));
  const linked = $derived(known.filter((s) => s.kind === 'link'));
  const checkable = $derived(sites.filter((s) => s.kind === 'check' || pagesHere));

  function sentence(message: unknown) {
    const text = String(message ?? '') || 'No answer';
    return text.charAt(0).toUpperCase() + text.slice(1);
  }

  function page(site: UsernameSite, who: string) {
    return site.profile.replace('{}', encodeURIComponent(who));
  }

  function hostOf(site: UsernameSite) {
    try {
      return new URL(site.profile.replace('{}', 'x')).hostname.replace(/^www\./, '');
    } catch {
      return '';
    }
  }

  function shortPage(site: UsernameSite, who: string) {
    return page(site, who).replace(/^https:\/\//, '').replace(/^www\./, '');
  }

  async function ask(site: UsernameSite, who: string): Promise<string> {
    if (remote) return remote.check(site.name, who);
    if (site.kind === 'page') return host().checkUsernamePage!(site.name, who, online());
    return (await pass.engine<{ answer: string }>('usernames.check', { site: site.name, name: who }, { online: online() })).json.answer;
  }

  function check(site: UsernameSite, who: string) {
    answers[site.name] = 'checking';
    ask(site, who).then(
      (word) => {
        if (asked === who) answers[site.name] = word === 'free' || word === 'taken' ? word : 'unknown';
      },
      (failure: any) => {
        if (asked === who) answers[site.name] = { failed: sentence(failure?.message ?? failure) };
      },
    );
  }

  async function checkAll() {
    const who = name.trim();
    if (!who || running) return;
    asked = who;
    answers = {};
    // The desktop app may have connected since the page opened.
    if (remote && known.length === 0) await loadSites();
    for (const site of sites) {
      if (site.kind === 'page' && !pagesHere) answers[site.name] = 'open';
      else check(site, who);
    }
  }

  const values = $derived(checkable.map((s) => answers[s.name]).filter((a) => a !== undefined));
  const running = $derived(values.some((a) => a === 'checking'));
  const done = $derived(values.filter((a) => a !== 'checking').length);
  const free = $derived(values.filter((a) => a === 'free').length);
  const unclear = $derived(values.filter((a) => a === 'unknown' || typeof a === 'object').length);

  const WORDS: Record<string, string> = { free: 'Free', taken: 'Taken', unknown: 'No clear answer' };
</script>

<NetGate
  remote={!!remote}
  what={`${remote ? 'The desktop app asks for this extension, and only while network features are allowed there too. ' : ''}Asks every site at once whether a name is taken. Each site learns the name you looked up. TikTok and Instagram only answer a browser, so their pages load in a hidden, private window that runs their scripts and keeps nothing afterwards. X, Threads, Reddit and a few others refuse or give the same answer for every name, so their profile pages are linked for you to open yourself.`}
>
  <div class="stack" style="padding-bottom:14px;max-width:420px">
    <Field bind:value={name} placeholder="A username" onenter={checkAll} />
    <div><Btn label={running ? 'Checking…' : 'Check'} icon="person_search" variant="primary" disabled={running || !name.trim()} onclick={checkAll} /></div>
    {#if remote && known.length === 0 && problem}<p class="dim">Open the desktop app and connect it under Settings, Desktop app, to check names from here.</p>{/if}
    {#if problem}<p class="danger-text">{problem}</p>{/if}
    {#if values.length > 0}
      <div class="stack" style="gap:6px">
        <p class="dim">
          {running
            ? `Checked ${done} of ${checkable.length}`
            : unclear > 0
              ? `Free on ${free} of ${checkable.length}. ${unclear} gave no clear answer.`
              : `Free on ${free} of ${checkable.length}.`}
        </p>
        <div class="progress" role="progressbar" aria-label="Sites checked" aria-valuemin={0} aria-valuemax={checkable.length} aria-valuenow={done}>
          <i style="width:{checkable.length === 0 ? 0 : (done * 100) / checkable.length}%"></i>
        </div>
      </div>
    {/if}
  </div>

  {#if sites.length > 0}
    <Group title="{sites.length} sites">
      {#each sites as site (site.name)}
        {@const answer = answers[site.name]}
        {@const failed = typeof answer === 'object' ? answer.failed : null}
        {@const opens = !!asked && answer !== undefined && answer !== 'checking'}
        <div class="set-row" class:act={opens}>
          {#if opens}<button type="button" class="hit" aria-label="Open {shortPage(site, asked)}" onclick={() => void host().openUrl(page(site, asked))}></button>{/if}
          <span style="display:flex;pointer-events:none"><Tile title={site.name} icon={site.icon} size={30} /></span>
          <span class="text">
            <span class="ell" style="display:block">{site.name}</span>
            <span class="sub ell" class:danger-text={!!failed} style="display:block">
              {failed ?? (site.kind === 'page' && !pagesHere ? 'Can only be checked by opening it' : opens ? shortPage(site, asked) : hostOf(site))}
            </span>
          </span>
          {#if answer === 'checking'}
            <Spinner size={16} label="Asking {site.name}" />
          {:else if failed}
            <IconBtn icon="refresh" label="Ask {site.name} again" onclick={() => check(site, asked)} />
          {:else if answer === 'open'}
            <span class="dimmer" style="display:flex;pointer-events:none"><Icon name="open_in_new" size={18} /></span>
          {:else if answer}
            <span class:ok-text={answer === 'free'} class:dim={answer === 'taken'} class:dimmer={answer === 'unknown'} style="font-weight:500;pointer-events:none">{WORDS[answer as string]}</span>
          {/if}
        </div>
      {/each}
    </Group>
    <p class="dimmer small" style="margin-top:-10px;padding-bottom:18px">Click a site to open that name's page there.</p>
  {/if}

  {#if linked.length > 0}
    <Group title="Look for yourself" help="group.usernames.links">
      {#each linked as site (site.name)}
        <div class="set-row" class:act={!!asked}>
          {#if asked}<button type="button" class="hit" aria-label="Open {shortPage(site, asked)} in your browser" onclick={() => void host().openUrl(page(site, asked))}></button>{/if}
          <span style="display:flex;pointer-events:none"><Tile title={site.name} icon={site.icon} size={30} /></span>
          <span class="text">
            <span class="ell" style="display:block">{site.name}</span>
            <span class="sub ell" style="display:block">{asked ? shortPage(site, asked) : hostOf(site)}</span>
          </span>
          {#if asked}<span class="dimmer" style="display:flex;pointer-events:none"><Icon name="open_in_new" size={18} /></span>{/if}
        </div>
      {/each}
    </Group>
  {/if}
</NetGate>
