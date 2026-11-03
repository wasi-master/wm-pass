<script lang="ts">
  import Group from '../parts/Group.svelte';
  import Row from '../parts/Row.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { ui } from '../vault/ui.svelte';
  import { copyPlainLine } from './clipboard';
  import IconBtn from '../parts/IconBtn.svelte';
  import Toggle from './Toggle.svelte';

  const desktop = host().desktop!;
  let shown = $state<{ running: boolean; socket: string | null; last: string | null }>({ running: false, socket: null, last: null });
  const keys = $derived(pass.entries.filter((e) => e.kind === 'ssh'));

  async function refresh() {
    shown = await desktop.sshState();
  }

  $effect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => clearInterval(timer);
  });

  async function apply() {
    await desktop.sshSetEnabled(prefs.v.ssh_agent, prefs.v.ssh_confirm);
    await refresh();
  }

  const line = $derived(shown.socket ? (host().os === 'windows' ? `$env:SSH_AUTH_SOCK = "${shown.socket}"` : `export SSH_AUTH_SOCK="${shown.socket}"`) : '');
</script>

<p class="dim" style="padding-bottom:14px">While the vault is open, WM Pass can answer for the SSH keys in it, so <span class="mono">ssh</span> and <span class="mono">git</span> work with nothing on disk. The keys stop answering the moment the vault locks.</p>

<Group title="Agent">
  <Toggle title="Run the SSH agent" icon="terminal" pref="ssh_agent" onchange={apply} />
  <Toggle title="Ask before each use" sub="Recommended. A window asks each time a program wants a key to sign something." icon="rule" pref="ssh_confirm" onchange={apply} />
  {#if shown.running && line}
    <Row title="Point your shell at it" help="action.ssh_shell" sub={line}>
      <IconBtn icon="content_copy" label="Copy" onclick={() => copyPlainLine('Line', line)} />
    </Row>
  {/if}
  {#if shown.last}<Row title="Last used" sub={shown.last} />{/if}
</Group>

<Group title="Keys in this vault" help="group.ssh.keys">
  {#if keys.length === 0}
    <div class="set-body"><p class="dim">No SSH keys yet. Add one from the New item menu. Only ed25519 keys are generated here; others can be pasted in.</p></div>
  {/if}
  {#each keys as key (key.id)}
    <Row title={key.title} sub={key.subtitle} icon="vpn_key" chevron onclick={() => { ui.go('all'); ui.open(key.id); }} />
  {/each}
</Group>
