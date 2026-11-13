<script lang="ts">
  import Btn from '../../ui/src/parts/Btn.svelte';
  import PasswordField from '../../ui/src/parts/PasswordField.svelte';
  import { applyTheme } from '../../ui/src/theme/themes';
  import { prefs } from '../../ui/src/engine/prefs.svelte';
  import { send } from './host';
  import { toB64 } from './shared';

  const id = new URLSearchParams(location.search).get('id') ?? '';
  let ask = $state<any>(null);
  let choice = $state(0);
  let password = $state('');
  let wrong = $state(false);
  let needed = $state(false);
  let problem = $state<string | null>(null);
  let busy = $state(false);
  applyTheme(prefs.theme, matchMedia('(prefers-color-scheme: dark)').matches, false);

  // With nothing to answer, the window goes, and closing it answers no.
  void send({ type: 'confirm.get', id }).then(
    (found) => {
      ask = found;
      if (!found) window.close();
    },
    () => window.close(),
  );

  async function answer(picked: number) {
    if (busy) return;
    busy = true;
    wrong = false;
    needed = false;
    problem = null;
    const secret = password ? toB64(new TextEncoder().encode(password)) : null;
    try {
      const reply = await send({ type: 'confirm.answer', id, choice: picked, password: secret });
      password = '';
      if (reply?.wrong) wrong = true;
      else if (reply?.needed) needed = true;
      else window.close();
    } catch (failure: any) {
      // Such as the vault locking while this window was open: said here, not a window that stays put.
      if (picked < 0) window.close();
      else problem = failure?.message ?? 'That did not work.';
    } finally {
      busy = false;
    }
  }
  // Escape answers no, as closing the window does.
  function keys(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !ask || event.defaultPrevented) return;
    event.preventDefault();
    void answer(-1);
  }
</script>

<svelte:window onkeydown={keys} />

{#if ask}
  <main class="confirm">
    <h1>{ask.exists ? 'Passkey already saved' : ask.op === 'create' ? 'Save a passkey' : 'Sign in with a passkey'}</h1>
    <p class="dim">{ask.site} asks for this{ask.rp !== ask.site ? `, for ${ask.rp}` : ''}.</p>
    {#if ask.exists}
      <p>A passkey for <strong>{ask.user || 'this account'}</strong> is already in your vault, so no second one is made. Continue to tell the site.</p>
    {:else if ask.op === 'create'}
      <p>Save a passkey for <strong>{ask.user || 'this account'}</strong> in your vault.</p>
    {:else}
      <div class="choices" role="radiogroup" aria-label="Passkeys saved for this site">
        {#each ask.choices as item, i}
          <label class="choice">
            <input type="radio" name="key" checked={choice === i} onchange={() => (choice = i)} />
            <span><span class="title">{item.title}</span><span class="dim small">{item.user_name}</span></span>
          </label>
        {/each}
      </div>
    {/if}
    {#if ask.exists}
      {#if problem}<p class="small danger-text" role="alert">{problem}</p>{/if}
    {:else}
      <div>
      <p class="small dim">{ask.verify ? 'This site requires your master password, to prove it is you.' : 'Enter your password to prove it is you. Some sites require this.'}</p>
      <!-- Enter in the box answers as the main button does, so the window opens with it ready to type in. -->
      <PasswordField bind:value={password} placeholder={ask.verify ? 'Master password' : 'Master password (optional)'} error={wrong || needed} describedby={wrong || needed || problem ? 'confirm-problem' : undefined} autofocus onenter={() => answer(choice)} />
      <div id="confirm-problem">
        {#if wrong}<p class="small danger-text" role="alert">That password is not right.</p>{/if}
        {#if needed}<p class="small danger-text" role="alert">Enter your master password first.</p>{/if}
        {#if problem}<p class="small danger-text" role="alert">{problem}</p>{/if}
      </div>
      </div>
    {/if}
    <div class="gap">
      <Btn label="Cancel" onclick={() => answer(-1)} />
      <Btn label={ask.exists ? 'Continue' : ask.op === 'create' ? 'Save passkey' : 'Sign in'} variant="primary" disabled={busy} onclick={() => answer(choice)} />
    </div>
  </main>
{/if}

<style>
  .confirm { padding: 22px; display: grid; gap: 14px; }
  h1 { font-size: var(--fs-xl); margin: 0; font-weight: 600; }
  p { margin: 0; }
  .choices { display: grid; gap: 4px; }
  .choice { display: flex; gap: 10px; align-items: center; padding: 6px; border-radius: var(--r-m); }
  .choice:hover { background: var(--s1); }
  .choice span { display: flex; flex-direction: column; }
  .title { font-weight: 600; }
  .gap { display: flex; gap: 8px; justify-content: flex-end; }
</style>
