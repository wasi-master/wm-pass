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
  applyTheme(prefs.theme, matchMedia('(prefers-color-scheme: dark)').matches, false);

  void send({ type: 'confirm.get', id }).then((found) => {
    ask = found;
    if (!found) window.close();
  });

  async function answer(picked: number) {
    wrong = false;
    const secret = password ? toB64(new TextEncoder().encode(password)) : null;
    const reply = await send({ type: 'confirm.answer', id, choice: picked, password: secret });
    password = '';
    if (reply?.wrong) wrong = true;
    else window.close();
  }
</script>

{#if ask}
  <div class="confirm">
    <h1>{ask.op === 'create' ? 'Save a passkey' : 'Sign in with a passkey'}</h1>
    <p class="dim">{ask.site} asks for this{ask.rp !== ask.site ? `, for ${ask.rp}` : ''}.</p>
    {#if ask.op === 'create'}
      <p>Save a passkey for <strong>{ask.user || 'this account'}</strong> in your vault.</p>
    {:else}
      <div class="choices">
        {#each ask.choices as item, i}
          <label class="choice">
            <input type="radio" name="key" checked={choice === i} onchange={() => (choice = i)} />
            <span><span class="title">{item.title}</span><span class="dim small">{item.user_name}</span></span>
          </label>
        {/each}
      </div>
    {/if}
    <div>
      <p class="small dim">Enter your password to prove it is you. Some sites require this.</p>
      <PasswordField bind:value={password} placeholder="Master password (optional)" />
      {#if wrong}<p class="small danger-text">That password is not right.</p>{/if}
    </div>
    <div class="gap">
      <Btn label="Cancel" onclick={() => answer(-1)} />
      <Btn label={ask.op === 'create' ? 'Save passkey' : 'Sign in'} variant="primary" onclick={() => answer(choice)} />
    </div>
  </div>
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
