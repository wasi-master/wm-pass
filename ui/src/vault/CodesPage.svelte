<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Ring from '../parts/Ring.svelte';
  import Tile from '../parts/Tile.svelte';
  import { consumeText, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { grouped } from '../engine/text';
  import type { OtpRow } from '../engine/types';
  import { ui } from './ui.svelte';

  let rows = $state<OtpRow[] | null>(null);
  let hotp = $state<Record<string, string>>({});
  let shownUntil = $state<Record<string, number>>({});
  let now = $state(Date.now());

  $effect(() => {
    void pass.revision;
    let stop = false;
    const tick = async () => {
      if (stop) return;
      try {
        rows = await pass.codes();
      } catch {
        rows = [];
      }
      now = Date.now();
      setTimeout(tick, 1000 - (Date.now() % 1000) + 10);
    };
    void tick();
    return () => {
      stop = true;
    };
  });

  const list = $derived(
    rows && prefs.v.code_sort === 'name'
      ? [...rows].sort((a, b) => (a.otp.issuer || a.title).toLowerCase().localeCompare((b.otp.issuer || b.title).toLowerCase()))
      : rows,
  );

  function name(row: OtpRow) {
    return row.otp.issuer || row.title || 'Code';
  }

  function clicked(row: OtpRow) {
    const code = row.code ?? hotp[row.id];
    if (!code) return;
    shownUntil = { ...shownUntil, [row.id]: Date.now() + 15_000 };
    void ui.copyCode(row.id);
  }

  async function nextHotp(event: MouseEvent, row: OtpRow) {
    event.stopPropagation();
    const reply = await pass.change('otp.hotp_next', { id: row.id });
    hotp = { ...hotp, [row.id]: consumeText(reply.secret) };
  }
</script>

<div class="settings-pane scroll">
  <div class="settings-inner" style="max-width:none">
    <h1 class="page-title">Codes</h1>
    {#if list && list.length === 0}
      <div class="stack" style="max-width:420px">
        <p class="dim">No codes yet. When a site offers two-factor sign-in, enter its setup key here.</p>
        <div class="gap">
          <Btn label="Add a code" variant="primary" onclick={() => ui.add('otp')} />
          <Btn label="Bring them from another app" onclick={() => ui.go('settings/import')} />
        </div>
      </div>
    {:else if list}
      <div class="codes-grid">
        {#each list as row (row.id)}
          {@const code = row.code ?? hotp[row.id]}
          {@const hidden = prefs.v.code_hidden && (shownUntil[row.id] ?? 0) < now}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            class="code-row"
            role="button"
            tabindex="0"
            title="Click to copy. Double-click to open the item."
            onclick={() => clicked(row)}
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
