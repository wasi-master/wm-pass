<script lang="ts">
  import HelpButton from '../help/HelpButton.svelte';
  import Hint from '../help/Hint.svelte';
  import Btn from '../parts/Btn.svelte';
  import Ring from '../parts/Ring.svelte';
  import Tile from '../parts/Tile.svelte';
  import { consumeText, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { everySecond } from '../engine/clock';
  import { byName, grouped } from '../engine/text';
  import type { OtpRow } from '../engine/types';
  import { overlays } from '../parts/floating.svelte';
  import { ui } from './ui.svelte';

  let rows = $state<OtpRow[] | null>(null);
  let hotp = $state<Record<string, string>>({});
  let shownUntil = $state<Record<string, number>>({});
  let now = $state(Date.now());

  $effect(() => {
    void pass.revision;
    return everySecond(async () => {
      try {
        rows = await pass.codes();
      } catch {
        rows = [];
      }
      now = Date.now();
    });
  });

  const list = $derived(rows && prefs.v.code_sort === 'name' ? [...rows].sort((a, b) => byName(name(a), name(b))) : rows);

  // The item's own name first: it is the one the person typed, and renaming the item leaves the
  // issuer saved in its setup as it was, so the page would keep showing the old name.
  function name(row: OtpRow) {
    return row.title || row.otp.issuer || 'Code';
  }

  function clicked(row: OtpRow) {
    const code = row.code ?? hotp[row.id];
    if (!code) return;
    shownUntil = { ...shownUntil, [row.id]: Date.now() + 15_000 };
    // A counter-based code on show is the one copied: asking again would move the counter on and
    // copy a code other than the one on screen, as the item page does too.
    if (row.code) void ui.copyCode(row.id);
    else void ui.copyShownCode(code);
  }

  function keyed(event: KeyboardEvent, row: OtpRow) {
    if (event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' ')) return;
    event.preventDefault();
    clicked(row);
  }

  async function nextHotp(event: MouseEvent, row: OtpRow) {
    event.stopPropagation();
    try {
      const reply = await pass.change('otp.hotp_next', { id: row.id });
      hotp = { ...hotp, [row.id]: consumeText(reply.secret) };
    } catch (failure: any) {
      overlays.toast(failure?.message ?? 'A new code could not be made.');
    }
  }
</script>

<div class="settings-pane scroll">
  <div class="settings-inner" style="max-width:none">
    <div class="page-title-row" style="padding-bottom:10px">
      <h1 class="page-title">Codes</h1>
      <HelpButton id="screen.codes" title="Codes" />
      <span class="grow"></span>
      {#if list && list.length > 0}<span data-help-anchor="codes.add" style="display:flex"><Btn label="Add a code" icon="add" variant="quiet" onclick={() => ui.add('otp')} /></span>{/if}
    </div>
    <Hint id="codes" topic="screen.codes" tour="tour.codes" />
    {#if list && list.length === 0}
      <div class="stack" style="max-width:420px">
        <p class="dim">No codes yet. When a site offers two-factor sign-in, enter its setup key here.</p>
        <div class="gap wrap" data-help-anchor="codes.add">
          <Btn label="Add a code" variant="primary" onclick={() => ui.add('otp')} />
          <Btn label="Bring them from another app" onclick={() => ui.go('settings/import')} />
        </div>
      </div>
    {:else if list}
      <div class="codes-grid" data-help-anchor="codes.list">
        {#each list as row (row.id)}
          {@const code = row.code ?? hotp[row.id]}
          {@const hidden = prefs.v.code_hidden && (shownUntil[row.id] ?? 0) < now}
          <!-- A counter-based code not yet shown has nothing to copy: the row is no button then, so
               its own Show the next code button is not a button inside a button. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (the tabindex is there only as a button) -->
          <div
            class="code-row"
            role={code ? 'button' : 'group'}
            tabindex={code ? 0 : undefined}
            aria-label={code ? `${name(row)}${row.otp.account ? `, ${row.otp.account}` : ''}. ${hidden ? 'Code hidden' : `Code ${code.split('').join(' ')}`}. Copy` : name(row)}
            title={code ? 'Click to copy. Double-click to open the item.' : 'Double-click to open the item.'}
            onclick={() => clicked(row)}
            onkeydown={(e) => keyed(e, row)}
            ondblclick={() => ui.open(row.id)}
          >
            <Tile title={name(row)} site={row.site} kind="otp" icon={row.icon} size={36} />
            <span class="grow">
              <span class="ell" style="display:block;font-weight:500">{name(row)}</span>
              {#if row.otp.account}<span class="dim small ell" style="display:block">{row.otp.account}</span>{/if}
              {#if code && hidden}
                <span class="code-value dimmer {prefs.v.code_size}">••• •••</span>
              {:else if code}
                <span class="code-value {prefs.v.code_size}">{prefs.v.code_groups ? grouped(code) : code}</span>
              {:else}
                <Btn label="Show the next code" variant="quiet" onclick={(e) => nextHotp(e, row)} />
              {/if}
              {#if row.next_code && prefs.v.code_next && !hidden}
                <span class="dim small mono" style="display:block">Next {grouped(row.next_code)}</span>
              {/if}
            </span>
            {#if row.otp.period && row.seconds_left != null && prefs.v.code_ring}
              <Ring period={row.otp.period} left={row.seconds_left} size={24} />
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
