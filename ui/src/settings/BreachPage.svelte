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
  import { whenText } from '../engine/text';
  import { breachCheck, breachCounts, breachLast, breachSourceName, online, type BreachLast, type BreachSource } from './net.svelte';
  import { saveServiceKey, serviceKey } from './aliasServices';

  /** The key is an item in the vault, so it follows the vault to every device. */
  const HIBP_KEY = 'Have I Been Pwned API key';

  let source = $state<'leakcheck' | 'hibp' | 'comb'>('leakcheck');
  let passwordSource = $state<BreachSource>(breachCheck.running ? breachCheck.source : 'hibp');
  let byCount = $state(true);
  let apiKey = $state('');
  let busy = $state<string | null>(null);
  let results = $state<Record<string, string>>({});

  const running = $derived(breachCheck.running);
  const found = $derived(Object.keys(breachCounts.counts));
  const hibpKept = $derived(pass.entries.some((e) => e.kind === 'api' && e.title === HIBP_KEY));
  const checkable = $derived(pass.entries.filter((e) => e.has_password));
  const breached = $derived.by(() => {
    const list = pass.entries.filter((e) => e.id in breachCounts.counts);
    return byCount
      ? list.sort((a, b) => (breachCounts.counts[b.id] ?? 0) - (breachCounts.counts[a.id] ?? 0))
      : list.sort((a, b) => a.title.toLowerCase().localeCompare(b.title.toLowerCase()));
  });
  const emails = $derived([...new Set(pass.entries.map((e) => e.subtitle.trim().toLowerCase()).filter((s) => s.includes('@')))]);
  const last = $derived(breachLast());

  function lastText(at: BreachLast) {
    const time = new Date(at.atMs).toLocaleTimeString(undefined, { timeStyle: 'short' });
    return `Last checked ${whenText(at.atMs)} at ${time} with ${breachSourceName(at.source)}: ${at.found} of ${at.checked} found. What it found is kept only until the vault locks.`;
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
        const secret = source !== 'hibp' ? null : hibpKept ? await serviceKey(HIBP_KEY) : bytesOf(apiKey.trim());
        const rows = await pass.engine<{ name: string }[]>('breach.account', { source, email }, { secret, online: online() });
        secret?.fill(0);
        if (source === 'hibp' && !hibpKept && apiKey.trim()) {
          await saveServiceKey(HIBP_KEY, apiKey.trim());
          apiKey = '';
        }
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

<NetGate what="Checks your passwords against Have I Been Pwned or XposedOrNot. Only the start of each password’s hash is sent: five characters of SHA-1, or ten of Keccak-512. Neither service can tell which password you asked about. Checking an email address sends that address.">
  <Group title="Your passwords">
    <div class="set-body stack">
      <p class:dim={found.length === 0 || running} class:danger-text={found.length > 0 && !running}>
        {#if checkable.length === 0}No item in this vault holds a password yet, so there is nothing to check.
        {:else if running}Checked {breachCheck.done} of {breachCheck.total} with {breachSourceName(breachCheck.source)}. You can leave this page; the check carries on.
        {:else if found.length > 0}{found.length} of them have appeared in leaks. They now lead the list on the Health page.
        {:else}See whether any password in this vault has appeared in a known leak.{/if}
      </p>
      {#if last && !running}<p class="dimmer small">{lastText(last)}</p>{/if}
      {#if running}<div class="progress"><i style="width:{breachCheck.total === 0 ? 0 : (breachCheck.done * 100) / breachCheck.total}%"></i></div>{/if}
      {#if checkable.length > 0 && !running}
        <Segmented value={passwordSource} options={[['hibp', 'Have I Been Pwned'], ['xon', 'XposedOrNot']]} onchange={(v) => (passwordSource = v)} />
        <div><Btn label={last ? 'Check again' : 'Check every password'} icon="gpp_good" variant="primary" onclick={() => void breachCheck.start(checkable.map((e) => e.id), passwordSource)} /></div>
      {/if}
      {#if running}<div><Btn label="Stop" icon="close" onclick={() => breachCheck.stop()} /></div>{/if}
      {#if breachCheck.problem}<p class="danger-text">{breachCheck.problem}</p>{/if}
    </div>
  </Group>
  {#if breached.length > 0}
    <div class="stack" style="margin-bottom:18px;gap:8px">
      <div><Segmented value={byCount ? 'count' : 'name'} options={[['count', 'Most seen'], ['name', 'Name']]} onchange={(v) => (byCount = v === 'count')} /></div>
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
        {#if source === 'hibp'}
          {#if hibpKept}
            <p class="dim">Using the key kept in the vault as “{HIBP_KEY}”.</p>
          {:else}
            <Field bind:value={apiKey} placeholder="API key" mono />
            <p class="dim">After the first check the key is kept in the vault, so your other devices have it too.</p>
          {/if}
        {/if}
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
