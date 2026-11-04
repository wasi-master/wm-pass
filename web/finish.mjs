// Puts the icons beside the built page, and writes the service worker with this
// build's own file list, so the offline copy is exactly one release and nothing else.
import { createHash } from 'node:crypto';
import { cpSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

process.chdir(fileURLToPath(new URL('.', import.meta.url)));
// The app's own icons, for the hand-over page and the manifest.
mkdirSync('dist/icons', { recursive: true });
for (const size of [256, 512]) cpSync(`../assets/icon/wm-pass-${size}.png`, `dist/icons/wm-pass-${size}.png`);

const walk = (dir) => readdirSync(dir).flatMap((name) => (statSync(join(dir, name)).isDirectory() ? walk(join(dir, name)) : [join(dir, name)]));
// Site marks are many and small, so they are kept as they are first shown rather than all at once.
const files = walk('dist')
  .map((path) => relative('dist', path).split('\\').join('/'))
  .filter((path) => !path.startsWith('brands/') && path !== 'sw.js')
  .sort();
const hash = createHash('sha256');
for (const path of files) hash.update(path).update(readFileSync(join('dist', path)));
const version = hash.digest('hex').slice(0, 16);

writeFileSync(
  'dist/sw.js',
  `// The web vault's offline copy, written by web/finish.mjs for build ${version}.
const VERSION = 'wmpass-${version}';
const FILES = ${JSON.stringify(['./', ...files.filter((path) => path !== 'index.html').map((path) => `./${path}`)])};

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(VERSION).then((cache) => cache.addAll(FILES)));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(caches.keys().then((names) => Promise.all(names.filter((name) => name !== VERSION).map((name) => caches.delete(name)))));
});

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'GET' || url.origin !== location.origin || !url.pathname.startsWith(new URL(self.registration.scope).pathname)) return;
  // The page from the network whenever it can be had, so a release arrives at once.
  if (event.request.mode === 'navigate') {
    event.respondWith(fetch(event.request).catch(() => caches.match('./', { ignoreSearch: true }).then((hit) => hit ?? Response.error())));
    return;
  }
  // Everything else is named by its contents, so a copy kept is a copy that is still right.
  event.respondWith(
    caches.match(event.request).then(
      (hit) =>
        hit ??
        fetch(event.request).then((response) => {
          if (response.ok && url.pathname.includes('/brands/')) {
            const copy = response.clone();
            void caches.open(VERSION).then((cache) => cache.put(event.request, copy));
          }
          return response;
        }),
    ),
  );
});
`,
);
console.log(`web vault ${version}: ${files.length} files`);
