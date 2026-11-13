<script lang="ts">
  import { untrack } from 'svelte';
  import Btn from '../parts/Btn.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Spinner from '../parts/Spinner.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import { copyPlainLine } from './clipboard';
  import MailFrame from './MailFrame.svelte';
  import { online } from './net.svelte';
  import { codesIn, forwardLink, mailAsText, mailDate, mailFile, mailFileName, sentence, type Mail } from './inbox';

  // One mail, read in full from the service.
  let { id, message }: { id: string; message: string } = $props();

  let mail = $state.raw<Mail | null>(null);
  let problem = $state<string | null>(null);
  let pictures = $state(false);
  let again = $state(0);

  async function read() {
    problem = null;
    try {
      const body = (await pass.net<Mail>('inbox.read', online(), { id, message })).json;
      mail = { ...body, html: body.html?.trim() ? body.html : null };
    } catch (failure: any) {
      problem = sentence(failure.message);
    }
  }

  $effect(() => {
    void id;
    void message;
    void again;
    untrack(() => void read());
  });

  const codes = $derived(mail ? codesIn(mail) : []);

  function back() {
    ui.section = `settings/inbox/${id}`;
  }

  function toNote() {
    if (!mail) return;
    ui.add('note', { presetTitle: mail.subject || `Mail from ${mail.from}`, presetNote: mailAsText(mail) });
  }

  async function forward() {
    if (!mail) return;
    try {
      await host().openUrl(forwardLink(mail));
    } catch {
      overlays.toast('No mail program opened. Copy the text instead.');
    }
  }

  async function more(event: MouseEvent) {
    if (!mail) return;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const choice = await overlays.popup(
      [
        { id: 'copy', label: 'Copy the text' },
        { id: 'file', label: 'Save as a file' },
      ],
      box.left,
      box.bottom,
    );
    if (choice === 'copy') await copyPlainLine('Mail', mail.text);
    if (choice === 'file') {
      try {
        if (await host().saveFile('Save the mail', mailFileName(mail), new TextEncoder().encode(mailFile(mail)))) overlays.toast('Saved.');
      } catch {
        overlays.toast('That file could not be written.');
      }
    }
  }
</script>

<div class="gap" style="padding-bottom:12px">
  <IconBtn icon="arrow_back" label="Back to the address" onclick={back} />
  <span class="grow ell" style="font-size:var(--fs-l);font-weight:600">{mail ? mail.subject || 'No subject' : 'Mail'}</span>
  {#if mail}
    <IconBtn icon="mail" label="Forward in your mail program" onclick={forward} />
    <IconBtn icon="sticky_note_2" label="Save to the vault as a note" onclick={toNote} />
    <IconBtn icon="more_vert" label="More" onclick={more} />
  {/if}
</div>

{#if problem}
  <div class="stack" style="align-items:flex-start;padding-bottom:12px">
    <p class="danger-text" role="alert">{problem}</p>
    <Btn label="Try again" icon="refresh" onclick={() => again++} />
  </div>
{/if}

{#if !mail}
  {#if !problem}<div style="padding:16px"><Spinner size={20} label="Opening the mail" /></div>{/if}
{:else}
  <div class="stack" style="gap:12px;padding-bottom:20px">
    <div>
      <div class="selectable" style="font-weight:600">{mail.from || 'Unknown sender'}</div>
      {#if mail.to}<div class="dim small selectable">To {mail.to}</div>{/if}
      {#if mail.at_ms > 0}<div class="dimmer small">{mailDate(mail)}</div>{/if}
    </div>
    {#if codes.length > 0}
      <div class="gap wrap">
        {#each codes as code (code)}
          <Btn label="Copy {code}" icon="content_copy" onclick={() => copyPlainLine('Code', code)} />
        {/each}
      </div>
    {/if}
    {#if mail.html}
      {#if !pictures}
        <div class="gap">
          <span class="grow dimmer small">Pictures are not loaded, so the sender cannot see that you opened this.</span>
          <Btn label="Load" icon="image" variant="quiet" onclick={() => (pictures = true)} />
        </div>
      {/if}
      <MailFrame html={mail.html} {pictures} />
    {:else}
      <p class="selectable" style="white-space:pre-wrap;overflow-wrap:anywhere">{mail.text || 'This mail has no text.'}</p>
    {/if}
  </div>
{/if}
