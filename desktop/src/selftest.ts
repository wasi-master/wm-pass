// Runs the real flows in the real webview, against a throwaway folder, when a debug build is made
// with VITE_SELFTEST set. Release builds do not contain this file.

import { invoke } from '@tauri-apps/api/core';
import { host } from '$ui/engine/host';
import { bytesOf, pass } from '$ui/engine/pass.svelte';
import { prefs } from '$ui/engine/prefs.svelte';
import { places, blankPlace } from '$ui/settings/places.svelte';
import { ui } from '$ui/vault/ui.svelte';
import { lan } from '$ui/settings/lan.svelte';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function run() {
  const out: Record<string, unknown> = {};
  const step = async (name: string, body: () => Promise<unknown>) => {
    try {
      out[`~${name}`] = 'started';
      await host().saveSettings('selftest', JSON.stringify(out, null, 1));
      const timeout = new Promise((_, reject) => setTimeout(() => reject(new Error('TIMEOUT')), 25_000));
      out[name] = (await Promise.race([body(), timeout])) ?? 'ok';
      delete out[`~${name}`];
    } catch (failure: any) {
      out[name] = `FAILED: ${failure?.message ?? failure}`;
    }
    await host().saveSettings('selftest', JSON.stringify(out, null, 1));
  };
  await sleep(1500);
  const folder = import.meta.env.VITE_SELFTEST as string;
  const password = bytesOf('selftest correct horse battery staple');
  await step('os', async () => `${host().os} ${(await host().memoryGib()).toFixed(1)} GiB`);
  await step('create', async () => {
    await pass.create('Personal', password.slice());
    return pass.state.kind;
  });
  let id = '';
  await step('add', async () => {
    id = (await pass.change('entry.create', { kind: 'login', title: 'GitHub', fields: { username: 'sample@example.com' }, sites: [{ Url: 'https://github.com' }] })).json.id;
    await pass.change('entry.set_secret', { id, field: 'password' }, { secret: bytesOf('hT7#kQ2!mZpL9$xV') });
    return pass.entries.length;
  });
  await step('attachment', async () => {
    await pass.addAttachment(id, 'a.txt', 'text/plain', bytesOf('attached text'));
    const detail = await pass.detail(id);
    const back = await pass.openAttachment(id, detail.attachment_list[0].blob);
    return new TextDecoder().decode(back);
  });
  await step('ssh key', async () => {
    const key = (await pass.change('entry.create', { kind: 'ssh', title: 'Deploy key' })).json.id;
    const made = (await pass.change('ssh.generate', { id: key, comment: 'selftest' })).json;
    await pass.change('entry.set', { id: key, fields: { agent_allowed: true } });
    return made.fingerprint;
  });
  await step('files on disk', async () => (await host().listFiles('vaults')).length);
  await step('secret to clipboard', async () => {
    await pass.copySecret('entry.reveal', { id, field: 'password' }, 2);
    const during = await invoke<string | null>('test_clipboard');
    await sleep(2800);
    const after = await invoke<string | null>('test_clipboard');
    await host().copyText('copied by the person meanwhile', true, 1);
    await host().copyText('something else', false, 0);
    await sleep(1500);
    const kept = await invoke<string | null>('test_clipboard');
    return { during_is_the_password: during === 'hT7#kQ2!mZpL9$xV', cleared_after: !after, later_copy_kept: kept === 'something else' };
  });
  await step('lock', async () => {
    await pass.flush();
    await pass.lock();
    return pass.state.kind;
  });
  await step('wrong password', async () => (await pass.unlock(bytesOf('nope'))).kind);
  await step('unlock', async () => {
    const result = (await pass.unlock(password.slice())).kind;
    return `${result}, ${pass.entries.length} items`;
  });
  await step('sealed round trip', async () => {
    await host().saveSealed!('t', 'hello sealed');
    return host().loadSealed!('t');
  });
  await step('path escape refused', async () => {
    try {
      await host().readFile('../settings/prefs.json');
      return 'WRONGLY ALLOWED';
    } catch {
      return 'refused';
    }
  });
  await step('outside file refused', async () => {
    try {
      await host().outside!.read('/etc/hosts');
      return 'WRONGLY ALLOWED';
    } catch {
      return 'refused';
    }
  });
  await step('open_url refuses file:', async () => {
    try {
      await host().openUrl('file:///etc/hosts');
      return 'WRONGLY ALLOWED';
    } catch {
      return 'refused';
    }
  });
  await step('backup to folder', async () => {
    const place = { ...blankPlace('folder'), label: 'Test', path: folder };
    await places.put(place);
    const problem = await places.backupTo(place);
    const copies = await places.copies(place);
    return `${problem ?? 'fine'}, ${copies.length} copy`;
  });
  await step('file sync', async () => {
    const place = places.list[0];
    await places.put({ ...place, sync: true });
    await places.syncNow();
    return places.syncProblem ?? 'fine';
  });
  await step('bring back', async () => {
    const place = places.list[0];
    const copies = await places.copies(place);
    return pass.mergeIn(await places.read(place, copies[0]));
  });
  await step('network off refuses', async () => {
    try {
      await pass.engine('breach.password', {}, { secret: bytesOf('x'), online: false });
      return 'WRONGLY ALLOWED';
    } catch (failure: any) {
      return failure.message;
    }
  });
  await step('lan identity', async () => {
    const address = await host().lan!.localAddress();
    const name = await host().lan!.deviceName();
    await lan.withIdentity(async (identity) => identity.length);
    return `${name ? 'named' : 'unnamed'}, ${address ? 'has address' : 'no address'}`;
  });
  await step('qr', async () => (await pass.engine('qr.encode', {}, { secret: bytesOf('hello') })).json.size);
  await step('ssh agent', async () => {
    await host().desktop!.sshSetEnabled(true, false);
    await sleep(300);
    return host().desktop!.sshState();
  });
  await step('app locks by itself', async () => {
    const desktop = host().desktop!;
    await desktop.setLockRule(1, false);
    const before = await desktop.currentHandle();
    await sleep(3500);
    const after = await desktop.currentHandle();
    await sleep(400);
    const page = pass.state.kind;
    await desktop.setLockRule(300, true);
    const again = (await pass.unlock(password.slice())).kind;
    return { open_before: before !== 0, closed_after: after === 0, page_followed: page === 'locked', reopened: again };
  });
  await step('autostart readable', async () => host().desktop!.autostart());
  await step('version', async () => host().desktop!.version());
  await step('settings file keeps protection out', async () => {
    const text = prefs.exportText();
    return ['lock_after', 'screen_protection', 'network_allowed'].every((k) => !text.includes(k));
  });
  // Pictures need the window uncovered.
  await invoke('set_screen_protection', { on: false });
  ui.go('all');
  ui.open(id);
  await step('done', async () => 'waiting');
  await sleep(15_000);
  await pass.lock();
  await step('agent after lock', async () => 'locked');
  await sleep(6000);
  await invoke('test_exit');
}
