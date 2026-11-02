<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import { pass } from '../engine/pass.svelte';
  import { copyPlainLine } from './clipboard';
  import NetGate from './NetGate.svelte';
  import { online } from './net.svelte';

  interface Message {
    id: string;
    from: string;
    subject: string;
    preview: string;
  }

  let service = $state<'mailtm' | 'guerrilla'>('mailtm');
  let address = $state<string | null>(null);
  let token = $state<Uint8Array | null>(null);
  let messages = $state.raw<Message[]>([]);
  let opened = $state<{ subject: string; body: string } | null>(null);
  let problem = $state<string | null>(null);

  $effect(() => {
    const current = address;
    if (!current) return;
    const check = async () => {
      try {
        messages = (await pass.engine<Message[]>('inbox.messages', { service, address: current }, { secret: token, online: online() })).json;
      } catch {
        // The next round tries again.
      }
    };
    void check();
    const timer = setInterval(check, 8000);
    return () => clearInterval(timer);
  });

  async function make() {
    try {
      const reply = await pass.engine<{ address: string }>('inbox.new', { service }, { online: online() });
      token = reply.secret;
      address = reply.json.address;
      problem = null;
    } catch (failure: any) {
      problem = failure.message;
    }
  }

  async function open(message: Message) {
    let body = message.preview;
    if (service === 'mailtm') {
      try {
        body = (await pass.engine<{ text: string }>('inbox.body', { id: message.id }, { secret: token, online: online() })).json.text;
      } catch {
        // The preview will do.
      }
    }
    opened = { subject: message.subject, body };
  }
</script>

<NetGate what="Makes a throwaway address on a public mail service, for a sign-up that only needs one confirmation mail. The service, and possibly others, can read what arrives. Never use one for an account you want to keep.">
  <div class="stack">
    {#if !address}
      <p class="dim">Anyone could read these mailboxes in principle, and the address is gone when you leave this page. Good for a one-off sign-up, nothing more.</p>
      <div><Segmented value={service} options={[['mailtm', 'mail.tm'], ['guerrilla', 'Guerrilla Mail']]} onchange={(s) => (service = s)} /></div>
      <div><Btn label="Make an address" icon="inbox" variant="primary" onclick={make} /></div>
    {:else}
      <div class="gap">
        <span class="mono selectable grow">{address}</span>
        <IconBtn icon="content_copy" label="Copy" onclick={() => copyPlainLine('Address', address!)} />
      </div>
      <Group title="Arrived">
        {#if messages.length === 0}
          <div class="set-body"><p class="dim">Nothing yet. This page checks every few seconds.</p></div>
        {/if}
        {#each messages as message (message.id)}
          <button type="button" class="set-row" onclick={() => open(message)}>
            <span class="text">
              <span class="ell" style="display:block;font-weight:500">{message.subject || 'No subject'}</span>
              <span class="sub ell" style="display:block">{message.from}</span>
              <span class="sub" style="display:block">{message.preview}</span>
            </span>
          </button>
        {/each}
      </Group>
      {#if opened}
        <p style="font-weight:500">{opened.subject}</p>
        <!-- Plain text only, so no tracking pixel or script in a mail ever runs here. -->
        <p class="selectable" style="white-space:pre-wrap">{opened.body}</p>
      {/if}
    {/if}
    {#if problem}<p class="danger-text">{problem}</p>{/if}
  </div>
</NetGate>
