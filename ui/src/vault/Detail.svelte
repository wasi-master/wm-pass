<script lang="ts" module>
  import { pass as engine } from '../engine/pass.svelte';
  import type { Detail as DetailView, OtpRow as CodeRow } from '../engine/types';

  /** Everything an item's page shows on its first frame. */
  export type Loaded = { revision: number; detail: DetailView; suggested: string[] | null; row: CodeRow | null };

  /**
   * Fetches an item's page before it is shown, so the page that is already there stays until the
   * new one can be drawn whole, rather than going blank for the round trip to the engine.
   */
  export async function loadDetail(id: string): Promise<Loaded> {
    const revision = engine.revision;
    const detail = await engine.detail(id);
    const [suggested, row] = await Promise.all([
      detail.trashed
        ? null
        : engine
            .view<{ categories: string[] }>('categorise.suggest', { id })
            .then((r) => r.categories ?? [])
            .catch(() => null),
      detail.otp ? engine.codes([id]).then((rows) => rows[0] ?? null, () => null) : null,
    ]);
    return { revision, detail, suggested, row };
  }
</script>

<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import HelpButton from '../help/HelpButton.svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Qr from '../parts/Qr.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Ring from '../parts/Ring.svelte';
  import SecretText from '../parts/SecretText.svelte';
  import Strength from '../parts/Strength.svelte';
  import TagChip from '../parts/TagChip.svelte';
  import Tile from '../parts/Tile.svelte';
  import { bytesOf, consumeText, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { host, type PickedFile } from '../engine/host';
  import { kindName } from '../engine/face';
  import { everySecond } from '../engine/clock';
  import { grouped, plural, siteLine, sizeText, whenText } from '../engine/text';
  import type { AttachmentView, Detail, OtpRow, StrengthView } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { ui } from './ui.svelte';
  import { entryMenu, runEntryMenu } from './entryMenu';
  import Pickers, { type PickerKind } from './Pickers.svelte';
  import SendItem from './SendItem.svelte';

  let { id, initial = null }: { id: string; initial?: Loaded | null } = $props();

  // Only the first frame comes from `initial`, and only while it is current; from then on the page
  // keeps itself up to date.
  const start = untrack(() => (initial && initial.revision === pass.revision ? initial : null));
  let detail = $state<Detail | null>(start?.detail ?? null);
  let revealed = $state<Record<string, string>>({});
  let row = $state<OtpRow | null>(start?.row ?? null);
  let hotp = $state<string | null>(null);
  let holds = $state<StrengthView | null>(null);
  let historyOpen = $state(false);
  let picker = $state<PickerKind | null>(null);
  /** Tags the item's site and kind suggest. */
  let suggested = $state<string[]>(start?.suggested ?? []);
  // Set from `initial` for the first detail; asked again when the item changes.
  let suggestedReady = start?.suggested != null;
  let sending = $state(false);
  let image = $state<{ name: string; url: string } | null>(null);
  let wifiCode = $state(false);

  // A page that came with `initial` already has this revision; it fetches again once it changes.
  let fetched = start ? start.revision : -1;
  $effect(() => {
    const revision = pass.revision;
    const current = id;
    if (revision === fetched) return;
    fetched = -1;
    pass
      .detail(current)
      .then((d) => {
        if (current === id) detail = d;
      })
      .catch(() => {
        if (current === id) {
          detail = null;
          ui.deselect(current);
        }
      });
  });

  // Leaving an item takes its secrets off the screen.
  let shownId = untrack(() => id);
  $effect(() => {
    if (id === shownId) return;
    shownId = id;
    revealed = {};
    hotp = null;
    holds = null;
    historyOpen = false;
    historyLimit = HISTORY_PAGE;
    suggested = [];
    wifiCode = false;
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
    const current = id;
    return everySecond(async () => {
      try {
        const found = (await pass.codes([current]))[0] ?? null;
        // A reply for the item just left does not land on the next one's page.
        if (current === id) row = found;
      } catch {
        if (current === id) row = null;
      }
    });
  });

  $effect(() => {
    if (detail && !detail.trashed) {
      const current = detail.id;
      void detail.title;
      void detail.sites;
      if (suggestedReady) {
        suggestedReady = false;
        return;
      }
      pass
        .view<{ categories: string[] }>('categorise.suggest', { id: current })
        .then((r) => {
          if (current === id) suggested = r.categories ?? [];
        })
        .catch(() => undefined);
    }
  });

  // The tags as the last click left them, until the item comes back with them: a second quick
  // click builds on the first, rather than on tags that have not changed yet and so undoing it.
  let tagsNow = $state<string[] | null>(null);
  let tagChanges = 0;
  const itemTags = $derived(tagsNow ?? detail?.tags ?? []);
  $effect(() => {
    const saved = detail?.tags;
    const now = untrack(() => tagsNow);
    if (!now || !saved) return;
    // Caught up, or nothing of ours still on its way (a tag picker may have changed them since).
    if (tagChanges === 0 || (saved.length === now.length && saved.every((t) => now.includes(t)))) tagsNow = null;
  });

  function setTags(next: string[]) {
    const tags = [...new Set(next)];
    tagsNow = tags;
    tagChanges += 1;
    void pass
      .change('entry.common', { id, tags })
      .catch((failure: any) => {
        tagsNow = null;
        overlays.toast(failure?.message ?? 'The tags could not be changed.');
      })
      .finally(() => (tagChanges -= 1));
  }

  async function toggle(key: string, method: string, args: unknown) {
    if (revealed[key] !== undefined) {
      const { [key]: _, ...rest } = revealed;
      revealed = rest;
      if (key === 'field:password') holds = null;
      return;
    }
    let text: string;
    try {
      text = await pass.reveal(method, args);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be shown.');
      return;
    }
    revealed = { ...revealed, [key]: text };
    if (key === 'field:password') {
      const bytes = bytesOf(text);
      try {
        const rated = await pass.strength(bytes);
        // Hidden again while it was rated: the rating goes with the password.
        if (revealed[key] !== undefined) holds = rated;
      } catch {
        holds = null;
      } finally {
        bytes.fill(0);
      }
    }
  }

  // A network another phone can join from its code: any with a name but an enterprise one.
  const wifiSsid = $derived(detail?.kind === 'wifi' ? String(detail.fields.find((f) => f.key === 'ssid')?.value ?? '') : '');
  const wifiShareable = $derived(!!wifiSsid && (detail?.fields.find((f) => f.key === 'security')?.value ?? 'wpa') !== 'eap');
  const username = $derived(detail?.fields.find((f) => f.key === 'username' || f.key === 'user')?.value as string | undefined);
  const url = $derived.by(() => {
    for (const site of detail?.sites ?? []) {
      const line = siteLine(site);
      if (line?.openable) return line.openable;
    }
    return null;
  });
  // Only a site that has such a page gets the line: the list says at once, and any other site is
  // asked once whether its standard address leads anywhere.
  let changeAt = $state<string | null>(null);
  let changeSite: string | null = null;
  $effect(() => {
    const listed = detail?.change_password ?? null;
    const site = detail?.kind === 'login' ? url : null;
    // The page reloads on every change to the vault: the line stays while its site does, rather
    // than going and coming back each time.
    if (listed || site !== changeSite) changeAt = listed;
    changeSite = site;
    if (listed || !site) return;
    void pass
      .changePasswordUrl(site)
      .then((found) => {
        if (url === site) changeAt = found;
      })
      .catch(() => {});
  });

  function openUrl(target: string) {
    void host()
      .openUrl(target.includes('://') ? target : `https://${target}`)
      .catch((failure: Error) => overlays.toast(failure?.message || 'That address could not be opened.'));
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
    let file: PickedFile | null = null;
    try {
      file = await host().pickFile('Attach a file');
      if (!file || !detail) return;
      await pass.addAttachment(detail.id, file.name, '', file.bytes);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'The file could not be attached.');
    } finally {
      file?.bytes.fill(0);
    }
  }

  /** A change from the page's own buttons, said out loud if the vault refuses it. */
  function setCommon(args: Record<string, unknown>) {
    void pass.change('entry.common', { id, ...args }).catch((failure: any) => overlays.toast(failure?.message ?? 'That could not be changed.'));
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
    try {
      if (choice === 'view') {
        const plain = await pass.openAttachment(id, file.blob);
        // Only a picture's type: a file named .png that says it is HTML would open as a page in
        // this one's name when its picture is opened in a tab of its own.
        const type = file.mime.toLowerCase().startsWith('image/') ? file.mime : 'image/png';
        const blob = new Blob([plain.slice()], { type });
        plain.fill(0);
        closeImage();
        image = { name: file.name, url: URL.createObjectURL(blob) };
      } else if (choice === 'save') {
        const yes = await overlays.ask({ title: 'Save a copy?', body: 'The copy will not be encrypted.', action: 'Save' });
        if (!yes) return;
        const plain = await pass.openAttachment(id, file.blob);
        try {
          await host().saveFile('Save a copy', file.name, plain);
        } finally {
          plain.fill(0);
        }
      } else if (choice === 'remove') {
        const yes = await overlays.ask({ title: `Remove “${file.name}”?`, body: 'The file is deleted from this vault.', action: 'Remove', danger: true });
        if (yes) await pass.change('attachment.remove', { id, blob: file.blob });
      }
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'That could not be done with the file.');
    }
  }

  function closeImage() {
    if (image) URL.revokeObjectURL(image.url);
    image = null;
  }

  // The decrypted picture goes with the page, when the vault locks or another item opens too.
  onDestroy(closeImage);

  async function nextHotp() {
    try {
      const reply = await pass.change('otp.hotp_next', { id });
      hotp = consumeText(reply.secret);
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'A new code could not be made.');
    }
  }

  function fieldText(value: unknown, input: string) {
    if (input === 'date' && typeof value === 'number') return new Date(value).toLocaleDateString(undefined, { dateStyle: 'medium', timeZone: 'UTC' });
    return String(value ?? '');
  }

  const others = $derived(detail ? detail.links.flatMap((link) => pass.entries.filter((e) => e.id === link)) : []);
  const folderName = $derived(pass.folders.find((f) => f.id === detail?.folder)?.name);
  const reminderText = $derived.by(() => {
    const r = detail?.reminders;
    if (r?.change_every_days) return r.change_every_days === 1 ? 'Every day' : `Every ${r.change_every_days} days`;
    // One already past reads as such: not "Expires 3 days ago".
    if (r?.expires_ms) return `${r.expires_ms < Date.now() ? 'Expired' : 'Expires'} ${whenText(r.expires_ms)}`;
    return null;
  });
  const copyOnClick = $derived(prefs.v.tap_copies);
  // The newest first, a page at a time: an item changed a thousand times has a thousand rows.
  const HISTORY_PAGE = 50;
  let historyLimit = $state(HISTORY_PAGE);
  const history = $derived(detail && historyOpen ? [...detail.history].sort((a, b) => b.at_ms - a.at_ms) : []);
  const currentCode = $derived(row?.code ?? hotp);
  // A counter-based code moves on once shown, so asking the engine again would copy the next one.
  function copyCurrentCode() {
    if (!row?.code && hotp) void ui.copyShownCode(hotp);
    else void ui.copyCode(id);
  }
</script>

{#snippet textField(label: string, value: string, copyAs: string = label, multiline = false)}
  <div class="field">
    <span class="label">{label}</span>
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <span class="value selectable" class:click={copyOnClick} style={multiline ? 'white-space:pre-wrap' : ''} title={copyOnClick ? 'Click to copy' : undefined} onclick={() => copyOnClick && !getSelection()?.toString() && ui.copyPlain(copyAs, value)}>{value}</span>
    <span class="tools"><IconBtn icon="content_copy" size={16} label="Copy {label}" onclick={() => ui.copyPlain(copyAs, value)} /></span>
  </div>
{/snippet}

{#snippet secretField(label: string, key: string, method: string, args: unknown, multiline = false, copyAs: string = label)}
  {@const shown = revealed[key]}
  <div class="field">
    <span class="label">{label}</span>
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <span class="value" class:click={copyOnClick && shown === undefined} style={multiline ? 'white-space:pre-wrap' : ''} title={copyOnClick && shown === undefined ? 'Click to copy' : undefined} onclick={() => copyOnClick && shown === undefined && ui.copySecret(copyAs, method, args)}>
      {#if shown !== undefined}
        {#if multiline}<span class="selectable">{shown}</span>{:else}<SecretText value={shown} />{/if}
      {:else}
        <span class="secret-dots mono" aria-hidden="true">••••••••••••</span><span class="sr-only">Hidden</span>
      {/if}
    </span>
    <span class="tools" class:keep={shown !== undefined}>
      <IconBtn icon={shown !== undefined ? 'visibility_off' : 'visibility'} size={16} label="{shown !== undefined ? 'Hide' : 'Show'} {label}" onclick={() => toggle(key, method, args)} />
      <IconBtn icon="content_copy" size={16} label="Copy {label}" onclick={() => ui.copySecret(copyAs, method, args)} />
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
        onclick={() => setCommon({ favourite: !detail!.favourite })}
      />
      <IconBtn icon="edit" label="Edit" onclick={() => ui.edit(id, detail!.kind)} />
      <span data-help-anchor="detail.more" style="display:flex"><IconBtn icon="more_horiz" label="More" menu onclick={moreMenu} /></span>
    {/if}
  </div>
  <div class="detail-body scroll">
    <div class="detail-inner">
      <div class="detail-head">
        <Tile title={detail.title} site={detail.site} kind={detail.kind} label={detail.color} icon={detail.icon} size={48} />
        <div class="grow">
          <h1 class="name selectable" style="overflow-wrap:anywhere" tabindex="-1">{detail.title || 'Untitled'}</h1>
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
      {:else if prefs.v.entry_actions && (detail.has_password || (username && prefs.v.entry_action_username) || (url && prefs.v.entry_action_open) || wifiShareable)}
        <div class="actions" data-help-anchor="detail.actions">
          {#if detail.has_password}<Btn label="Copy password" variant="primary" icon="content_copy" onclick={() => ui.copyPassword(id)} />{/if}
          {#if username && prefs.v.entry_action_username}<Btn label="Copy username" icon="person" variant={detail.has_password ? '' : 'primary'} onclick={() => ui.copyPlain('Username', username!)} />{/if}
          {#if url && prefs.v.entry_action_open}<Btn label="Open site" icon="open_in_new" onclick={() => openUrl(url!)} />{/if}
          {#if wifiShareable}<Btn label="QR code" icon="qr_code" variant={detail.has_password ? '' : 'primary'} onclick={() => (wifiCode = true)} />{/if}
        </div>
      {/if}

      {#if detail.destroy_at_ms}
        <div class="banner">
          <Icon name="timer" size={18} />
          <span class="grow">Deletes itself on {new Date(detail.destroy_at_ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</span>
          <Btn label="Keep it" variant="quiet" onclick={() => setCommon({ destroy_at_ms: null })} />
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
            <span class="value" class:click={!!currentCode} onclick={() => currentCode && copyCurrentCode()}>
              {#if currentCode}
                <span class="code-value small">{prefs.v.code_groups ? grouped(currentCode) : currentCode}</span>
                {#if row?.next_code && prefs.v.code_next}<span class="dim small mono" style="display:block">Next {grouped(row.next_code)}</span>{/if}
              {:else}
                <span class="dim">{detail.otp.period ? '…' : 'Counter-based'}</span>
              {/if}
            </span>
            <span class="tools keep gap">
              {#if detail.otp.period}
                {#if prefs.v.code_ring && row?.seconds_left != null}<Ring period={detail.otp.period} left={row.seconds_left} />{/if}
              {:else}
                <Btn label="New code" variant="quiet" onclick={nextHotp} />
              {/if}
              {#if currentCode}<IconBtn icon="content_copy" size={16} label="Copy code" onclick={copyCurrentCode} />{/if}
            </span>
          </div>
        {/if}

        {#each detail.sites as site}
          {@const line = siteLine(site)}
          {#if line?.app}
            <div class="field">
              <span class="label">App</span>
              <span class="value"><span class="selectable">{line.label}</span>{#if line.label !== line.app}<span class="dim small selectable" style="display:block">{line.app}</span>{/if}</span>
              <span></span>
            </div>
          {:else if line}
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
        {#if changeAt}
          {@const change = changeAt}
          <div class="field">
            <span class="label"></span>
            <span class="value">
              <button type="button" class="link" style="text-align:left" onclick={() => openUrl(change)}>Change the password on the site</button>
              <span class="dim small ell" style="display:block">{change.replace(/^https:\/\//, '')}</span>
            </span>
            <span class="tools">
              <IconBtn icon="content_copy" size={16} label="Copy the address" onclick={() => ui.copyPlain('Address', change)} />
            </span>
          </div>
        {/if}

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
          {@render secretField(question, `question:${index}`, 'entry.reveal_question', { id, index }, false, 'Answer')}
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
            <span class="value"><button type="button" class="link ell" style="max-width:100%;text-align:left" onclick={() => ui.open(other.id)}>{other.title || 'Untitled'}</button></span>
            <span></span>
          </div>
        {/each}

        {#if reminderText}
          <div class="field">
            <span class="label">Reminder</span>
            <!-- An item in the trash is only looked at, as its other buttons have it. -->
            <span class="value">{#if detail.trashed}{reminderText}{:else}<button type="button" class="link" onclick={() => (picker = 'reminder')}>{reminderText}</button>{/if}</span>
            <span></span>
          </div>
        {/if}

        {#if !detail.trashed || itemTags.length > 0}
          {@const offered = detail.trashed ? [] : suggested.filter((t) => !itemTags.includes(t))}
          <div class="field">
            <span class="label">Tags</span>
            <span class="value stack" style="gap:8px">
              <span class="gap wrap">
                {#each itemTags as tag (tag)}
                  <TagChip
                    {tag}
                    selected
                    onclick={() => void ui.guard(() => ui.go(`tag:${tag}`))}
                    onremove={detail.trashed ? undefined : () => setTags(itemTags.filter((t) => t !== tag))}
                    onmanage={() => (ui.managingTag = tag)}
                  />
                {/each}
                {#if !detail.trashed}
                  <button type="button" class="chip" onclick={() => (picker = 'tags')}><Icon name="add" size={14} />{itemTags.length ? 'Add' : 'Add a tag'}</button>
                {/if}
              </span>
              {#if offered.length}
                <span class="dim small gap" style="gap:6px"><Icon name="auto_awesome" size={14} />Suggested</span>
                <span class="gap wrap">
                  {#each offered as tag (tag)}
                    <button type="button" class="chip" title={tag} onclick={() => setTags([...itemTags, tag])}><Icon name="add" size={14} /><span class="ell">{tag}</span></button>
                  {/each}
                </span>
              {/if}
            </span>
            <span></span>
          </div>
        {/if}
      </div>

      {#if detail.history.length > 0}
        <div style="padding-top:14px">
          <button type="button" class="link" onclick={() => (historyOpen = !historyOpen)}>
            {historyOpen ? 'Hide history' : `Show history, ${plural(detail.history.length, 'earlier value')}`}
          </button>
        </div>
        {#if historyOpen}
          <div class="fields" style="margin-top:8px">
            {#each history.slice(0, historyLimit) as revision (revision.index)}
              {@render secretField(`${revision.field[0].toUpperCase()}${revision.field.slice(1)}, ${whenText(revision.at_ms)}`, `history:${revision.index}`, 'entry.reveal_history', { id, index: revision.index }, false, `Old ${revision.field}`)}
            {/each}
          </div>
          {#if history.length > historyLimit}
            <div style="padding-top:8px"><button type="button" class="link" onclick={() => (historyLimit += HISTORY_PAGE)}>Show {Math.min(HISTORY_PAGE, history.length - historyLimit)} older</button></div>
          {/if}
        {/if}
      {/if}

      <p class="foot-note">Made {whenText(detail.created_ms)}. Changed {whenText(detail.modified_ms)}.</p>
    </div>
  </div>
{/if}

{#if picker && detail}
  <Pickers kind={picker} entry={detail} {suggested} onclose={() => (picker = null)} />
{/if}
{#if sending && detail}
  <SendItem id={detail.id} onclose={() => (sending = false)} />
{/if}
{#if wifiCode && detail}
  <Dialog title={wifiSsid} onclose={() => (wifiCode = false)}>
    <div class="stack" style="align-items:center;gap:14px">
      <Qr from={{ method: 'wifi.qr', args: { id: detail.id } }} />
      <p class="dim" style="text-align:center">Point another phone's camera at this to join the network. The password is in the code, so anyone who can see it can read it.</p>
    </div>
    {#snippet foot()}<Btn label="Done" variant="primary" onclick={() => (wifiCode = false)} />{/snippet}
  </Dialog>
{/if}
{#if image}
  <!-- A sheet like any other: it takes the focus, keeps Tab inside, closes on Escape or a click
       outside, and hands the focus back to the attachment it was opened from. -->
  <Dialog title={image.name} class="image-sheet" onclose={closeImage}>
    <img src={image.url} alt={image.name} style="display:block;max-width:100%;max-height:70vh;margin:0 auto;object-fit:contain" />
    {#snippet foot()}<Btn label="Close" variant="primary" onclick={closeImage} />{/snippet}
  </Dialog>
{/if}
