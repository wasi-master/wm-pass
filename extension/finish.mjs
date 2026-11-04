// Puts the rest beside the built pages, and derives the Firefox folder from the Chrome one.
import { cpSync, copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

process.chdir(fileURLToPath(new URL('.', import.meta.url)));
const manifest = JSON.parse(readFileSync('manifest.json', 'utf8'));
copyFileSync('../ui/src/wasm/wmpass_wasm_bg.wasm', 'dist/chrome/wmpass_wasm_bg.wasm');
for (const size of [16, 32, 48, 128]) copyFileSync(`icons/icon-${size}.png`, `dist/chrome/icon-${size}.png`);
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
// Firefox has no externally_connectable: the web vault finds no extension there and runs on its own.
delete firefox.externally_connectable;
// Nothing goes to the developer or anyone else: the vault leaves only encrypted, for places the
// owner sets up. Mozilla asks every new add-on to say so, which Firefox reads from 140 (142 on
// Android).
firefox.browser_specific_settings = {
  gecko: { id: 'wmpass@wasimaster.com', strict_min_version: '140.0', data_collection_permissions: { required: ['none'] } },
  gecko_android: { strict_min_version: '142.0' },
};
writeFileSync('dist/firefox/manifest.json', JSON.stringify(firefox, null, 2));
