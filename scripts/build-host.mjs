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

const args = ['build', '-p', 'wmpass-host', '--target', triple];
if (!debug) args.push('--release');
execFileSync('cargo', args, { stdio: 'inherit' });

const ext = triple.includes('windows') ? '.exe' : '';
mkdirSync('desktop/src-tauri/binaries', { recursive: true });
copyFileSync(
  `target/${triple}/${debug ? 'debug' : 'release'}/wm-pass-host${ext}`,
  `desktop/src-tauri/binaries/wm-pass-host-${triple}${ext}`,
);
