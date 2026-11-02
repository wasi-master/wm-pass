// Puts the rest beside the built pages, and derives the Firefox folder from the Chrome one.
import { cpSync, copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

process.chdir(fileURLToPath(new URL('.', import.meta.url)));
const manifest = JSON.parse(readFileSync('manifest.json', 'utf8'));
copyFileSync('../ui/src/wasm/wmpass_wasm_bg.wasm', 'dist/chrome/wmpass_wasm_bg.wasm');
for (const size of [16, 32, 48, 128]) {
  execFileSync('sips', ['-z', String(size), String(size), '../desktop/src-tauri/icons/128x128@2x.png', '--out', `dist/chrome/icon-${size}.png`], { stdio: 'ignore' });
}
writeFileSync('dist/chrome/manifest.json', JSON.stringify(manifest, null, 2));

// Firefox has no worker for this and no key field; its background is a page with its own id.
rmSync('dist/firefox', { recursive: true, force: true });
mkdirSync('dist/firefox', { recursive: true });
cpSync('dist/chrome', 'dist/firefox', { recursive: true });
rmSync('dist/firefox/offscreen.html', { force: true });
const firefox = structuredClone(manifest);
delete firefox.key;
delete firefox.minimum_chrome_version;
firefox.background = { scripts: ['background.js'], type: 'module' };
firefox.permissions = firefox.permissions.filter((p) => p !== 'offscreen');
firefox.browser_specific_settings = { gecko: { id: 'wmpass@wasimaster.com', strict_min_version: '128.0' } };
writeFileSync('dist/firefox/manifest.json', JSON.stringify(firefox, null, 2));
