<script lang="ts">
  import { untrack } from 'svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Icon from '../parts/Icon.svelte';
  import Row from '../parts/Row.svelte';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import NetGate from './NetGate.svelte';
  import InboxAddress from './InboxAddress.svelte';
  import MailView from './MailView.svelte';
  import { online } from './net.svelte';
  import { INBOX_WARNING, PROVIDERS, isOwn, keptInboxes, mailTime, providerName, sentence, titleOf, type KeptInbox } from './inbox';

  // The list of kept addresses, one address at settings/inbox/<id>, and one mail below that.
  const parts = $derived(ui.section.split('/'));
  const openId = $derived(parts[2] ?? null);
  const openMail = $derived(parts[3] ? decodeURIComponent(parts[3]) : null);

  let kept = $state.raw<KeptInbox[] | null>(null);
  let problem = $state<string | null>(null);

  async function load() {
    try {
      kept = await keptInboxes();
      problem = null;
    } catch (failure: any) {
      kept = [];
      problem = sentence(failure.message);
    }
  }

  // The list is read again on the way back from an address, which may have been renamed or closed.
  $effect(() => {
    if (!openId) untrack(() => void load());
  });

  function go(id: string) {
    ui.section = `settings/inbox/${id}`;
  }

  // ---- a new address ----

  let making = $state(false);
  let provider = $state('mailtm');
  let label = $state('');
  let base = $state('');
  let domain = $state('');
  let busy = $state(false);
  let makeProblem = $state<string | null>(null);
  const own = $derived(isOwn(provider));
  const ready = $derived(!own || (base.trim() !== '' && (provider !== 'inbucket' || domain.trim() !== '')));

  function startMaking() {
    provider = 'mailtm';
    label = base = domain = '';
    makeProblem = null;
    making = true;
  }

  async function make() {
    if (!ready || busy) return;
    busy = true;
    makeProblem = null;
    try {
      const reply = await pass.net<{ id: string }>('inbox.open', online(), {
        provider,
        base: base.trim().replace(/\/+$/, ''),
        domain: domain.trim(),
        label: label.trim(),
      });
      await pass.flush();
      making = false;
      go(reply.json.id);
    } catch (failure: any) {
      makeProblem = sentence(failure.message);
    } finally {
      busy = false;
    }
  }
</script>

<NetGate what={INBOX_WARNING}>
  {#if openId && openMail}
    <MailView id={openId} message={openMail} />
  {:else if openId}
    <InboxAddress id={openId} />
  {:else}
    <div class="stack" style="padding-bottom:14px;max-width:480px">
      <p class="dim">Each address stays here, with its mail, until you close it. Good for one-off sign-ups, nothing more.</p>
      <div><Btn label="New address" icon="add" variant="primary" onclick={startMaking} /></div>
      {#if problem}<p class="danger-text">{problem}</p>{/if}
      {#if kept && kept.length === 0 && !problem}<p class="dimmer">No addresses yet.</p>{/if}
    </div>
    {#if kept && kept.length > 0}
      <Group title={kept.length === 1 ? '1 address' : `${kept.length} addresses`}>
        {#each kept as inbox (inbox.id)}
          <Row
            title={titleOf(inbox)}
            sub={[inbox.label ? inbox.address : null, providerName(inbox.provider), mailTime(inbox.created_ms) || null].filter(Boolean).join('  ·  ')}
            icon={isOwn(inbox.provider) ? 'dns' : 'inbox'}
            chevron
            onclick={() => go(inbox.id)}
          />
        {/each}
      </Group>
    {/if}
  {/if}
</NetGate>

{#if making}
  <Dialog title="New address" onclose={() => !busy && (making = false)}>
    <div class="stack">
      <Group>
        {#each PROVIDERS as choice (choice.id)}
          <Row
            title={choice.name}
            sub={choice.about}
            icon={choice.own ? 'dns' : 'inbox'}
            onclick={() => {
              provider = choice.id;
              makeProblem = null;
            }}
          >
            {#if provider === choice.id}<span class="ok-text" style="display:flex" aria-label="Chosen"><Icon name="check" size={18} /></span>{/if}
          </Row>
        {/each}
      </Group>
      {#if own}
        <Field bind:value={base} placeholder="Server, like https://mail.example.com" />
        {#if provider === 'inbucket'}<Field bind:value={domain} placeholder="The domain it takes mail for" />{/if}
        <p class="dimmer small">Plain http works only for this computer and Tailscale addresses.</p>
      {/if}
      <Field bind:value={label} placeholder="What it is for (optional)" onenter={make} />
      {#if makeProblem}<p class="danger-text small">{makeProblem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" disabled={busy} onclick={() => (making = false)} />
      <Btn label={busy ? 'Making the address…' : 'Make the address'} icon="inbox" variant="primary" disabled={!ready || busy} onclick={make} />
    {/snippet}
  </Dialog>
{/if}
