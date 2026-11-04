<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import NetGate from './NetGate.svelte';
  import { networkExists, online } from './net.svelte';

  let name = $state('');
  let sites = $state.raw<string[]>([]);
  // Sites that only answer a browser, where this shell can open a hidden one.
  let pages = $state.raw<string[]>([]);
  let answers = $state<Record<string, string>>({});
  let links = $state.raw<{ site: string; url: string }[]>([]);
  let running = $state(false);
  let problem = $state<string | null>(null);

  // Sites that answer a plain request are asked from here. TikTok and Instagram only answer a
  // browser that runs their scripts: the desktop and phone apps open a hidden one, and the
  // extension asks the desktop app to, over its link.
  const remote = host().usernames;
  const local = networkExists();
  const pagesRemote = !!remote && !host().checkUsernamePage;

  async function remoteSites() {
    try {
      const reply = await remote!.sites();
      if (!local) sites = reply.sites;
      pages = reply.pages;
      problem = null;
    } catch (failure: any) {
      if (!local) problem = failure.message;
    }
  }

  $effect(() => {
    if (local) {
      void pass.engine<string[]>('usernames.sites', {}, {}).then((reply) => (sites = reply.json));
      if (host().checkUsernamePage) void pass.engine<string[]>('usernames.pages', {}, {}).then((reply) => (pages = reply.json));
    }
    if (pagesRemote) void remoteSites();
  });

  async function ask(site: string, run: () => Promise<string>) {
    answers[site] = '…';
    try {
      answers[site] = await run();
    } catch (failure: any) {
      answers[site] = 'unknown';
      problem = failure.message;
    }
  }

  async function check() {
    const wanted = name.trim();
    running = true;
    problem = null;
    answers = {};
    links = [];
    // The desktop app may have connected since the page opened.
    if (pagesRemote && pages.length === 0) await remoteSites();
    for (const site of sites) {
      await ask(site, async () =>
        local ? (await pass.engine<{ answer: string }>('usernames.check', { site, name: wanted }, { online: online() })).json.answer : remote!.check(site, wanted),
      );
    }
    for (const site of pages) {
      await ask(site, () => (pagesRemote ? remote!.check(site, wanted) : host().checkUsernamePage!(site, wanted, online())));
    }
    try {
      links = local ? (await pass.engine<{ site: string; url: string }[]>('usernames.links', { name: wanted }, {})).json : await remote!.links(wanted);
    } catch {}
    running = false;
  }

  const WORDS: Record<string, string> = { free: 'Free', taken: 'Taken', unknown: 'No clear answer' };
  const shown = $derived([...sites, ...pages].filter((s) => s in answers));
</script>

<NetGate remote={!!remote} what={`Asks each site whether a name is taken, one at a time. Every site you check learns the name you looked up. TikTok and Instagram only answer a browser, so their pages load in a hidden, private window that runs their scripts and keeps nothing afterwards${pagesRemote ? '; here the desktop app opens them, while it is connected and allows network features too' : ''}. X, Threads, Reddit and a few others refuse or give the same answer for every name, so their profile pages are linked for you to open yourself.`}>
  <div class="stack" style="padding-bottom:14px;max-width:420px">
    <Field bind:value={name} placeholder="A username" onenter={() => name.trim() && !running && check()} />
    <div><Btn label={running ? 'Checking…' : 'Check'} icon="person_search" variant="primary" disabled={running || !name.trim()} onclick={check} /></div>
    {#if pagesRemote && pages.length === 0}<p class="dim">Connect the desktop app under Settings, Desktop app, to check TikTok and Instagram too.</p>{/if}
    {#if problem}<p class="danger-text">{problem}</p>{/if}
  </div>
  {#if shown.length > 0}
    <Group>
      {#each shown as site}
        <Row title={site}>
          <span class:ok-text={answers[site] === 'free'} class:dim={answers[site] === 'taken'} class:dimmer={answers[site] !== 'free' && answers[site] !== 'taken'} style="font-weight:500">{WORDS[answers[site]] ?? '…'}</span>
        </Row>
      {/each}
    </Group>
  {/if}
  {#if links.length > 0}
    <Group title="Look for yourself" help="group.usernames.links">
      {#each links as link}
        <Row title={link.site} sub="Opens the profile in your browser" icon="open_in_new" chevron onclick={() => void host().openUrl(link.url)} />
      {/each}
    </Group>
  {/if}
</NetGate>
