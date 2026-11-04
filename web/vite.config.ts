import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

// The web vault, served from wmpass.pages.dev/vault/. The docs site's build puts it there.
export default defineConfig({
  root: here('.'),
  base: '/vault/',
  publicDir: here('../ui/public'),
  plugins: [svelte()],
  resolve: { alias: { $ui: here('../ui/src'), $help: here('../help') } },
  server: { port: 5184, strictPort: true, fs: { allow: [here('..')] } },
  build: {
    outDir: here('dist'),
    emptyOutDir: true,
    target: 'es2022',
    assetsInlineLimit: 0,
    rollupOptions: { input: { index: here('index.html'), oauth: here('oauth.html') } },
  },
});
