<script lang="ts">
  import HelpButton from '../help/HelpButton.svelte';
  import Btn from '../parts/Btn.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Ring from '../parts/Ring.svelte';
  import SecretText from '../parts/SecretText.svelte';
  import Strength from '../parts/Strength.svelte';
  import Tile from '../parts/Tile.svelte';
  import { bytesOf, consumeText, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { host } from '../engine/host';
  import { kindName } from '../engine/face';
  import { grouped, siteLine, sizeText, whenText } from '../engine/text';
  import type { AttachmentView, Detail, OtpRow, StrengthView } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { ui } from './ui.svelte';
  import { entryMenu, runEntryMenu } from './entryMenu';
  import Pickers, { type PickerKind } from './Pickers.svelte';
  import SendItem from './SendItem.svelte';

  let { id }: { id: string } = $props();

  let detail = $state<Detail | null>(null);
  let revealed = $state<Record<string, string>>({});
  let row = $state<OtpRow | null>(null);
  let hotp = $state<string | null>(null);
  let holds = $state<StrengthView | null>(null);
  let historyOpen = $state(false);
  let picker = $state<PickerKind | null>(null);
  let guess = $state<string | null>(null);
  let sending = $state(false);
  let image = $state<{ name: string; url: string } | null>(null);

  $effect(() => {
    void pass.revision;
    const current = id;
    pass
      .detail(current)
      .then((d) => {
        if (current === id) detail = d;
      })
      .catch(() => {
        if (current === id) {
          detail = null;
          ui.selected = null;
        }
      });
  });

  // Leaving an item takes its secrets off the screen.
  $effect(() => {
    void id;
    revealed = {};
    hotp = null;
    holds = null;
    historyOpen = false;
    guess = null;
  });

  // Nothing stays on show by accident: after a while, all of it goes back.
  $effect(() => {
    if (Object.keys(revealed).length === 0) return;
    const seconds = prefs.v.reveal_seconds;
    if (seconds <= 0) return;
    const timer = setTimeout(() => (revealed = {}), seconds * 1000);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!detail?.otp) {
      row = null;
      return;
    }
    let stop = false;
    const tick = async () => {
      if (stop) return;
      try {
        row = (await pass.codes([id]))[0] ?? null;
      } catch {
        row = null;
      }
      setTimeout(tick, 1000 - (Date.now() % 1000) + 10);
    };
    void tick();
    return () => {
      stop = true;
    };
  });

  $effect(() => {
    if (detail && detail.tags.length === 0 && !detail.trashed) {
      const current = detail.id;
      pass.view<{ category: string | null }>('categorise.suggest', { id: current }).then((r) => {
        if (current === id) guess = r.category;
      });
    }
  });

  async function toggle(key: string, method: string, args: unknown) {
    if (revealed[key] !== undefined) {
      const { [key]: _, ...rest } = revealed;
      revealed = rest;
      if (key === 'field:password') holds = null;
      return;
    }
    const text = await pass.reveal(method, args);
    revealed = { ...revealed, [key]: text };
    if (key === 'field:password') {
      const bytes = bytesOf(text);
      holds = await pass.strength(bytes);
      bytes.fill(0);
    }
  }

  const username = $derived(detail?.fields.find((f) => f.key === 'username' || f.key === 'user')?.value as string | undefined);
  const url = $derived.by(() => {
    for (const site of detail?.sites ?? []) {
      const line = siteLine(site);
      if (line?.openable) return line.openable;
    }
    return null;
  });

  function openUrl(target: string) {
    void host().openUrl(target.includes('://') ? target : `https://${target}`);
  }

  async function moreMenu(event: MouseEvent) {
    if (!detail) return;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const choice = await overlays.popup(
      [
        { id: 'attach', label: 'Attach a file…' },
        { id: 'link', label: 'Link another item…' },
        { id: 'reminder', label: 'Change reminder…' },
        { id: 'destruct', label: 'Self-destruct…' },
        { id: 'folder', label: 'Move to folder…' },
        { id: 'tags', label: 'Tags…' },
        { id: 'colour', label: 'Color…' },
        { id: 'icon', label: 'Icon…' },
        { id: 'send', label: 'Send to a device…' },
        { separator: true },
        ...entryMenu(detail).filter((i) => !['password', 'username', 'code', 'open_site', 'edit'].includes(i.id ?? 'x')),
      ].filter((item, index, list) => !(item.separator && list[index - 1]?.separator)),
      box.right - 180,
      box.bottom + 2,
    );
    if (!choice) return;
    if (choice === 'attach') await attach();
    else if (choice === 'send') sending = true;
    else if (['link', 'reminder', 'destruct', 'folder', 'tags', 'colour', 'icon'].includes(choice)) picker = choice as PickerKind;
    else await runEntryMenu(choice, detail);
  }

  async function attach() {
    const file = await host().pickFile('Attach a file');
    if (!file || !detail) return;
    await pass.addAttachment(detail.id, file.name, '', file.bytes);
    file.bytes.fill(0);
  }

  async function attachmentMenu(event: MouseEvent, file: AttachmentView) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const isImage = file.mime.startsWith('image/') || /\.(png|jpe?g|gif|webp)$/i.test(file.name);
    const choice = await overlays.popup(
      [
        ...(isImage ? [{ id: 'view', label: 'View' }] : []),
        { id: 'save', label: 'Save a copy…' },
        { separator: true },
        { id: 'remove', label: 'Remove…', danger: true },
      ],
      box.left,
      box.bottom + 2,
    );
    if (choice === 'view') {
      const plain = await pass.openAttachment(id, file.blob);
      const blob = new Blob([plain.slice()], { type: file.mime || 'image/png' });
      plain.fill(0);
      image = { name: file.name, url: URL.createObjectURL(blob) };
    } else if (choice === 'save') {
      const yes = await overlays.ask({ title: 'Save a copy?', body: 'The copy will not be encrypted.', action: 'Save' });
      if (!yes) return;
      const plain = await pass.openAttachment(id, file.blob);
      await host().saveFile('Save a copy', file.name, plain);
      plain.fill(0);
    } else if (choice === 'remove') {
      const yes = await overlays.ask({ title: `Remove “${file.name}”?`, body: 'The file is deleted from this vault.', action: 'Remove', danger: true });
      if (yes) await pass.change('attachment.remove', { id, blob: file.blob });
    }
  }

  function closeImage() {
    if (image) URL.revokeObjectURL(image.url);
    image = null;
  }

  async function nextHotp() {
    const reply = await pass.change('otp.hotp_next', { id });
    hotp = consumeText(reply.secret);
  }

  function fieldText(value: unknown, input: string) {
    if (input === 'date' && typeof value === 'number') return new Date(value).toLocaleDateString(undefined, { dateStyle: 'medium' });
    return String(value ?? '');
  }

  const others = $derived(detail ? detail.links.flatMap((link) => pass.entries.filter((e) => e.id === link)) : []);
  const folderName = $derived(pass.folders.find((f) => f.id === detail?.folder)?.name);
  const reminderText = $derived.by(() => {
    const r = detail?.reminders;
    if (r?.change_every_days) return `Every ${r.change_every_days} days`;
    if (r?.expires_ms) return `Expires ${whenText(r.expires_ms)}`;
    return null;
  });
  const copyOnClick = $derived(prefs.v.tap_copies);
  const currentCode = $derived(row?.code ?? hotp);
</script>

{#snippet textField(label: string, value: string, copyAs: string = label, multiline = false)}
  <div class="field">
    <span class="label">{label}</span>
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <span class="value selectable" class:click={copyOnClick} style={multiline ? 'white-space:pre-wrap' : ''} title={copyOnClick ? 'Click to copy' : undefined} onclick={() => copyOnClick && !getSelection()?.toString() && ui.copyPlain(copyAs, value)}>{value}</span>
    <span class="tools"><IconBtn icon="content_copy" size={16} label="Copy {label}" onclick={() => ui.copyPlain(copyAs, value)} /></span>
  </div>
{/snippet}

{#snippet secretField(label: string, key: string, method: string, args: unknown, multiline = false)}
  {@const shown = revealed[key]}
  <div class="field">
    <span class="label">{label}</span>
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <span class="value" class:click={copyOnClick && shown === undefined} style={multiline ? 'white-space:pre-wrap' : ''} title={copyOnClick && shown === undefined ? 'Click to copy' : undefined} onclick={() => copyOnClick && shown === undefined && ui.copySecret(label, method, args)}>
      {#if shown !== undefined}
        {#if multiline}<span class="selectable">{shown}</span>{:else}<SecretText value={shown} />{/if}
      {:else}
        <span class="secret-dots mono">••••••••••••</span>
      {/if}
    </span>
    <span class="tools" class:keep={shown !== undefined}>
      <IconBtn icon={shown !== undefined ? 'visibility_off' : 'visibility'} size={16} label="{shown !== undefined ? 'Hide' : 'Show'} {label}" onclick={() => toggle(key, method, args)} />
      <IconBtn icon="content_copy" size={16} label="Copy {label}" onclick={() => ui.copySecret(label, method, args)} />
    </span>
  </div>
{/snippet}

{#if detail}
  <div class="detail-bar" data-tauri-drag-region>
    {#if ui.narrow}<IconBtn icon="arrow_back" label="Back" onclick={() => { ui.selected = null; ui.showDetail = false; }} />{/if}
    <span class="grow" data-tauri-drag-region></span>
    <HelpButton id="screen.item" title="the item page" />
    {#if !detail.trashed}
      <IconBtn
        icon={detail.favourite ? 'star_fill' : 'star'}
        label={detail.favourite ? 'Remove from favorites' : 'Add to favorites'}
        class={detail.favourite ? 'star-on' : ''}
        onclick={() => pass.change('entry.common', { id, favourite: !detail!.favourite })}
      />
      <IconBtn icon="edit" label="Edit" onclick={() => ui.edit(id, detail!.kind)} />
      <span data-help-anchor="detail.more" style="display:flex"><IconBtn icon="more_horiz" label="More" onclick={moreMenu} /></span>
    {/if}
  </div>
  <div class="detail-body scroll">
    <div class="detail-inner">
      <div class="detail-head">
        <Tile title={detail.title} site={detail.site} kind={detail.kind} label={detail.color} icon={detail.icon} size={48} />
        <div class="grow">
          <div class="name selectable">{detail.title || 'Untitled'}</div>
          <div class="dim ell">
            {kindName(detail.kind)}{folderName ? `, in ${folderName}` : ''}
          </div>
        </div>
      </div>

      {#if detail.trashed}
        <div class="actions">
          <Btn label="Restore" variant="primary" icon="restore_from_trash" onclick={() => runEntryMenu('restore', detail!)} />
          <Btn label="Delete for good" variant="danger" icon="delete" onclick={() => runEntryMenu('purge', detail!)} />
        </div>
      {:else if prefs.v.entry_actions && (detail.has_password || username || url)}
        <div class="actions" data-help-anchor="detail.actions">
          {#if detail.has_password}<Btn label="Copy password" variant="primary" icon="content_copy" onclick={() => ui.copyPassword(id)} />{/if}
          {#if username && prefs.v.entry_action_username}<Btn label="Copy username" icon="person" variant={detail.has_password ? '' : 'primary'} onclick={() => ui.copyPlain('Username', username!)} />{/if}
          {#if url && prefs.v.entry_action_open}<Btn label="Open site" icon="open_in_new" onclick={() => openUrl(url!)} />{/if}
        </div>
      {/if}

      {#if detail.destroy_at_ms}
        <div class="banner">
          <Icon name="timer" size={18} />
          <span class="grow">Deletes itself on {new Date(detail.destroy_at_ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</span>
          <Btn label="Keep it" variant="quiet" onclick={() => pass.change('entry.common', { id, destroy_at_ms: null })} />
        </div>
      {/if}

      <div class="fields" data-help-anchor="detail.fields">
        {#each detail.fields.filter((f) => f.has_value || f.value === true) as field (field.key)}
          {#if field.input === 'toggle'}
            <div class="field"><span class="label">{field.label}</span><span class="value">{field.value ? 'On' : 'Off'}</span><span></span></div>
          {:else if field.secret}
            {@render secretField(field.label, `field:${field.key}`, 'entry.reveal', { id, field: field.key }, field.input === 'multiline')}
            {#if field.key === 'password' && holds && revealed['field:password'] !== undefined}
              <div class="field"><span class="label">Strength</span><span class="value"><Strength score={holds.score} /><span class="dim small">Time to crack: {holds.crack_slow_hash}</span></span><span></span></div>
            {/if}
          {:else}
            {@render textField(field.label, fieldText(field.value, field.input), field.label, field.input === 'multiline')}
          {/if}
        {/each}

        {#if detail.otp}
          <div class="field">
            <span class="label">One-time code</span>
            <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
            <span class="value" class:click={!!currentCode} onclick={() => currentCode && ui.copyCode(id)}>
              {#if currentCode}
                <span class="code-value small">{prefs.v.code_groups ? grouped(currentCode) : currentCode}</span>
                {#if row?.next_code && prefs.v.code_next}<span class="dim small mono" style="display:block">Next {grouped(row.next_code)}</span>{/if}
              {:else}
                <span class="dim">Counter-based</span>
              {/if}
            </span>
            <span class="tools keep gap">
              {#if detail.otp.period && row?.seconds_left != null}
                {#if prefs.v.code_ring}<Ring period={detail.otp.period} left={row.seconds_left} />{/if}
              {:else}
                <Btn label="New code" variant="quiet" onclick={nextHotp} />
              {/if}
              {#if currentCode}<IconBtn icon="content_copy" size={16} label="Copy code" onclick={() => ui.copyCode(id)} />{/if}
            </span>
          </div>
        {/if}

        {#each detail.sites as site}
          {@const line = siteLine(site)}
          {#if line}
            <div class="field">
              <span class="label">Website</span>
              <span class="value">
                {#if line.openable}<button type="button" class="link ell" style="max-width:100%;text-align:left" onclick={() => openUrl(line.openable!)}>{line.label}</button>
                {:else}<span class="selectable">{line.label}</span>{/if}
              </span>
              <span class="tools">
                <IconBtn icon="content_copy" size={16} label="Copy website" onclick={() => ui.copyPlain('Website', line.label)} />
              </span>
            </div>
          {/if}
        {/each}

        {#each detail.passkeys as key}
          <div class="field">
            <span class="label">Passkey</span>
            <span class="value">{key.rp_id}{key.user_name ? `, ${key.user_name}` : ''}<span class="dim small" style="display:block">Made {whenText(key.created_ms)}</span></span>
            <span></span>
          </div>
        {/each}

        {#each detail.custom_fields as field (field.index)}
          {#if field.value !== null}
            {@render textField(field.name, field.value)}
          {:else}
            {@render secretField(field.name, `custom:${field.index}`, 'entry.reveal_custom', { id, index: field.index })}
          {/if}
        {/each}

        {#each detail.questions as question, index}
          {@render secretField(question, `question:${index}`, 'entry.reveal_question', { id, index })}
        {/each}

        {#if detail.has_notes}
          {@render secretField('Notes', 'notes', 'entry.reveal', { id, field: 'notes' }, true)}
        {/if}

        {#if detail.alias}
          {@render textField('Email alias', detail.alias)}
        {/if}

        {#each detail.attachment_list as file (file.blob)}
          <div class="field">
            <span class="label">Attachment</span>
            <span class="value"><button type="button" class="link ell" style="max-width:100%;text-align:left" onclick={(e) => attachmentMenu(e, file)}>{file.name}</button><span class="dim small" style="display:block">{sizeText(file.size)}</span></span>
            <span></span>
          </div>
        {/each}

        {#each others as other (other.id)}
          <div class="field">
            <span class="label">Linked</span>
            <span class="value"><button type="button" class="link ell" style="max-width:100%;text-align:left" onclick={() => ui.open(other.id)}>{other.title}</button></span>
            <span></span>
          </div>
        {/each}

        {#if reminderText}
          <div class="field">
            <span class="label">Reminder</span>
            <span class="value"><button type="button" class="link" onclick={() => (picker = 'reminder')}>{reminderText}</button></span>
            <span></span>
          </div>
        {/if}

        {#if detail.tags.length > 0}
          <div class="field">
            <span class="label">Tags</span>
            <span class="value gap wrap">
              {#each detail.tags as tag}
                <button type="button" class="chip" onclick={() => void ui.guard(() => ui.go(`tag:${tag}`))}>{tag}</button>
              {/each}
            </span>
            <span class="tools"><IconBtn icon="edit" size={16} label="Change tags" onclick={() => (picker = 'tags')} /></span>
          </div>
        {:else if guess}
          <div class="field">
            <span class="label">Tags</span>
            <span class="value"><button type="button" class="link" onclick={() => pass.change('entry.common', { id, tags: [guess] })}>Tag as {guess}</button></span>
            <span></span>
          </div>
        {/if}
      </div>

      {#if detail.history.length > 0}
        <div style="padding-top:14px">
          <button type="button" class="link" onclick={() => (historyOpen = !historyOpen)}>
            {historyOpen ? 'Hide history' : `History, ${detail.history.length} earlier`}
          </button>
        </div>
        {#if historyOpen}
          <div class="fields" style="margin-top:8px">
            {#each [...detail.history].sort((a, b) => b.at_ms - a.at_ms) as revision (revision.index)}
              {@render secretField(`${revision.field[0].toUpperCase()}${revision.field.slice(1)}, ${whenText(revision.at_ms)}`, `history:${revision.index}`, 'entry.reveal_history', { id, index: revision.index })}
            {/each}
          </div>
        {/if}
      {/if}

      <p class="foot-note">Made {whenText(detail.created_ms)}. Changed {whenText(detail.modified_ms)}.</p>
    </div>
  </div>
{/if}

{#if picker && detail}
  <Pickers kind={picker} entry={detail} onclose={() => (picker = null)} />
{/if}
{#if sending && detail}
  <SendItem id={detail.id} onclose={() => (sending = false)} />
{/if}
{#if image}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div style="position:fixed;inset:0;z-index:40;background:rgb(0 0 0 / 0.7);display:flex;align-items:center;justify-content:center;padding:32px" onclick={closeImage}>
    <img src={image.url} alt={image.name} style="max-width:100%;max-height:100%;object-fit:contain" />
  </div>
{/if}
