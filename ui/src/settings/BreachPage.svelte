<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import EntryLines from './EntryLines.svelte';
  import NetGate from './NetGate.svelte';
  import { breachCounts, online } from './net.svelte';

  let running = $state(false);
  let done = $state(0);
  let total = $state(0);
  let problem = $state<string | null>(null);
  let source = $state<'leakcheck' | 'hibp' | 'comb'>('leakcheck');
  let apiKey = $state('');
  let busy = $state<string | null>(null);
  let results = $state<Record<string, string>>({});

  const found = $derived(Object.keys(breachCounts.counts));
  const breached = $derived(pass.entries.filter((e) => e.id in breachCounts.counts));
  const emails = $derived([...new Set(pass.entries.map((e) => e.subtitle.trim().toLowerCase()).filter((s) => s.includes('@')))]);

  async function checkAll() {
    running = true;
    problem = null;
    const targets = pass.entries.filter((e) => e.has_password);
    total = targets.length;
    done = 0;
    const counts: Record<string, number> = {};
    try {
      for (const entry of targets) {
        const reply = await pass.engine('entry.reveal', { id: entry.id, field: 'password' }, { handle: pass.handle });
        const secret = reply.secret;
        try {
          const check = await pass.engine<{ count: number }>('breach.password', {}, { secret, online: online() });
          if (check.json.count > 0) counts[entry.id] = check.json.count;
        } finally {
          secret?.fill(0);
        }
        done++;
      }
      breachCounts.counts = counts;
    } catch (failure: any) {
      problem = failure.message;
    }
    running = false;
  }

  async function checkEmail(email: string) {
    busy = email;
    try {
      if (source === 'comb') {
        const reply = await pass.net<{ matched: unknown[]; lines: number }>('breach.comb', online(), { email });
        const matched = reply.json.matched.length;
        results[email] =
          matched > 0 ? `${matched} of your passwords for it are in this leak. Change them.` : reply.json.lines > 0 ? 'The address is listed, with passwords you no longer use.' : 'Not listed.';
      } else {
        const secret = source === 'hibp' ? bytesOf(apiKey) : null;
        const rows = await pass.engine<{ name: string }[]>('breach.account', { source, email }, { secret, online: online() });
        secret?.fill(0);
        results[email] = rows.json.length === 0 ? 'Not listed.' : `In ${rows.json.length}: ${rows.json.slice(0, 5).map((r) => r.name).join(', ')}`;
      }
    } catch (failure: any) {
      results[email] = failure.message;
    }
    busy = null;
  }

  const WHAT_EMAIL = $derived(
    source === 'hibp'
      ? 'Have I Been Pwned needs an API key of your own.'
      : source === 'comb'
        ? 'Asks Proxynova for your address, then compares your own passwords here on this device. Only yes or no comes out: leaked text is never shown or kept.'
        : 'LeakCheck’s free check names the services an address appeared in.',
  );
</script>

<NetGate what="Checks your passwords against Have I Been Pwned. Only the first five characters of each password’s SHA-1 hash are sent, so the service cannot tell which password you asked about. Checking an email address sends that address.">
  <Group title="Your passwords">
    <div class="set-body stack">
      <p class:dim={found.length === 0 || running} class:danger-text={found.length > 0 && !running}>
        {#if running}Checked {done} of {total}.
        {:else if found.length === 0}See whether any password in this vault has appeared in a known leak.
        {:else}{found.length} of them have appeared in leaks. They now lead the list on the Health page.{/if}
      </p>
      {#if running}<div class="progress"><i style="width:{total === 0 ? 0 : (done * 100) / total}%"></i></div>{/if}
      <div><Btn label={running ? 'Checking…' : 'Check every password'} icon="gpp_good" variant="primary" disabled={running} onclick={checkAll} /></div>
      {#if problem}<p class="danger-text">{problem}</p>{/if}
    </div>
  </Group>
  {#if breached.length > 0}
    <div style="margin-bottom:18px">
      <EntryLines items={breached} onpick={(e) => { ui.go('all'); ui.open(e.id); }}>
        {#snippet note(e)}<span class="small danger-text" style="display:block">Seen {breachCounts.counts[e.id].toLocaleString()} times</span>{/snippet}
      </EntryLines>
    </div>
  {/if}
  {#if emails.length > 0}
    <Group title="Your email addresses" help="group.breaches.emails">
      <div class="set-body stack">
        <p class="dim">{WHAT_EMAIL}</p>
        <Segmented value={source} options={[['leakcheck', 'LeakCheck'], ['hibp', 'Have I Been Pwned'], ['comb', 'Passwords in COMB']]} onchange={(v) => (source = v)} />
        {#if source === 'hibp'}<Field bind:value={apiKey} placeholder="API key" mono />{/if}
      </div>
      {#each emails as email}
        <div class="set-row">
          <span class="text">
            <span class="ell" style="display:block">{email}</span>
            {#if results[email]}<span class="sub" style="display:block">{results[email]}</span>{/if}
          </span>
          <IconBtn icon="search" label="Check {email}" disabled={busy === email} onclick={() => checkEmail(email)} />
        </div>
      {/each}
    </Group>
  {/if}
</NetGate>
