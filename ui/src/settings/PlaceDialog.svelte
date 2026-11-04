<script lang="ts">
  import HelpButton from '../help/HelpButton.svelte';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Segmented from '../parts/Segmented.svelte';
  import Select from '../parts/Select.svelte';
  import Switch from '../parts/Switch.svelte';
  import { host } from '../engine/host';
  import { shownName } from '../hosts/folders';
  import { blankPlace, placeReady, places, type Place } from './places.svelte';
  import { DEFAULT_FOLDER, GROUPS, PLACE_KINDS, S3_PRESETS, WEBDAV_PRESETS, defaultPort, kindOf, s3Endpoint, webDavUrl } from './placeKinds';

  // Add or change a place to keep copies. Works at the door too, where no vault is open yet, so a
  // computer can be restored from a place that was set up elsewhere. An account is saved only once
  // it is signed in.
  let { existing = null, onclose, onsaved }: { existing?: Place | null; onclose: () => void; onsaved?: (place: Place) => void } = $props();

  const offered = new Set(host().places?.kinds ?? []);
  const kinds = PLACE_KINDS.filter((k) => offered.has(k.id) || k.id === existing?.kind);
  // Every text field bound below starts as a string: a bound value cannot start undefined.
  const blankExtra = () =>
    Object.fromEntries(
      ['server', 'folder', 'account', 'host', 'port', 'share', 'domain', 'private_key', 'key_passphrase', 'export', 'uid', 'gid', 'repository', 'branch', 'token'].map((key) => [key, '']),
    ) as Record<string, any>;
  const start = existing ? { ...existing, extra: { ...(existing.extra ?? {}) } } : blankPlace(kinds[0]?.id ?? 'folder');
  let place = $state<Place>(start);
  let x = $state<Record<string, any>>({ ...blankExtra(), ...Object.fromEntries(Object.entries(start.extra ?? {}).map(([k, v]) => [k, typeof v === 'number' ? String(v) : v])) });
  let problem = $state<string | null>(null);
  let busy = $state(false);
  let code = $state('');
  let mailbox = $state('');
  let needs = $state<string | null>(null);
  let pending = $state<string | undefined>(undefined);

  const kind = $derived(kindOf(place.kind));
  const webdav = $derived(WEBDAV_PRESETS.find((p) => p.id === x.preset));
  const s3 = $derived(S3_PRESETS.find((p) => p.id === x.preset));

  function pick(id: string) {
    place.kind = id;
    x = blankExtra();
    needs = null;
    pending = undefined;
    problem = null;
  }

  /** The place as typed so far, with presets turned into addresses. */
  function draft(): Place {
    const snap = $state.snapshot(place) as Place;
    const extra: Record<string, any> = { ...$state.snapshot(x) };
    for (const key of ['host', 'share', 'domain', 'export', 'server', 'repository', 'branch', 'folder', 'account']) {
      if (typeof extra[key] === 'string') extra[key] = extra[key].trim();
    }
    if (['sftp', 'ftp', 'smb', 'imap'].includes(snap.kind)) extra.port = Number(extra.port) || defaultPort(snap.kind, extra.security ?? 'tls');
    if (snap.kind === 'nfs') {
      extra.uid = Number(extra.uid ?? 1000) || 0;
      extra.gid = Number(extra.gid ?? 1000) || 0;
    }
    const out: Place = {
      ...snap,
      label: snap.label.trim() || (kind?.name ?? snap.kind),
      url: snap.url.trim(),
      user: snap.user.trim(),
      endpoint: snap.endpoint.trim(),
      region: snap.region.trim() || 'us-east-1',
      bucket: snap.bucket.trim(),
      prefix: snap.prefix.trim(),
      access_key: snap.access_key.trim(),
      extra,
    };
    if (out.kind === 'webdav' && webdav) out.url = webDavUrl(webdav, extra.server ?? '', out.user, extra.folder ?? '');
    if (out.kind === 's3' && s3) {
      out.endpoint = s3Endpoint(s3, out.region, extra.account ?? '');
      extra.path_style = !!s3.pathStyle;
    }
    return out;
  }

  const signsIn = $derived(!!kind?.browser || !!kind?.password);
  const ready = $derived.by(() => {
    const d = draft();
    if (kind?.browser) return true;
    if (kind?.password) return (!!d.user || !!d.extra?.server) && !!d.password && (!needs || needs === 'approval' || !!code.trim() || !!mailbox);
    return placeReady(d);
  });

  async function finish(saved: Place) {
    await places.put(saved);
    onsaved?.(saved);
    onclose();
  }

  async function save() {
    busy = true;
    problem = null;
    try {
      await finish(draft());
    } catch (failure: any) {
      problem = failure.message;
    } finally {
      busy = false;
    }
  }

  async function signIn() {
    const shell = host().places!;
    if (!shell.signIn) return;
    busy = true;
    problem = null;
    const d = draft();
    try {
      const answer = await shell.signIn(d, { code: code.trim() || undefined, mailbox_password: mailbox || undefined, pending });
      if ('extra' in answer) {
        // A password sign-in keeps the session it was given, not the password, where the service allows.
        const keepsPassword = ['icloud', 'synology', 'qnap'].includes(d.kind);
        await finish({ ...d, password: keepsPassword ? d.password : '', extra: { ...d.extra, ...answer.extra } });
        return;
      }
      needs = answer.needs;
      pending = answer.pending;
      code = '';
      if (answer.needs === 'approval') problem = 'Approve this sign-in on one of your devices, then try again.';
      if (answer.needs === 'captcha') problem = 'The service wants a CAPTCHA first. Sign in once on its website from this network, then try again here.';
    } catch (failure: any) {
      problem = failure.message;
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title={existing ? existing.label : 'A place for copies'} {onclose}>
  <div class="stack">
    {#if !existing}
      <Select
        label="Kind of place"
        value={place.kind}
        options={GROUPS.flatMap((group, index) => kinds.filter((k) => k.group === index).map((k) => [k.id, `${k.name}  ·  ${group}`] as [string, string]))}
        onchange={pick}
      />
    {/if}
    <p class="dim">
      {#if place.kind === 'folder'}
        Pick a folder. Syncthing, Nextcloud or a cloud provider’s own app can carry it from there.
      {:else if kind?.browser}
        You sign in on the service’s own page, in your browser. WM Pass never sees that password, and can reach only its own folder there.
      {:else if ['pcloud', 'mega', 'proton'].includes(place.kind)}
        Only the encrypted file goes there. WM Pass keeps the session the sign-in hands back, not your password.
      {:else}
        Only the encrypted file goes there, so the server cannot read it. Its login is kept on this computer in the system’s keychain.
      {/if}
    </p>
    <Field bind:value={place.label} placeholder="A name for it" />

    {#if place.kind === 'folder'}
      <Btn label={place.path ? shownName(place.path) : 'Choose a folder'} icon="folder" onclick={async () => (place.path = (await host().places!.chooseFolder()) ?? place.path)} wide />
    {:else if place.kind === 'webdav'}
      <Select label="Service" value={x.preset ?? ''} options={[['', 'Type the address'], ...WEBDAV_PRESETS.map((p) => [p.id, p.label] as [string, string])]} onchange={(id) => (x.preset = id)} />
      {#if !webdav}
        <Field bind:value={place.url} placeholder="https://example.com/remote.php/dav/files/you/" />
      {:else}
        {#if webdav.server}
          <Field bind:value={x.server} placeholder={webdav.server === 'account' ? 'Account number' : webdav.server === 'taildrive' ? 'Share, as tailnet/machine/share' : 'Server, such as cloud.example.com'} />
        {/if}
        <Field bind:value={x.folder} placeholder={`Folder there, ${DEFAULT_FOLDER} if empty`} />
      {/if}
      {#if webdav?.server !== 'taildrive'}
        <Field bind:value={place.user} placeholder="User name" />
        <PasswordField bind:value={place.password} placeholder="Password" />
      {/if}
    {:else if place.kind === 's3'}
      <Select
        label="Service"
        value={x.preset ?? ''}
        options={[['', 'Type the endpoint'], ...S3_PRESETS.map((p) => [p.id, p.label] as [string, string])]}
        onchange={(id) => {
          x.preset = id;
          const chosen = S3_PRESETS.find((p) => p.id === id);
          if (chosen) place.region = chosen.region;
        }}
      />
      {#if !s3}<Field bind:value={place.endpoint} placeholder="Endpoint, empty for AWS" />{/if}
      {#if s3?.account}
        <Field bind:value={x.account} placeholder={s3.account === 'account_id' ? 'Account id' : s3.account === 'endpoint' ? 'Endpoint, from the service’s console' : 'Server address'} />
      {/if}
      {#if s3?.regions}
        <Select label="Region" value={place.region} options={s3.regions.map((r) => [r, r] as [string, string])} onchange={(r) => (place.region = r)} />
      {:else}
        <Field bind:value={place.region} placeholder="Region" />
      {/if}
      <Field bind:value={place.bucket} placeholder="Bucket" />
      <Field bind:value={place.prefix} placeholder="Folder inside it, if any" />
      <Field bind:value={place.access_key} placeholder="Access key" />
      <PasswordField bind:value={place.secret_key} placeholder="Secret key" />
      {#if !s3}
        <label class="line"><span>Path-style addresses, for MinIO, Garage or an IP address</span><Switch checked={!!x.path_style} label="Path-style addresses" onchange={(v) => (x.path_style = v)} /><HelpButton id="action.place_path_style" title="Path-style addresses" /></label>
      {/if}
    {:else if ['sftp', 'ftp', 'smb', 'imap'].includes(place.kind)}
      <Field bind:value={x.host} placeholder="Host" />
      <Field bind:value={x.port} placeholder={`Port, ${defaultPort(place.kind, x.security ?? 'tls')} if empty`} />
      {#if place.kind === 'smb'}
        <Field bind:value={x.share} placeholder="Share" />
        <Field bind:value={x.domain} placeholder="Domain or workgroup, if any" />
      {/if}
      {#if place.kind === 'imap'}
        <Segmented value={x.security ?? 'tls'} options={[['tls', 'TLS'], ['starttls', 'STARTTLS'], ['none', 'None']]} onchange={(v) => (x.security = v)} />
      {/if}
      <Field bind:value={place.user} placeholder={place.kind === 'imap' ? 'Email address or user name' : 'User name'} />
      <PasswordField bind:value={place.password} placeholder="Password" />
      {#if place.kind === 'sftp'}
        <Field bind:value={x.private_key} placeholder="Private key, OpenSSH format, if it signs in with one" multiline rows={3} mono />
        {#if x.private_key}<PasswordField bind:value={x.key_passphrase} placeholder="Key passphrase, if any" />{/if}
      {/if}
      {#if place.kind === 'imap'}
        <Field bind:value={x.folder} placeholder="Mailbox, WM Pass backups if empty" />
      {:else}
        <Field bind:value={place.path} placeholder="Folder on the server" />
      {/if}
      {#if place.kind === 'sftp'}
        <p class="dim small">The server’s key is remembered on the first connection. If it ever changes, WM Pass refuses to connect.</p>
      {:else if place.kind === 'ftp'}
        <label class="line"><span>Encrypt the connection. Plain FTP sends the password as text.</span><Switch checked={x.secure !== false} label="Encrypt the connection" onchange={(v) => (x.secure = v)} /><HelpButton id="action.place_ftp_encrypt" title="Encrypt the connection" /></label>
      {:else if place.kind === 'smb'}
        <label class="line"><span>Require encryption. Off lets an older server sign without encrypting.</span><Switch checked={x.encrypt !== false} label="Require encryption" onchange={(v) => (x.encrypt = v)} /><HelpButton id="action.place_smb_encrypt" title="Require encryption" /></label>
      {:else if x.security === 'none'}
        <p class="dim small">Without TLS the password crosses the network as text.</p>
      {/if}
    {:else if place.kind === 'nfs'}
      <Field bind:value={x.host} placeholder="Host" />
      <Field bind:value={x.export} placeholder="Export, such as /srv/nfs" />
      <Field bind:value={place.path} placeholder="Folder inside it, if any" />
      <Field bind:value={x.uid} placeholder="User id, 1000 if empty" />
      <Field bind:value={x.gid} placeholder="Group id, 1000 if empty" />
      <p class="dim small">NFS without Kerberos takes these ids on trust and does not encrypt. Use it on a network you control.</p>
    {:else if place.kind === 'synology' || place.kind === 'qnap'}
      <Field bind:value={x.server} placeholder={place.kind === 'synology' ? 'nas.local or 192.168.1.20:5001' : 'nas.local or 192.168.1.20'} />
      <Field bind:value={place.user} placeholder="User name" />
      <PasswordField bind:value={place.password} placeholder="Password" />
      <Field bind:value={place.path} placeholder={place.kind === 'synology' ? 'Folder, such as /home/Drive/WM Pass' : 'Folder, such as /Public/WM Pass'} />
      <label class="line">
        <span>Accept the NAS’s own certificate. The first one seen is remembered; a different one later is refused.</span>
        <Switch checked={!!x.own_certificate} label="Accept the NAS’s own certificate" onchange={(v) => { x.own_certificate = v; x.cert_pin = ''; }} />
        <HelpButton id="action.place_nas_certificate" title="Accept the NAS’s own certificate" />
      </label>
    {:else if place.kind === 'git'}
      <Segmented value={x.provider ?? 'github'} options={[['github', 'GitHub'], ['gitlab', 'GitLab'], ['gitea', 'Gitea']]} onchange={(v) => (x.provider = v)} />
      <Field bind:value={x.server} placeholder="Server, the provider’s own if empty" />
      <Field bind:value={x.repository} placeholder="owner/repository" />
      <Field bind:value={x.branch} placeholder="Branch, the default one if empty" />
      <Field bind:value={place.path} placeholder="Folder in the repository, if any" />
      <PasswordField bind:value={x.token} placeholder="Access token" />
      <label class="line"><span>Allow a public repository. Anyone could download the encrypted file.</span><Switch checked={!!x.allow_public} label="Allow a public repository" onchange={(v) => (x.allow_public = v)} /><HelpButton id="action.place_git_public" title="Allow a public repository" /></label>
    {:else if place.kind === 'drive'}
      <Segmented value={x.drive_space ?? 'appdata'} options={[['appdata', 'Hidden app folder'], ['folder', 'A folder you can see']]} onchange={(v) => (x.drive_space = v)} />
      {#if x.drive_space === 'folder'}<Field bind:value={x.folder} placeholder={`Folder in My Drive, ${DEFAULT_FOLDER} if empty`} />{/if}
    {:else if place.kind === 'dropbox' || place.kind === 'onedrive'}
      <p class="dim small">Copies go to WM Pass’s own app folder there.</p>
    {:else}
      {#if place.kind === 'pcloud'}
        <Segmented value={x.region ?? 'us'} options={[['us', 'United States'], ['eu', 'Europe']]} onchange={(v) => (x.region = v)} />
      {/if}
      {#if place.kind !== 'box'}
        <Field bind:value={place.user} placeholder={place.kind === 'icloud' ? 'Apple Account email' : place.kind === 'proton' ? 'Proton address or user name' : 'Email'} />
        <PasswordField bind:value={place.password} placeholder="Password" />
      {/if}
      {#if needs === 'mailbox_password'}<PasswordField bind:value={mailbox} placeholder="Mailbox password" />{/if}
      <Field bind:value={x.folder} placeholder={`Folder there, ${DEFAULT_FOLDER} if empty`} />
      {#if place.kind === 'icloud'}<p class="dim small">Apple asks for a code from one of your devices. The sign-in lasts about a month, then WM Pass asks again.</p>{/if}
    {/if}
    {#if needs === 'code'}<Field bind:value={code} placeholder="Code" autofocus />{/if}
    {#if problem}<p class="danger-text">{problem}</p>{/if}
  </div>
  {#snippet foot()}
    <Btn label="Cancel" onclick={onclose} />
    {#if existing && signsIn && placeReady(existing)}
      <Btn label="Keep the sign-in" disabled={busy} onclick={() => finish({ ...draft(), password: existing.password, extra: { ...existing.extra, ...draft().extra } })} />
    {/if}
    {#if signsIn}
      <Btn
        label={busy ? (kind?.browser ? 'Waiting for the browser…' : 'Signing in…') : needs === 'code' || needs === 'mailbox_password' ? 'Continue' : kind?.browser ? `Sign in with ${kind.name}` : 'Sign in'}
        variant="primary"
        disabled={!ready || busy}
        onclick={signIn}
      />
    {:else}
      <Btn label="Save" variant="primary" disabled={!ready || busy} onclick={save} />
    {/if}
  {/snippet}
</Dialog>

<style>
  .line {
    display: flex;
    align-items: center;
    gap: 12px;
    justify-content: space-between;
  }
  .small {
    font-size: 0.85em;
  }
</style>
