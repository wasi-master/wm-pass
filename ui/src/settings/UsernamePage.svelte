<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { pass } from '../engine/pass.svelte';
  import NetGate from './NetGate.svelte';
  import { networkExists, online } from './net.svelte';

  let name = $state('');
  let sites = $state.raw<string[]>([]);
  let answers = $state<Record<string, string>>({});
  let running = $state(false);
  let problem = $state<string | null>(null);

  $effect(() => {
    if (!networkExists()) return;
    void pass.engine<string[]>('usernames.sites', {}, {}).then((reply) => (sites = reply.json));
  });

  async function check() {
    running = true;
    problem = null;
    answers = {};
    for (const site of sites) {
      answers[site] = '…';
      try {
        answers[site] = (await pass.engine<{ answer: string }>('usernames.check', { site, name: name.trim() }, { online: online() })).json.answer;
      } catch (failure: any) {
        answers[site] = 'unknown';
        problem = failure.message;
      }
    }
    running = false;
  }

  const WORDS: Record<string, string> = { free: 'Free', taken: 'Taken', unknown: 'No clear answer' };
</script>

<NetGate what="Asks each site whether a name is taken, one at a time. Every site you check learns the name you looked up. X, Instagram, Reddit, Twitch, npm and PyPI are left out, because they refuse or give the same answer for every name.">
  <div class="stack" style="padding-bottom:14px;max-width:420px">
    <Field bind:value={name} placeholder="A username" onenter={() => name.trim() && !running && check()} />
    <div><Btn label={running ? 'Checking…' : 'Check'} icon="person_search" variant="primary" disabled={running || !name.trim()} onclick={check} /></div>
    {#if problem}<p class="danger-text">{problem}</p>{/if}
  </div>
  {#if Object.keys(answers).length > 0}
    <Group>
      {#each sites.filter((s) => s in answers) as site}
        <Row title={site}>
          <span class:ok-text={answers[site] === 'free'} class:dim={answers[site] === 'taken'} class:dimmer={answers[site] !== 'free' && answers[site] !== 'taken'} style="font-weight:500">{WORDS[answers[site]] ?? '…'}</span>
        </Row>
      {/each}
    </Group>
  {/if}
</NetGate>
