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

  // The extension has no network of its own, so the desktop app makes its checks.
  const remote = host().usernames;

  async function remoteSites() {
    try {
      const reply = await remote!.sites();
      sites = reply.sites;
      pages = reply.pages;
      problem = null;
    } catch (failure: any) {
      problem = failure.message;
    }
  }

  $effect(() => {
    if (remote) {
      void remoteSites();
      return;
    }
    if (!networkExists()) return;
    void pass.engine<string[]>('usernames.sites', {}, {}).then((reply) => (sites = reply.json));
    if (host().checkUsernamePage) void pass.engine<string[]>('usernames.pages', {}, {}).then((reply) => (pages = reply.json));
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
    if (remote && sites.length === 0) await remoteSites();
    for (const site of sites) {
      await ask(site, async () =>
        remote ? remote.check(site, wanted) : (await pass.engine<{ answer: string }>('usernames.check', { site, name: wanted }, { online: online() })).json.answer,
      );
    }
    for (const site of pages) {
      await ask(site, () => (remote ? remote.check(site, wanted) : host().checkUsernamePage!(site, wanted, online())));
    }
    try {
      links = remote ? await remote.links(wanted) : (await pass.engine<{ site: string; url: string }[]>('usernames.links', { name: wanted }, {})).json;
    } catch {}
    running = false;
  }

  const WORDS: Record<string, string> = { free: 'Free', taken: 'Taken', unknown: 'No clear answer' };
  const shown = $derived([...sites, ...pages].filter((s) => s in answers));
</script>

<NetGate remote={!!remote} what={`${remote ? 'The desktop app asks for this extension, and only while network features are allowed there too. ' : ''}Asks each site whether a name is taken, one at a time. Every site you check learns the name you looked up. TikTok and Instagram only answer a browser, so their pages load in a hidden, private window that runs their scripts and keeps nothing afterwards. X, Threads, Reddit and a few others refuse or give the same answer for every name, so their profile pages are linked for you to open yourself.`}>
  <div class="stack" style="padding-bottom:14px;max-width:420px">
    <Field bind:value={name} placeholder="A username" onenter={() => name.trim() && !running && check()} />
    <div><Btn label={running ? 'Checking…' : 'Check'} icon="person_search" variant="primary" disabled={running || !name.trim()} onclick={check} /></div>
    {#if remote && sites.length === 0 && problem}<p class="dim">Open the desktop app and connect it under Settings, Desktop app, to check names from here.</p>{/if}
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
    <Group title="Look for yourself">
      {#each links as link}
        <Row title={link.site} sub="Opens the profile in your browser" icon="open_in_new" chevron onclick={() => void host().openUrl(link.url)} />
      {/each}
    </Group>
  {/if}
</NetGate>
