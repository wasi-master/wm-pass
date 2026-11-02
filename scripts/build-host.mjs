// Builds the relay program the browser starts and puts it where Tauri packs it beside the app.
// Tauri sets these two variables for its before-build command.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

process.chdir(fileURLToPath(new URL('..', import.meta.url)));

const triple =
  process.env.TAURI_ENV_TARGET_TRIPLE ||
  /host: (.+)/.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))[1];
const debug = process.env.TAURI_ENV_DEBUG === 'true';

const profile = debug ? 'debug' : 'release';
const build = (target) => {
  const args = ['build', '-p', 'wmpass-host', '--target', target];
  if (!debug) args.push('--release');
  execFileSync('cargo', args, { stdio: 'inherit' });
};

mkdirSync('desktop/src-tauri/binaries', { recursive: true });
if (triple === 'universal-apple-darwin') {
  // A universal app needs a universal relay.
  const parts = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
  parts.forEach(build);
  const inputs = parts.map((part) => `target/${part}/${profile}/wm-pass-host`);
  execFileSync('lipo', ['-create', '-output', `desktop/src-tauri/binaries/wm-pass-host-${triple}`, ...inputs], { stdio: 'inherit' });
} else {
  build(triple);
  const ext = triple.includes('windows') ? '.exe' : '';
  copyFileSync(`target/${triple}/${profile}/wm-pass-host${ext}`, `desktop/src-tauri/binaries/wm-pass-host-${triple}${ext}`);
}
