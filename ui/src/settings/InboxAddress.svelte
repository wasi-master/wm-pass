<script lang="ts">
  import { untrack } from 'svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Spinner from '../parts/Spinner.svelte';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import { copyPlainLine } from './clipboard';
  import { online } from './net.svelte';
  import { keptInboxes, mailTime, providerName, sentence, titleOf, type KeptInbox, type MailRow } from './inbox';

  // One kept address: its mail, looked for every 8 seconds while the page is open.
  let { id }: { id: string } = $props();

  let inbox = $state.raw<KeptInbox | null>(null);
  let missing = $state<string | null>(null);
  let mail = $state.raw<MailRow[] | null>(null);
  // What was there when the page opened, so what comes in afterwards can be marked new.
  let seen = $state.raw<Set<string> | null>(null);
  let looking = $state(false);
  let problem = $state<string | null>(null);
  let again = $state(0);

  async function find() {
    try {
      inbox = (await keptInboxes()).find((k) => k.id === id) ?? null;
      missing = inbox ? null : 'This address was closed.';
    } catch (failure: any) {
      missing = sentence(failure.message);
    }
  }

  $effect(() => {
    void id;
    untrack(() => void find());
  });

  async function look() {
    if (looking) return;
    looking = true;
    try {
      const rows = (await pass.net<MailRow[]>('inbox.list', online(), { id })).json;
      if (!seen) seen = new Set(rows.map((r) => r.id));
      mail = rows;
      problem = null;
    } catch (failure: any) {
      problem = sentence(failure.message);
    } finally {
      looking = false;
    }
  }

  $effect(() => {
    if (!inbox) return;
    void again;
    untrack(() => void look());
    const timer = setInterval(() => {
      // A window nobody can see has no one to show new mail to.
      if (document.visibilityState === 'visible') void look();
    }, 8000);
    return () => clearInterval(timer);
  });

  function back() {
    ui.section = 'settings/inbox';
  }

  function openMail(row: MailRow) {
    ui.section = `settings/inbox/${id}/${encodeURIComponent(row.id)}`;
  }

  // ---- naming and closing ----

  let renaming = $state(false);
  let closing = $state(false);
  let label = $state('');
  let busy = $state(false);
  let sheetProblem = $state<string | null>(null);

  function startRenaming() {
    label = inbox?.label ?? '';
    sheetProblem = null;
    renaming = true;
  }

  async function rename() {
    // Enter pressed again while the name is on its way would send it twice.
    if (busy) return;
    busy = true;
    sheetProblem = null;
    try {
      await pass.net('inbox.rename', online(), { id, label: label.trim() });
      await pass.flush();
      renaming = false;
      await find();
    } catch (failure: any) {
      sheetProblem = sentence(failure.message);
    } finally {
      busy = false;
    }
  }

  async function close(remove: boolean) {
    if (busy) return;
    busy = true;
    sheetProblem = null;
    try {
      await pass.net('inbox.close', online(), { id, delete: remove });
      await pass.flush();
      closing = false;
      back();
    } catch (failure: any) {
      sheetProblem = sentence(failure.message);
    } finally {
      busy = false;
    }
  }

  const deletes = $derived(inbox?.provider !== 'guerrilla');
</script>

<div class="gap" style="padding-bottom:12px">
  <IconBtn icon="arrow_back" label="All addresses" onclick={back} />
  <span class="grow ell" style="font-size:var(--fs-l);font-weight:600">{inbox ? titleOf(inbox) : 'Inbox'}</span>
  {#if inbox}
    <IconBtn icon="edit" label="Name this address" onclick={startRenaming} />
    <IconBtn
      icon="delete"
      label="Close this address"
      onclick={() => {
        sheetProblem = null;
        closing = true;
      }}
    />
  {/if}
</div>

{#if missing}
  <p class="dim">{missing}</p>
{:else if inbox}
  <Group>
    <div class="set-row">
      <span class="text">
        <span class="mono selectable" style="display:block">{inbox.address}</span>
        <span class="sub" style="display:block">{providerName(inbox.provider)}</span>
      </span>
      <IconBtn icon="content_copy" label="Copy the address" onclick={() => copyPlainLine('Address', inbox!.address)} />
    </div>
  </Group>

  <Group title="Arrived" help="group.inbox.arrived">
    {#snippet trail()}
      <span style="display:flex;align-items:center;justify-content:center;width:28px;height:20px">
        {#if looking}<Spinner size={14} label="Looking for new mail" />{:else}<IconBtn icon="refresh" size={16} label="Look for new mail" onclick={() => again++} />{/if}
      </span>
    {/snippet}
    {#if problem}
      <div class="set-body"><p class="danger-text small" role="alert">{problem}</p></div>
    {/if}
    {#if mail && mail.length === 0}
      <div class="set-body"><p class="dim">Nothing yet. This page looks for new mail every few seconds while it is open.</p></div>
    {:else if !mail && !problem}
      <div class="set-body"><p class="dimmer">Looking…</p></div>
    {/if}
    {#each mail ?? [] as row (row.id)}
      {@const fresh = seen ? !seen.has(row.id) : false}
      <div class="set-row act" style="align-items:flex-start;padding-top:9px;padding-bottom:9px">
        <button type="button" class="hit" aria-label="{fresh ? 'New. ' : ''}{row.from || 'Unknown sender'}. {row.subject || 'No subject'}" onclick={() => openMail(row)}></button>
        <span class="text">
          <span class="gap" style="gap:8px">
            {#if fresh}<span class="fresh" aria-hidden="true"></span>{/if}
            <span class="grow ell" style="font-weight:600">{row.from || 'Unknown sender'}</span>
            <span class="dimmer small" style="flex:none">{mailTime(row.at_ms)}</span>
          </span>
          <span class="ell" style="display:block">{row.subject || 'No subject'}</span>
          {#if row.preview.trim()}<span class="sub clamp" style="display:block">{row.preview}</span>{/if}
        </span>
      </div>
    {/each}
  </Group>
{/if}

{#if renaming}
  <Dialog title="Name this address" onclose={() => !busy && (renaming = false)}>
    <div class="stack">
      <Field bind:value={label} placeholder="What it is for" autofocus onenter={rename} />
      {#if sheetProblem}<p class="danger-text small" role="alert">{sheetProblem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" disabled={busy} onclick={() => (renaming = false)} />
      <Btn label="Save" variant="primary" disabled={busy} onclick={rename} />
    {/snippet}
  </Dialog>
{/if}

{#if closing && inbox}
  <Dialog title="Close this address?" onclose={() => !busy && (closing = false)}>
    <div class="stack">
      <p class="dim">
        {deletes
          ? `WM Pass forgets ${inbox.address} and its mail. Anything saved to the vault stays.`
          : `WM Pass forgets ${inbox.address}. Guerrilla Mail clears it on its own within the hour. Anything saved to the vault stays.`}
      </p>
      {#if sheetProblem}<p class="danger-text small" role="alert">{sheetProblem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" disabled={busy} onclick={() => (closing = false)} />
      {#if deletes}
        <Btn label="Only forget it here" disabled={busy} onclick={() => close(false)} />
        <Btn label="Close and delete it on {providerName(inbox!.provider)}" icon="delete" variant="danger-fill" disabled={busy} onclick={() => close(true)} />
      {:else}
        <Btn label="Close" icon="delete" variant="danger-fill" disabled={busy} onclick={() => close(false)} />
      {/if}
    {/snippet}
  </Dialog>
{/if}

<style>
  .fresh {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--primary);
  }
  .clamp {
    display: -webkit-box !important;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
</style>
