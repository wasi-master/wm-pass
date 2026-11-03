<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Qr from '../parts/Qr.svelte';
  import Row from '../parts/Row.svelte';
  import { overlays } from '../parts/floating.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import type { LinkedView } from '../engine/types';
  import { ui } from '../vault/ui.svelte';
  import { copyPlainLine } from './clipboard';
  import PlaceDialog from './PlaceDialog.svelte';
  import { places, placeProblem, type Place } from './places.svelte';

  type Sheet = 'nested' | 'shared' | 'join' | 'place' | 'accept' | 'people';
  let links = $state.raw<LinkedView[]>([]);
  let tick = $state(0);
  let sheet = $state<Sheet | null>(null);
  let chosen = $state<LinkedView | null>(null);
  let opening = $state<string | null>(null);
  let problem = $state<string | null>(null);
  let sheetProblem = $state<string | null>(null);
  let name = $state('');
  let myName = $state('');
  let password = $state('');
  let place = $state<Place | null>(null);
  let addingPlace = $state(false);
  let busy = $state(false);
  let joinCode = $state<string | null>(null);
  let fingerprint = $state('');
  let inviteText = $state('');
  let members = $state.raw<{ key: string; name: string; fingerprint: string; owner: boolean }[]>([]);
  let code = $state('');
  let reading = $state<{ code: string; name: string; fingerprint: string } | null>(null);
  let invite = $state<string | null>(null);
  let peopleTick = $state(0);

  $effect(() => {
    void tick;
    void pass.child;
    void pass.view<LinkedView[]>('linked.list').then((list) => (links = list)).catch(() => (links = []));
  });

  function openSheet(next: Sheet, link: LinkedView | null = null) {
    sheet = next;
    chosen = link;
    name = '';
    myName = '';
    password = '';
    place = null;
    busy = false;
    sheetProblem = null;
    joinCode = null;
    fingerprint = '';
    inviteText = '';
    members = [];
    code = '';
    reading = null;
    invite = null;
  }

  async function open(link: LinkedView) {
    opening = link.file;
    problem = null;
    try {
      if (link.shared) {
        const where = places.placeOf(link.file);
        if (!where) {
          openSheet('place', link);
        } else {
          await places.openShared(where, link.file);
          ui.go('all');
        }
      } else {
        const sealed = await pass.readLinked(link.file);
        const reply = await pass.engine('linked.open', { file: link.file }, { handle: pass.handle, blobs: [sealed] });
        await pass.enterChild(link.file, reply, (bytes) => pass.writeLinked(link.file, bytes));
        ui.go('all');
      }
    } catch (failure) {
      problem = placeProblem(failure);
    }
    opening = null;
  }

  async function makeNested() {
    const secret = bytesOf(password);
    busy = true;
    try {
      const reply = await pass.change('linked.create', { name: name.trim(), kdf: { m_kib: 64 * 1024 } }, { secret });
      await pass.writeLinked(reply.json.file, reply.blobs[0]);
      await pass.flush();
      tick++;
      sheet = null;
    } catch (failure: any) {
      sheetProblem = failure.message;
    } finally {
      secret.fill(0);
      busy = false;
    }
  }

  async function makeShared() {
    busy = true;
    try {
      await places.createShared(place!, name.trim(), myName.trim());
      tick++;
      sheet = null;
    } catch (failure) {
      sheetProblem = placeProblem(failure);
    }
    busy = false;
  }

  async function makeJoinCode() {
    const reply = await pass.change<{ code: string; fingerprint: string }>('shared.join_code', { my_name: myName.trim() });
    joinCode = reply.json.code;
    fingerprint = reply.json.fingerprint;
    tick++;
  }

  async function accept() {
    try {
      const reply = await pass.change<{ file: string }>('shared.accept', { file: chosen!.file, code: inviteText.trim() });
      places.setPlace(reply.json.file, place!);
      await pass.flush();
      tick++;
      sheet = null;
    } catch (failure: any) {
      sheetProblem = failure.message;
    }
  }

  // ---- people ----

  const peoplePlace = $derived(chosen ? places.placeOf(chosen.file) : null);

  $effect(() => {
    void peopleTick;
    if (sheet !== 'people' || !chosen) return;
    const file = chosen.file;
    const where = places.placeOf(file);
    if (!where) {
      sheetProblem = 'This computer does not know where the vault lives yet. Open it once first.';
      return;
    }
    places
      .manifest(where, file)
      .then((manifest) => pass.engine('shared.members', { file }, { handle: pass.handle, blobs: [manifest] }))
      .then((reply) => (members = reply.json))
      .catch((failure) => (sheetProblem = placeProblem(failure)));
  });

  async function readCode() {
    try {
      const reply = await pass.engine<{ name: string; fingerprint: string }>('shared.read_code', { code: code.trim() }, { handle: pass.handle });
      reading = { code: code.trim(), ...reply.json };
    } catch (failure: any) {
      sheetProblem = failure.message;
    }
  }

  async function addMember() {
    try {
      invite = await places.addMember(peoplePlace!, chosen!.file, reading!.code);
      reading = null;
      peopleTick++;
    } catch (failure) {
      sheetProblem = placeProblem(failure);
    }
  }

  async function removeMember(member: { key: string; name: string }) {
    const yes = await overlays.ask({
      title: `Remove ${member.name}?`,
      body: 'The vault moves to a new key, so they cannot open anything written from now on. Change any password they could see: they may have kept a copy.',
      action: 'Remove them',
      danger: true,
    });
    if (!yes) return;
    try {
      await places.removeMember(peoplePlace!, chosen!.file, member.key);
      peopleTick++;
    } catch (failure) {
      sheetProblem = placeProblem(failure);
    }
  }
</script>

{#snippet placePicker()}
  <p class="dim">Where it lives. Everyone sharing needs access to this place.</p>
  <div class="gap wrap">
    {#each places.list as option (option.id)}
      <button type="button" class="chip" aria-pressed={place?.id === option.id} onclick={() => (place = option)}>{option.label}</button>
    {/each}
    <button type="button" class="chip" onclick={() => (addingPlace = true)}>Add a place</button>
  </div>
{/snippet}

{#snippet codeCard(text: string, caption: string)}
  <Qr {text} />
  <p class="dim">{caption}</p>
  <div><Btn label="Copy it instead" icon="content_copy" onclick={() => copyPlainLine('Code', text)} /></div>
{/snippet}

{#if pass.child}
  <Group title="You are inside">
    <Row title={pass.child} sub="Back to the vault it lives in" icon="arrow_back" chevron onclick={() => pass.leaveChild()} />
  </Group>
{/if}

<p class="dim" style="padding-bottom:14px">A nested vault is its own encrypted file, opened from inside this one. A shared vault is held at a place you and others can reach, and each person has their own key to it.</p>

{#if links.length > 0}
  <Group title="Yours">
    {#each links as link (link.file)}
      <Row
        title={link.name}
        sub={link.pending ? 'Waiting for the owner’s invitation' : link.owner ? 'Shared, and yours to manage' : link.shared ? 'Shared with you' : 'Nested'}
        icon={link.shared ? 'family_restroom' : 'layers'}
        onclick={() => (link.pending ? openSheet('accept', link) : open(link))}
      >
        {#if opening === link.file}<span class="dim small">Opening…</span>{/if}
        {#if link.shared && !link.pending}
          <IconBtn icon="person" label="People" onclick={(event) => { event.stopPropagation(); openSheet('people', link); }} />
        {/if}
      </Row>
    {/each}
  </Group>
{/if}
{#if problem}<p class="danger-text" style="padding-bottom:12px">{problem}</p>{/if}

<Group title="Add">
  <Row title="A nested vault" help="action.nested_vault" sub="Its own encrypted file, kept apart from this one" icon="layers" chevron onclick={() => openSheet('nested')} />
  {#if places.available}
    <Row title="A shared vault" help="action.shared_vault" sub="For family or a team, without a server" icon="family_restroom" chevron onclick={() => openSheet('shared')} />
    <Row title="Join someone’s shared vault" help="action.join_shared_vault" sub="Make a code to send them" icon="add" chevron onclick={() => openSheet('join')} />
  {/if}
</Group>

{#if sheet === 'nested'}
  <Dialog title="A nested vault" onclose={() => (sheet = null)}>
    <div class="stack">
      <p class="dim">It opens from here without asking again, so anyone inside this vault can reach it. Its own password opens it only through a recovery, or if you later keep the file elsewhere.</p>
      <Field bind:value={name} placeholder="Its name, such as Work" autofocus />
      <PasswordField bind:value={password} placeholder="Its own password" />
      {#if sheetProblem}<p class="danger-text">{sheetProblem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label="Make it" variant="primary" disabled={busy || !name.trim() || !password} onclick={makeNested} />
    {/snippet}
  </Dialog>
{:else if sheet === 'shared'}
  <Dialog title="A shared vault" onclose={() => (sheet = null)}>
    <div class="stack">
      <Field bind:value={name} placeholder="Its name, such as Family" autofocus />
      <Field bind:value={myName} placeholder="Your name, as the others will see it" />
      {@render placePicker()}
      {#if sheetProblem}<p class="danger-text">{sheetProblem}</p>{/if}
      <p class="dimmer small">Everyone with the key can read and change what is in it. Removing someone moves it to a new key, but they keep any copy they already have.</p>
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label={busy ? 'Making it…' : 'Make it'} variant="primary" disabled={busy || !name.trim() || !myName.trim() || !place} onclick={makeShared} />
    {/snippet}
  </Dialog>
{:else if sheet === 'join'}
  <Dialog title="Join a shared vault" onclose={() => (sheet = null)}>
    <div class="stack">
      {#if joinCode}
        {@render codeCard(joinCode, 'Show or send this to the vault’s owner. It holds no secret.')}
        <p class="dim">Read this out to them, to check nobody swapped the code</p>
        <p class="mono">{fingerprint}</p>
        <p class="dimmer small">Their invitation comes back to this same list, under Waiting.</p>
      {:else}
        <Field bind:value={myName} placeholder="Your name, as the others will see it" autofocus />
      {/if}
    </div>
    {#snippet foot()}
      <Btn label="Close" onclick={() => (sheet = null)} />
      {#if !joinCode}<Btn label="Make my code" variant="primary" disabled={!myName.trim()} onclick={makeJoinCode} />{/if}
    {/snippet}
  </Dialog>
{:else if sheet === 'place' && chosen}
  <Dialog title="Where is {chosen.name}?" onclose={() => (sheet = null)}>
    <div class="stack">{@render placePicker()}</div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn
        label="Use this place"
        variant="primary"
        disabled={!place}
        onclick={() => {
          const link = chosen!;
          places.setPlace(link.file, place!);
          sheet = null;
          void open(link);
        }}
      />
    {/snippet}
  </Dialog>
{:else if sheet === 'accept' && chosen}
  <Dialog title="Your invitation" onclose={() => (sheet = null)}>
    <div class="stack">
      {@render placePicker()}
      <Field bind:value={inviteText} multiline rows={4} mono placeholder="Paste the invitation" />
      {#if sheetProblem}<p class="danger-text">{sheetProblem}</p>{/if}
    </div>
    {#snippet foot()}
      <Btn label="Cancel" onclick={() => (sheet = null)} />
      <Btn label="Accept" variant="primary" disabled={!place || !inviteText.trim()} onclick={accept} />
    {/snippet}
  </Dialog>
{:else if sheet === 'people' && chosen}
  <Dialog title={chosen.name} onclose={() => (sheet = null)}>
    <div class="stack">
      {#each members as member (member.key)}
        <div class="gap">
          <span class="grow"><span style="display:block">{member.name}</span><span class="mono dim small" style="display:block">{member.fingerprint}</span></span>
          {#if chosen.owner && !member.owner}<IconBtn icon="delete" label="Remove {member.name}" onclick={() => removeMember(member)} />{/if}
        </div>
      {/each}
      {#if sheetProblem}<p class="danger-text">{sheetProblem}</p>{/if}
      {#if invite}
        {@render codeCard(invite, 'They scan or paste this. Anyone can see it, but only the person you added can use it.')}
      {:else if chosen.owner && reading}
        <p style="font-weight:500">Is this {reading.name}?</p>
        <p class="dim">Ask them to read their code out. A different one means someone swapped it, and adding it would hand that person the vault.</p>
        <p class="mono">{reading.fingerprint}</p>
        <div class="gap">
          <Btn label="Not them" variant="danger" onclick={() => (reading = null)} />
          <Btn label="It matches" variant="primary" onclick={addMember} />
        </div>
      {:else if chosen.owner && peoplePlace}
        <p class="section-title">Add someone</p>
        <Field bind:value={code} multiline rows={3} mono placeholder="Paste their code" />
        <div><Btn label="Check it" disabled={!code.trim()} onclick={readCode} /></div>
      {/if}
    </div>
    {#snippet foot()}<Btn label="Close" onclick={() => (sheet = null)} />{/snippet}
  </Dialog>
{/if}

{#if addingPlace}
  <PlaceDialog onclose={() => (addingPlace = false)} onsaved={(saved) => (place = saved)} />
{/if}
