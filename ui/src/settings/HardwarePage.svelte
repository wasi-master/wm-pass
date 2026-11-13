<script lang="ts">
  import Btn from '../parts/Btn.svelte';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Icon from '../parts/Icon.svelte';
  import PasswordField from '../parts/PasswordField.svelte';
  import Row from '../parts/Row.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { host } from '../engine/host';
  import { ui } from '../vault/ui.svelte';
  import { passwordsDiffer } from '../engine/text';

  // Adds a hardware key as a second lock: from now on, this password opens the vault only with it.
  let step = $state<'choose' | 'details' | 'tap' | 'done'>('choose');
  let fido = $state(true);
  let label = $state('');
  let password = $state('');
  // Mistyped once, the password would open nothing even with the key.
  let again = $state('');
  let problem = $state<string | null>(null);
  let againTried = $state(false);
  const mismatch = $derived(passwordsDiffer(again, password, againTried));

  const kinds = host().hardware?.kinds ?? ['fido2', 'yubikey'];
  const fidoSub =
    host().kind !== 'desktop'
      ? 'YubiKey 5, SoloKey, Nitrokey and others. The browser asks for it, and it works in every WM Pass app afterwards.'
      : host().os === 'windows'
        ? 'Windows lets only administrators reach these directly. Run WM Pass as administrator, or use a YubiKey below.'
        : 'YubiKey 5, SoloKey, Nitrokey and others, by USB';

  async function tap() {
    step = 'tap';
    problem = null;
    try {
      const { factor, output } = await host().hardware!.tap(fido ? 'fido2' : 'yubikey', null);
      const secret = bytesOf(password);
      try {
        await pass.change('slot.add', { label: label.trim() || (fido ? 'Security key' : 'YubiKey'), action: 'open', factor }, { secret, blobs: [output] });
      } finally {
        secret.fill(0);
        output.fill(0);
      }
      await pass.flush();
      password = '';
      again = '';
      againTried = false;
      step = 'done';
    } catch (failure: any) {
      problem = failure?.message ?? 'The key could not be added.';
    }
  }
</script>

<div class="stack" style="max-width:520px">
  {#if step === 'choose'}
    <p class="dim">A hardware key adds something you hold to something you know. Someone who learns the password still gets nowhere without the key. Keep a second way in, in case you lose it.</p>
    <Group title="What kind of key">
      {#if kinds.includes('fido2')}
        <Row title="A FIDO2 security key" help="action.hardware_fido2" sub={fidoSub} icon="security_key" chevron onclick={() => { fido = true; step = 'details'; }} />
      {/if}
      {#if kinds.includes('yubikey')}
        <Row title="A YubiKey with challenge-response" help="action.hardware_yubikey" sub="Uses slot 2, set up beforehand in Yubico’s own tool" icon="key" chevron onclick={() => { fido = false; step = 'details'; }} />
      {/if}
    </Group>
  {:else if step === 'details'}
    <p class="dim">Choose the password that goes with this key. It can be a new one, used only with the key.</p>
    <Field bind:value={label} placeholder="A name for the key" autofocus />
    <PasswordField bind:value={password} placeholder="Password to use with it" />
    <PasswordField bind:value={again} placeholder="The same password again" error={mismatch} describedby={mismatch ? "mismatch-note" : undefined} onenter={() => { againTried = true; if (password && password === again) void tap(); }} />
    {#if mismatch}<p class="danger-text small" role="alert" id="mismatch-note">Passwords do not match.</p>{/if}
    <div class="gap">
      <Btn label="Back" onclick={() => (step = 'choose')} />
      <Btn label="Next" variant="primary" disabled={!password || password !== again} onclick={tap} />
    </div>
  {:else if step === 'tap'}
    <span class="dim" style="display:flex"><Icon name="security_key" size={40} /></span>
    <p style="font-size:var(--fs-l);font-weight:600">Plug in your key and touch it</p>
    <p class="dim">Keep it there until this page moves on. A key with a PIN set may refuse, since this app does not ask for one.</p>
    {#if problem}
      <p class="danger-text" role="alert">{problem}</p>
      <div class="gap">
        <Btn label="Back" onclick={() => (step = 'details')} />
        <Btn label="Try again" variant="primary" onclick={tap} />
      </div>
    {/if}
  {:else}
    <span class="ok-text" style="display:flex"><Icon name="check_circle" size={40} /></span>
    <p style="font-size:var(--fs-l);font-weight:600">The key is added</p>
    <p class="dim">Next time, type this password and touch the key when asked.</p>
    <div><Btn label="Done" variant="primary" onclick={() => ui.go('settings/security')} /></div>
  {/if}
</div>
