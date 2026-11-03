<script lang="ts">
  import HelpButton from '../help/HelpButton.svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Fold from '../parts/Fold.svelte';

  import IconBtn from '../parts/IconBtn.svelte';
  import Strength from '../parts/Strength.svelte';
  import Switch from '../parts/Switch.svelte';
  import Tile from '../parts/Tile.svelte';

  import { bytesOf, consumeText, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { kindName, oklch } from '../engine/face';
  import { siteLine, titleFromSite } from '../engine/text';
  import type { Detail, FieldView, StrengthView } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { GenState } from './gen.svelte';
  import GenControls from './GenControls.svelte';
  import GenValue from './GenValue.svelte';
  import Pickers, { type PickerKind } from './Pickers.svelte';
  import AliasPicker from '../settings/AliasPicker.svelte';
  import { ui, type Editing } from './ui.svelte';

  interface EditField {
    spec: FieldView;
    text: string;
    flag: boolean;
    millis: number | null;
    /** An existing secret stays in the vault until asked for. */
    loaded: boolean;
    original: string;
    showing: boolean;
  }
  interface EditCustom {
    index: number | null;
    name: string;
    value: string;
    original: string;
    hidden: boolean;
    loaded: boolean;
    remove: boolean;
  }
  interface EditQuestion {
    index: number | null;
    question: string;
    answer: string;
  }
  interface EditSite {
    text: string;
    raw: Record<string, any> | null;
  }

  /** Each kind's everyday fields. The rest wait under "More details". */
  const MAIN_FIELDS: Record<string, string[]> = {
    card: ['holder', 'number', 'expiry', 'cvv'],
    identity: ['full_name', 'email', 'phone'],
    ssh: ['private_key', 'public_key'],
    api: ['provider', 'key'],
    database: ['engine', 'host', 'port', 'name', 'user', 'password'],
    licence: ['product', 'key'],
    recovery: ['form', 'phrase'],
    wifi: ['ssid', 'password'],
  };

  let { editing }: { editing: Editing } = $props();

  const entryId = editing.id;
  const kind = editing.kind;
  let detail = $state<Detail | null>(null);
  let fields = $state<EditField[]>([]);
  let title = $state(editing.presetTitle ?? '');
  let sites = $state<EditSite[]>([]);
  let customs = $state<EditCustom[]>([]);
  let questions = $state<EditQuestion[]>([]);
  let notes = $state('');
  let notesLoaded = $state(entryId === null);
  let notesOriginal = $state('');
  let otpSeed = $state(editing.presetOtp ?? '');
  let color = $state<string | null>(null);
  let icon = $state<string | null>(null);
  let folder = $state<string | null>(null);
  let favourite = $state(false);
  let tags = $state<string[]>([]);
  let ready = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let picker = $state<PickerKind | null>(null);
  let generating = $state<EditField | null>(null);
  let aliasFor = $state<EditField | null>(null);
  let gen = $state<GenState | null>(null);
  let titleInput = $state<HTMLInputElement | HTMLTextAreaElement>();
  let rating = $state<Record<string, StrengthView | null>>({});

  function makeField(spec: FieldView): EditField {
    return {
      spec,
      text: spec.secret ? '' : spec.input === 'date' || spec.input === 'toggle' ? '' : spec.value == null ? '' : String(spec.value),
      flag: spec.value === true,
      millis: typeof spec.value === 'number' && spec.input === 'date' ? spec.value : null,
      loaded: !spec.has_value,
      original: spec.secret ? '' : spec.value == null ? '' : String(spec.value),
      showing: false,
    };
  }

  $effect(() => {
    void load();
  });

  async function load() {
    if (entryId) {
      const d = await pass.detail(entryId);
      detail = d;
      title = d.title;
      fields = d.fields.map(makeField);
      for (const raw of d.sites) {
        const line = siteLine(raw);
        if (line) sites.push({ text: line.label, raw });
      }
      customs = d.custom_fields.map((c) => ({
        index: c.index,
        name: c.name,
        value: c.value ?? '',
        original: c.value ?? '',
        hidden: c.value === null,
        loaded: c.value !== null,
        remove: false,
      }));
      questions = d.questions.map((question, index) => ({ index, question, answer: '' }));
      color = d.color;
      icon = d.icon;
      folder = d.folder;
      favourite = d.favourite;
      tags = [...d.tags];
      // A note's body is what you came to edit, so it opens straight away.
      const body = fields.find((f) => f.spec.key === 'body' && f.spec.has_value);
      if (body) {
        const text = await pass.reveal('entry.reveal', { id: entryId, field: 'body' });
        body.text = text;
        body.original = text;
        body.loaded = true;
      }
    } else {
      const spec = (await pass.kinds()).find((k) => k.kind === kind);
      fields = (spec?.fields ?? []).map((f) => {
        const field = makeField({ ...f, has_value: false, value: null });
        if (f.input === 'choice' && f.choices.length) field.text = f.choices[0];
        return field;
      });
      if (kind === 'login') sites.push({ text: '', raw: null });
      const password = fields.find((f) => f.spec.input === 'password');
      if (editing.presetPassword && password) password.text = editing.presetPassword;
      const body = fields.find((f) => f.spec.key === 'body');
      if (editing.presetNote && body) body.text = editing.presetNote;
      if (!editing.presetPassword && prefs.v.editor_generate && password) {
        const made = new GenState();
        await made.roll();
        password.text = made.value;
      }
    }
    ready = true;
    if (!entryId && !title) setTimeout(() => titleInput?.focus(), 30);
  }

  function changed(f: EditField) {
    if (f.spec.input === 'toggle') return f.flag !== (f.spec.value === true);
    if (f.spec.input === 'date') return f.millis !== (typeof f.spec.value === 'number' ? f.spec.value : null);
    if (f.spec.secret) return f.loaded && f.text !== f.original;
    return f.text !== f.original;
  }

  const firstSite = $derived(sites[0]?.text ?? '');
  const siteTitle = $derived(titleFromSite(firstSite));
  const previewSite = $derived.by(() => {
    if (!firstSite.trim()) return detail?.site ?? null;
    try {
      return new URL(firstSite.includes('://') ? firstSite : `https://${firstSite}`).hostname;
    } catch {
      return null;
    }
  });

  const dirty = $derived(
    fields.some(changed) ||
      title !== (detail?.title ?? editing.presetTitle ?? '') ||
      sites.some((s) => (s.raw === null ? s.text.trim() !== '' : siteLine(s.raw)?.label !== s.text)) ||
      (detail ? detail.sites.filter((s) => siteLine(s)).length !== sites.length : false) ||
      otpSeed !== '' ||
      (notesLoaded && notes !== notesOriginal) ||
      customs.some((c) => c.index === null ? c.name !== '' || c.value !== '' : c.remove || c.value !== c.original || c.name !== detail?.custom_fields[c.index]?.name) ||
      questions.some((q) => q.answer !== '') ||
      color !== (detail?.color ?? null) ||
      icon !== (detail?.icon ?? null) ||
      folder !== (detail?.folder ?? null) ||
      favourite !== (detail?.favourite ?? false) ||
      tags.join() !== (detail?.tags ?? []).join(),
  );

  $effect(() => {
    ui.editorDirty = dirty;
    return () => {
      ui.editorDirty = false;
    };
  });

  function jsonValue(f: EditField) {
    if (f.spec.input === 'toggle') return f.flag;
    if (f.spec.input === 'date') return f.millis;
    if (f.spec.input === 'number') {
      const n = Number(f.text.trim());
      return f.text.trim() === '' || Number.isNaN(n) ? null : n;
    }
    return f.text;
  }

  function close() {
    ui.editing = null;
    if (!entryId) ui.showDetail = false;
  }

  async function leave() {
    if (dirty) {
      const yes = await overlays.ask({ title: 'Discard changes?', body: 'Your edits to this item will be lost.', action: 'Discard', danger: true });
      if (!yes) return;
    }
    close();
  }

  async function secretCall(method: string, args: unknown, text: string) {
    const bytes = bytesOf(text);
    try {
      await pass.change(method, args, { secret: bytes });
    } finally {
      bytes.fill(0);
    }
  }

  export async function save() {
    if (saving) return;
    saving = true;
    error = null;
    try {
      const finalTitle = title.trim() || siteTitle || 'Untitled';
      const plain = fields.filter((f) => !f.spec.secret && changed(f));
      const fieldsJson = Object.fromEntries(plain.map((f) => [f.spec.key, jsonValue(f)]));
      const newSites: Record<string, any>[] = [];
      for (const site of sites) {
        const typed = site.text.trim();
        if (!typed) continue;
        if (site.raw && siteLine(site.raw)?.label === typed) newSites.push(site.raw);
        else newSites.push({ Url: typed.includes('://') ? typed : `https://${typed}` });
      }
      let id = entryId;
      if (!id) {
        const reply = await pass.change<{ id: string }>('entry.create', { kind, title: finalTitle, color, folder, fields: fieldsJson, sites: newSites });
        id = reply.json.id;
      } else if (plain.length) {
        await pass.change('entry.set', { id, fields: fieldsJson });
      }
      for (const f of fields.filter((f) => f.spec.secret && changed(f))) {
        await secretCall('entry.set_secret', { id, field: f.spec.key }, f.text);
      }
      if (notesLoaded && notes !== notesOriginal) await secretCall('entry.set_secret', { id, field: 'notes' }, notes);
      if (otpSeed.trim()) await secretCall('entry.otp_set', { id, ...(kind === 'otp' ? { issuer: finalTitle } : {}) }, otpSeed);
      // Removed from the end first, so earlier indexes stay right.
      for (const c of customs.filter((c) => c.remove && c.index !== null).sort((a, b) => b.index! - a.index!)) {
        await pass.change('entry.custom_remove', { id, index: c.index });
      }
      for (const c of customs.filter((c) => !c.remove && c.name.trim() && (c.index === null || c.value !== c.original || c.name !== detail?.custom_fields[c.index]?.name))) {
        if (c.hidden && !c.loaded) continue;
        await secretCall('entry.custom_set', { id, index: c.index, name: c.name.trim(), kind: c.hidden ? 'hidden' : 'text' }, c.value);
      }
      for (const q of questions.filter((q) => q.answer && q.question.trim())) {
        await secretCall('entry.question_set', { id, index: q.index, question: q.question.trim() }, q.answer);
      }
      await pass.change('entry.common', {
        id,
        title: finalTitle,
        color,
        icon,
        folder,
        favourite,
        tags,
        ...(kind === 'login' ? { sites: newSites } : {}),
      });
      ui.editing = null;
      ui.open(id!);
    } catch (failure: any) {
      error = failure.message;
    } finally {
      saving = false;
    }
  }

  async function load_secret(f: EditField) {
    if (entryId) {
      const text = await pass.reveal('entry.reveal', { id: entryId, field: f.spec.key });
      f.text = text;
      f.original = text;
    }
    f.loaded = true;
  }

  async function loadNotes() {
    if (detail?.has_notes) {
      const text = await pass.reveal('entry.reveal', { id: detail.id, field: 'notes' });
      notes = text;
      notesOriginal = text;
    }
    notesLoaded = true;
  }

  async function loadCustom(c: EditCustom) {
    if (entryId && c.index !== null) {
      const text = await pass.reveal('entry.reveal_custom', { id: entryId, index: c.index });
      c.value = text;
      c.original = text;
    }
    c.loaded = true;
  }

  async function newSshKey(f: EditField) {
    const reply = await pass.engine('ssh.new', { comment: title });
    f.text = consumeText(reply.secret);
    f.loaded = true;
    const pub = fields.find((x) => x.spec.key === 'public_key');
    const fp = fields.find((x) => x.spec.key === 'fingerprint');
    if (pub) pub.text = reply.json.public_key;
    if (fp) fp.text = reply.json.fingerprint;
  }

  function openGenerator(f: EditField) {
    gen = new GenState();
    generating = f;
    void gen.roll();
  }

  function rate(f: EditField) {
    const text = f.text;
    const key = f.spec.key;
    setTimeout(async () => {
      if (f.text !== text) return;
      if (!text) {
        rating = { ...rating, [key]: null };
        return;
      }
      const bytes = bytesOf(text);
      const view = await pass.engine<StrengthView>('strength.assess', { context: title ? [title] : [] }, { secret: bytes });
      bytes.fill(0);
      rating = { ...rating, [key]: view.json };
    }, 120);
  }

  function hintOf(key: string) {
    const table: Record<string, string> = { username: 'Email or username', email: 'name@example.com', phone: '+1 555 0100', host: 'db.example.com', port: '5432', ssid: 'Network name' };
    return table[key] ?? 'Not set';
  }

  function isOpen(key: string, hasContent: boolean) {
    return hasContent || prefs.listOf('editor_open').includes(key);
  }

  function remember(key: string, hasContent: boolean, open: boolean) {
    if (prefs.v.editor_remember && !hasContent) prefs.toggleInList('editor_open', key, open);
  }

  const mainKeys = $derived(prefs.v.editor_fold_details ? MAIN_FIELDS[kind] : undefined);
  const mainRows = $derived(fields.filter((f) => !mainKeys || mainKeys.includes(f.spec.key)));
  const detailRows = $derived(fields.filter((f) => mainKeys && !mainKeys.includes(f.spec.key)));
  const dateText = (ms: number | null) => (ms ? new Date(ms).toISOString().slice(0, 10) : '');
  const hasContent = (f: EditField) => !f.loaded || f.text !== '' || f.flag || f.millis !== null;
  const liveCustoms = $derived(customs.filter((c) => !c.remove).length);
  const folderName = $derived(pass.folders.find((f) => f.id === folder)?.name ?? 'None');
</script>

{#snippet fieldEditor(f: EditField)}
  {@const spec = f.spec}
  {#if spec.input === 'toggle'}
    <div class="form-row"><span class="label">{spec.label}</span><span><Switch checked={f.flag} label={spec.label} onchange={(v) => (f.flag = v)} /></span></div>
  {:else if spec.input === 'choice'}
    <div class="form-row">
      <span class="label">{spec.label}</span>
      <span class="gap wrap">
        {#each spec.choices as choice}
          <button type="button" class="chip" aria-pressed={f.text === choice} onclick={() => (f.text = choice)}>{choice.length <= 4 ? choice.toUpperCase() : choice[0].toUpperCase() + choice.slice(1)}</button>
        {/each}
      </span>
    </div>
  {:else if spec.input === 'date'}
    <div class="form-row">
      <span class="label">{spec.label}</span>
      <div class="input" style="max-width:180px">
        <input
          type="date"
          aria-label={spec.label}
          value={dateText(f.millis)}
          onchange={(e) => (f.millis = e.currentTarget.value ? new Date(e.currentTarget.value).getTime() : null)}
        />
      </div>
    </div>
  {:else if spec.secret && !f.loaded}
    <div class="form-row">
      <span class="label">{spec.label}</span>
      <span><Btn label="Saved. Click to change" onclick={() => load_secret(f)} /></span>
    </div>
  {:else if spec.input === 'multiline'}
    <div class="form-row top">
      <span class="label">{spec.label}</span>
      <Field bind:value={f.text} multiline rows={spec.key === 'body' ? 10 : 4} mono={spec.secret} placeholder={spec.key === 'body' ? '' : hintOf(spec.key)} label={spec.label}>
        {#snippet trail()}
          {#if spec.key === 'private_key'}<IconBtn icon="casino" label="Generate a key" size={16} onclick={() => newSshKey(f)} />{/if}
        {/snippet}
      </Field>
    </div>
  {:else}
    <div class="form-row">
      <span class="label">{spec.label}</span>
      <div>
        <Field
          bind:value={f.text}
          type={spec.secret && !f.showing ? 'password' : 'text'}
          mono={spec.secret}
          placeholder={hintOf(spec.key)}
          label={spec.label}
          oninput={() => spec.input === 'password' && rate(f)}
        >
          {#snippet trail()}
            {#if spec.secret && f.text}<IconBtn icon={f.showing ? 'visibility_off' : 'visibility'} label={f.showing ? 'Hide' : 'Show'} size={16} onclick={() => (f.showing = !f.showing)} />{/if}
            {#if spec.input === 'password'}<IconBtn icon="casino" label="Generate" size={16} on onclick={() => openGenerator(f)} />{/if}
            {#if spec.key === 'username'}<IconBtn icon="alternate_email" label="Use an alias" size={16} on onclick={() => (aliasFor = f)} />{/if}
          {/snippet}
        </Field>
        {#if spec.input === 'password' && f.loaded && f.text && rating[spec.key]}
          {@const s = rating[spec.key]!}
          <div style="padding-top:5px">
            <Strength score={s.score} />
            <p class="small" class:danger-text={s.score < 2} class:dim={s.score >= 2} style="padding-top:3px">{s.warning || `Time to crack: ${s.crack_slow_hash}`}</p>
          </div>
        {/if}
      </div>
    </div>
  {/if}
{/snippet}

<div class="detail-bar" data-tauri-drag-region>
  <IconBtn icon="close" label="Close" onclick={leave} />
  <span class="grow" style="font-weight:600" data-tauri-drag-region>{entryId ? 'Edit' : 'New'} {kindName(kind).toLowerCase()}</span>
  <HelpButton id="screen.editor" title="the editor" />
  <span data-help-anchor="editor.save" style="display:flex"><Btn label={saving ? 'Saving…' : 'Save'} variant="primary" disabled={!ready || saving || (!dirty && !!entryId)} onclick={save} /></span>
</div>

<div class="detail-body scroll">
  <div class="detail-inner">
    <div class="form-row" style="padding-bottom:8px" data-help-anchor="editor.name">
      <span style="justify-self:end"><Tile title={title || siteTitle} site={previewSite} {kind} label={color} {icon} size={36} /></span>
      <Field bind:value={title} bind:ref={titleInput} placeholder={siteTitle || 'Name'} label="Name" />
    </div>

    {#if ready}
      <div data-help-anchor="editor.fields">{#each mainRows as f (f.spec.key)}{@render fieldEditor(f)}{/each}</div>

      <div data-help-anchor="editor.folds">
      {#if detailRows.length > 0}
        {@const content = detailRows.some(hasContent)}
        <Fold title="More details" summary={detailRows.map((f) => f.spec.label).join(', ')} open={isOpen('details', content)} ontoggle={(o) => remember('details', content, o)}>
          {#each detailRows as f (f.spec.key)}{@render fieldEditor(f)}{/each}
        </Fold>
      {/if}

      {#if kind === 'login'}
        {@const content = firstSite !== '' || sites.length > 1}
        <Fold title="Websites" summary={firstSite || null} open={isOpen('sites', content)} ontoggle={(o) => remember('sites', content, o)}>
          {#each sites as site, index}
            <div class="form-row">
              <span class="label">Website</span>
              <Field bind:value={site.text} placeholder="example.com" label="Website">
                {#snippet trail()}
                  {#if sites.length > 1 || site.text}<IconBtn icon="close" label="Remove website" size={14} onclick={() => sites.splice(index, 1)} />{/if}
                {/snippet}
              </Field>
            </div>
          {/each}
          <Btn label="Add a website" icon="add" variant="quiet" onclick={() => sites.push({ text: '', raw: null })} />
        </Fold>
      {/if}

      {#if kind === 'login' || kind === 'otp'}
        {@const content = kind === 'otp' || !!detail?.otp || otpSeed !== ''}
        <Fold title="One-time code" summary={detail?.otp ? 'Saved' : null} open={isOpen('otp', content)} ontoggle={(o) => remember('otp', content, o)}>
          <div class="form-row">
            <span class="label">Setup key or otpauth link</span>
            <Field bind:value={otpSeed} mono placeholder={detail?.otp ? 'Saved. Type to replace' : 'JBSWY3DPEHPK3PXP'} label="Setup key or otpauth link" />
          </div>
        </Fold>
      {/if}

      {#if kind !== 'note'}
        {@const content = !!detail?.has_notes || notes !== ''}
        <Fold title="Notes" summary={detail?.has_notes ? 'Saved' : null} open={isOpen('notes', content)} ontoggle={(o) => remember('notes', content, o)}>
          {#if notesLoaded || !detail?.has_notes}
            <Field bind:value={notes} multiline rows={4} placeholder="Notes" label="Notes" />
          {:else}
            <Btn label="Edit notes" icon="edit" variant="quiet" onclick={loadNotes} />
          {/if}
        </Fold>
      {/if}

      <Fold title="More fields" summary={liveCustoms ? String(liveCustoms) : null} open={isOpen('custom', liveCustoms > 0)} ontoggle={(o) => remember('custom', liveCustoms > 0, o)}>
        {#each customs as custom}
          {#if !custom.remove}
            <div class="gap" style="padding:3px 0;align-items:flex-start">
              <div style="width:130px;flex:none"><Field bind:value={custom.name} placeholder="Name" /></div>
              <div class="grow">
                {#if custom.hidden && !custom.loaded}
                  <Btn label="Saved. Click to change" onclick={() => loadCustom(custom)} />
                {:else}
                  <Field bind:value={custom.value} placeholder="Value" mono={custom.hidden} type={custom.hidden ? 'password' : 'text'} />
                {/if}
              </div>
              <IconBtn icon="close" label="Remove field" size={16} onclick={() => (custom.remove = true)} />
            </div>
          {/if}
        {/each}
        <div class="gap">
          <Btn label="Add a field" icon="add" variant="quiet" onclick={() => customs.push({ index: null, name: '', value: '', original: '', hidden: false, loaded: true, remove: false })} />
          <Btn label="Add a hidden field" icon="visibility_off" variant="quiet" onclick={() => customs.push({ index: null, name: '', value: '', original: '', hidden: true, loaded: true, remove: false })} />
        </div>
      </Fold>

      {#if kind === 'login'}
        {@const content = questions.length > 0}
        <Fold title="Security questions" summary={questions.length ? String(questions.length) : null} open={isOpen('questions', content)} ontoggle={(o) => remember('questions', content, o)}>
          {#each questions as q}
            <div class="stack" style="gap:4px;padding:4px 0">
              <Field bind:value={q.question} placeholder="Question" />
              <Field bind:value={q.answer} mono placeholder={q.index !== null ? 'Saved. Type to replace the answer' : 'Answer'} />
            </div>
          {/each}
          <Btn label="Add a question" icon="add" variant="quiet" onclick={() => questions.push({ index: null, question: '', answer: '' })} />
        </Fold>
      {/if}

      <Fold title="Organize" summary="Folder, tags, icon, color and favorite" open={isOpen('organise', false)} ontoggle={(o) => remember('organise', false, o)}>
        <div class="form-row"><span class="label">Folder</span><span><Btn label={folderName} icon="folder" onclick={() => (picker = 'folder')} /></span></div>
        <div class="form-row"><span class="label">Tags</span><span><Btn label={tags.length ? tags.join(', ') : 'None'} icon="sell" onclick={() => (picker = 'tags')} /></span></div>
        <div class="form-row"><span class="label">Icon</span><span><Btn label={icon ? 'Your choice' : 'Automatic'} icon="image" onclick={() => (picker = 'icon')} /></span></div>
        <div class="form-row">
          <span class="label">Colour</span>
          <span>
            <Btn label={color ? 'Your choice' : 'From the site'} onclick={() => (picker = 'colour')}>
              {#if color && !Number.isNaN(Number(color))}<span style="width:12px;height:12px;border-radius:50%;background:{oklch(0.58, 0.11, Number(color))}"></span>{/if}
            </Btn>
          </span>
        </div>
        <div class="form-row"><span class="label">Favorite</span><span><Switch checked={favourite} label="Favorite" onchange={(v) => (favourite = v)} /></span></div>
      </Fold>
      </div>
    {/if}

    {#if error}<p class="danger-text" style="padding-top:10px">{error}</p>{/if}
  </div>
</div>

{#if picker === 'folder'}
  <Pickers kind="folder" current={folder} onclose={() => (picker = null)} onpick={(v) => (folder = v)} />
{:else if picker === 'tags'}
  <Pickers kind="tags" current={tags} onclose={() => (picker = null)} onpick={(v) => (tags = v)} />
{:else if picker === 'colour'}
  <Pickers kind="colour" current={color} onclose={() => (picker = null)} onpick={(v) => (color = v)} />
{:else if picker === 'icon'}
  <Pickers kind="icon" current={icon} onclose={() => (picker = null)} onpick={(v) => (icon = v)} />
{/if}

{#if generating && gen}
  <Dialog title="Generate password" wide onclose={() => (generating = null)}>
    <div class="stack" style="gap:12px">
      <div><GenValue state={gen} /></div>
      <GenControls state={gen} />
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (generating = null)} />
      <Btn label="Use this" variant="primary" onclick={() => { const f = generating!; f.loaded = true; f.text = gen!.value; f.showing = true; rate(f); generating = null; }} />
    {/snippet}
  </Dialog>
{/if}

{#if aliasFor}
  <AliasPicker hint={title} onclose={() => (aliasFor = null)} onpick={(address) => { aliasFor!.text = address; aliasFor = null; }} />
{/if}
