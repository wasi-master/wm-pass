<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Row from '../parts/Row.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import Switch from '../parts/Switch.svelte';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { ALIAS_SERVICES, saveServiceKey, serviceKey } from './aliasServices';
  import { copyPlainLine } from './clipboard';
  import NetGate from './NetGate.svelte';
  import { online } from './net.svelte';

  interface Alias {
    id: string;
    address: string;
    enabled: boolean;
    note: string;
  }

  let address = $state('');
  let tag = $state('');
  let trick = $state<string | null>(null);
  let trickProblem = $state<string | null>(null);
  let serviceId = $state(ALIAS_SERVICES[0].id);
  const service = $derived(ALIAS_SERVICES.find((s) => s.id === serviceId)!);
  const hasKey = $derived(pass.entries.some((e) => e.kind === 'api' && e.title === service.keyTitle));
  let keyText = $state('');
  let note = $state('');
  let aliases = $state.raw<Alias[]>([]);
  let problem = $state<string | null>(null);
  let busy = $state(false);

  async function runTrick(method: string) {
    try {
      trick = (await pass.engine<{ address: string }>(method, { address: address.trim(), tag: tag.trim() }, {})).json.address;
      trickProblem = null;
    } catch (failure: any) {
      trickProblem = failure?.message ?? 'That address could not be made.';
    }
  }

  async function withKey<T>(run: (key: Uint8Array) => Promise<T>): Promise<T | null> {
    const key = await serviceKey(service.keyTitle);
    if (!key) return null;
    try {
      return await run(key);
    } finally {
      key.fill(0);
    }
  }

  async function refresh() {
    problem = null;
    aliases = [];
    if (!hasKey || service.id === 'duckduckgo' || !prefs.v.network_allowed) return;
    try {
      aliases = (await withKey(async (secret) => (await pass.engine<Alias[]>('alias.list', { provider: service.id }, { secret, online: online() })).json)) ?? [];
    } catch (failure: any) {
      problem = failure?.message ?? 'The aliases could not be listed.';
    }
  }

  $effect(() => {
    void serviceId;
    void hasKey;
    void refresh();
  });

  let keeping = $state(false);
  async function keepKey() {
    if (keeping) return;
    keeping = true;
    problem = null;
    try {
      await saveServiceKey(service.keyTitle, keyText.trim());
      keyText = '';
    } catch (failure: any) {
      problem = failure?.message ?? 'The key could not be kept.';
    } finally {
      keeping = false;
    }
  }

  async function make() {
    busy = true;
    try {
      const made = await withKey(async (secret) => (await pass.engine<{ address: string }>('alias.create', { provider: service.id, note }, { secret, online: online() })).json);
      if (made) await copyPlainLine('Alias', made.address);
      note = '';
      await refresh();
    } catch (failure: any) {
      problem = failure?.message ?? 'The alias could not be made.';
    }
    busy = false;
  }

  async function setEnabled(alias: Alias, on: boolean) {
    try {
      await withKey((secret) => pass.engine('alias.set_enabled', { provider: service.id, id: alias.id, enabled: on }, { secret, online: online() }));
    } catch (failure: any) {
      problem = failure?.message ?? 'That could not be changed.';
    }
    await refresh();
  }
</script>

<Group title="With your own address" help="group.aliases.own">
  <div class="set-body stack">
    <p class="dim">A plus tag works with Gmail, Fastmail and many others. Dots work with Gmail. Neither hides your real address from a site that strips them.</p>
    <Field bind:value={address} placeholder="you@gmail.com" />
    <Field bind:value={tag} placeholder="Tag, such as the site’s name" />
    <div class="gap">
      <Btn label="Plus tag" onclick={() => runTrick('alias.plus')} />
      <Btn label="Gmail dots" onclick={() => runTrick('alias.dots')} />
    </div>
    {#if trickProblem}<p class="danger-text" role="alert">{trickProblem}</p>{/if}
    {#if trick}
      <div class="gap">
        <span class="mono selectable grow">{trick}</span>
        <IconBtn icon="content_copy" label="Copy" onclick={() => copyPlainLine('Address', trick!)} />
      </div>
    {/if}
  </div>
</Group>

<Group title="Forwarding services" help="group.aliases.services">
  <div class="set-body stack">
    <NetGate what="Alias services create addresses that forward to you, so a site never sees your real one. The service you pick receives your API key and the note you write.">
      <Segmented label="Service" value={serviceId} options={ALIAS_SERVICES.map((s) => [s.id, s.name] as [string, string])} onchange={(id) => (serviceId = id)} />
      {#if !hasKey}
        <p class="dim">{service.note}</p>
        <PasswordField bind:value={keyText} placeholder="API key" />
        <div><Btn label="Keep this key in the vault" variant="primary" disabled={!keyText.trim() || keeping} onclick={keepKey} /></div>
      {:else}
        <Field bind:value={note} placeholder="What is it for?" />
        <div><Btn label={busy ? 'Making it…' : 'Make a new alias'} icon="alternate_email" variant="primary" disabled={busy} onclick={make} /></div>
      {/if}
      {#if problem}<p class="danger-text" role="alert">{problem}</p>{/if}
    </NetGate>
  </div>
  {#each aliases as alias (alias.id)}
    <Row title={alias.address} sub={alias.note}>
      <IconBtn icon="content_copy" label="Copy" onclick={() => copyPlainLine('Alias', alias.address)} />
      <Switch checked={alias.enabled} label="Forwarding on" onchange={(on) => setEnabled(alias, on)} />
    </Row>
  {/each}
</Group>
