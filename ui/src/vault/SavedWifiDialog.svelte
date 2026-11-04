<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Row from '../parts/Row.svelte';
  import Spinner from '../parts/Spinner.svelte';
  import { host, type SavedNetwork } from '../engine/host';
  import { consumeText, pass } from '../engine/pass.svelte';

  // The networks this computer has saved. With `onpick` a click fills one item; without it several
  // are ticked and saved at once. A network already in the vault says so and starts unticked;
  // ticking it updates that item instead of adding a copy. Where the computer asks before handing
  // over each password (a Mac), nothing starts ticked, so one click cannot set off dozens of
  // prompts. The dialog stays open while they are saved, counting, and ends on what happened to
  // each network by name. Passwords are never shown, and each is read only when its network is
  // picked or saved.
  let {
    onclose,
    onpick,
    onfinished,
  }: {
    onclose: () => void;
    /** Fills one network into a form. The password is empty for an open network. */
    onpick?: (network: SavedNetwork, password: string) => void;
    /** After an add-all run, a line saying what it did. */
    onfinished?: (summary: string) => void;
  } = $props();

  const saved = host().savedWifi!;

  const SECURITY: Record<string, string> = { sae: 'WPA3', wpa: 'WPA', wep: 'WEP', eap: 'Enterprise', open: 'Open' };

  let networks = $state<SavedNetwork[] | null>(null);
  let problem = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let chosen = $state<string[]>([]);
  let saving = $state<{ at: number; of: number } | null>(null);
  let outcomes = $state<Record<string, { text: string; failed: boolean }> | null>(null);
  let summary = $state('');
  // The button's words when Add was clicked: while saving, each new item would otherwise count as
  // one already in the vault.
  let clickedLabel = $state('');

  // A network's item in the vault, by name, when it has one already.
  const inVault = $derived.by(() => {
    const found = new Map<string, string>();
    for (const entry of pass.entries) {
      if (entry.kind !== 'wifi' || entry.trashed) continue;
      for (const name of [entry.title, entry.subtitle]) if (name && !found.has(name)) found.set(name, entry.id);
    }
    return found;
  });

  const picked = $derived(networks?.filter((n) => chosen.includes(n.id)) ?? []);
  const updates = $derived(picked.filter((n) => inVault.has(n.ssid)).length);
  const adds = $derived(picked.length - updates);
  const addLabel = $derived(
    picked.length === 0
      ? 'Add networks'
      : updates === 0
        ? `Add ${adds} ${adds === 1 ? 'network' : 'networks'}`
        : adds === 0
          ? `Update ${updates} ${updates === 1 ? 'network' : 'networks'}`
          : `Add ${adds}, update ${updates}`,
  );

  function why(failure: unknown) {
    return (failure as { message?: string })?.message || String(failure) || 'no reason was given';
  }

  $effect(() => {
    saved.list().then(
      (found) => {
        networks = found;
        chosen = saved.asks ? [] : found.filter((n) => !inVault.has(n.ssid)).map((n) => n.id);
      },
      (failure) => (problem = why(failure)),
    );
  });

  const allTicked = $derived(!!networks?.length && chosen.length === networks.length);

  function tickAll() {
    chosen = allTicked ? [] : (networks ?? []).map((n) => n.id);
  }

  function toggle(network: SavedNetwork) {
    chosen = chosen.includes(network.id) ? chosen.filter((id) => id !== network.id) : [...chosen, network.id];
  }

  async function readPassword(network: SavedNetwork) {
    return network.security === 'open' ? '' : consumeText(await saved.password(network.id));
  }

  async function pick(network: SavedNetwork) {
    busy = network.ssid;
    problem = null;
    try {
      onpick!(network, await readPassword(network));
      onclose();
    } catch (failure) {
      problem = why(failure);
    } finally {
      busy = null;
    }
  }

  async function addChosen() {
    const list = picked;
    const done: Record<string, { text: string; failed: boolean }> = {};
    let added = 0;
    let updated = 0;
    let failed = 0;
    clickedLabel = addLabel;
    problem = null;
    for (const [at, network] of list.entries()) {
      saving = { at, of: list.length };
      const existing = inVault.get(network.ssid);
      try {
        const password = await readPassword(network);
        const id =
          existing ??
          (
            await pass.change<{ id: string }>('entry.create', {
              kind: 'wifi',
              title: network.ssid,
              fields: { ssid: network.ssid, security: network.security, hidden_ssid: network.hidden },
            })
          ).json.id;
        if (password) {
          const bytes = new TextEncoder().encode(password);
          try {
            await pass.change('entry.set_secret', { id, field: 'password' }, { secret: bytes });
          } finally {
            bytes.fill(0);
          }
        }
        if (existing) updated++;
        else added++;
        done[network.id] = { text: existing ? 'Updated its item' : 'Added', failed: false };
      } catch (failure) {
        failed++;
        done[network.id] = { text: `Not saved: ${why(failure)}`, failed: true };
      }
    }
    for (const network of networks ?? []) {
      if (!done[network.id] && inVault.has(network.ssid)) done[network.id] = { text: 'Already in your vault, left as it was', failed: false };
    }
    saving = null;
    outcomes = done;
    summary =
      [
        added === 0 ? null : added === 1 ? 'Added 1 network.' : `Added ${added} networks.`,
        updated === 0 ? null : updated === 1 ? 'Updated 1 that was already in your vault.' : `Updated ${updated} that were already in your vault.`,
        failed === 0 ? null : failed === 1 ? '1 could not be saved.' : `${failed} could not be saved.`,
      ]
        .filter(Boolean)
        .join(' ') || 'Nothing was added.';
  }

  function finish() {
    onfinished?.(summary);
    onclose();
  }
</script>

<Dialog title="Saved on this computer" onclose={saving ? undefined : onclose}>
  {#if networks === null && !problem}
    <p class="dim gap"><Spinner /> Reading saved networks…</p>
  {:else if networks?.length === 0}
    <p class="dim">This computer has no saved networks.</p>
  {:else if networks && outcomes}
    <p>{summary}</p>
    <div class="list">
      {#each networks.filter((n) => outcomes![n.id]) as network (network.id)}
        <Row title={network.ssid} sub={outcomes[network.id].text} icon={outcomes[network.id].failed ? 'error' : 'wifi'} danger={outcomes[network.id].failed} />
      {/each}
    </div>
  {:else if networks && saving}
    <p class="dim gap"><Spinner /> Saving {saving.at + 1} of {saving.of}…</p>
    {#if saved.asks}<p class="dimmer small">{saved.asks}</p>{/if}
  {:else if networks}
    {#if onpick}
      <p class="dim">Pick one to fill in its name, security and password.</p>
    {:else}
      <p class="dim">
        Ticked ones become Wi-Fi items.{#if networks.some((n) => inVault.has(n.ssid))}{' '}Ticking one that is already in your vault updates its item instead of adding a copy.{/if}
      </p>
    {/if}
    {#if saved.asks}<p class="dimmer small">{saved.asks}</p>{/if}
    {#if !onpick && networks.length > 1}
      <div class="row-end"><Btn label={allTicked ? 'Untick all' : 'Tick all'} variant="quiet" onclick={tickAll} /></div>
    {/if}
    <div class="list">
      {#each networks as network (network.id)}
        {@const sub = SECURITY[network.security] + (inVault.has(network.ssid) ? ', in your vault' : '')}
        {#if onpick}
          <Row title={network.ssid} {sub} icon="wifi" onclick={() => busy === null && void pick(network)}>
            {#if busy === network.ssid}<Spinner />{/if}
          </Row>
        {:else}
          <Row title={network.ssid} {sub} icon="wifi" onclick={() => toggle(network)}>
            <input type="checkbox" checked={chosen.includes(network.id)} onchange={() => toggle(network)} aria-label={network.ssid} />
          </Row>
        {/if}
      {/each}
    </div>
  {/if}
  {#if problem}<p class="danger-text">{problem}</p>{/if}
  {#snippet foot()}
    {#if outcomes}
      <Btn label="Done" variant="primary" onclick={finish} />
    {:else if !onpick && networks?.length}
      <Btn label="Cancel" onclick={onclose} disabled={!!saving} />
      <Btn label={saving ? clickedLabel : addLabel} variant="primary" disabled={picked.length === 0 || !!saving} onclick={addChosen} />
    {:else}
      <Btn label="Close" onclick={onclose} />
    {/if}
  {/snippet}
</Dialog>

<style>
  .row-end {
    display: flex;
    justify-content: flex-end;
  }
  .list {
    border: 1px solid var(--hair);
    border-radius: var(--r-m);
    overflow: auto;
    max-height: 50vh;
  }
</style>
