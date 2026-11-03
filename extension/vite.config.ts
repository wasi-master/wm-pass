import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

// Three builds, because the parts load differently: pages as modules, the background as one module,
// and the two content scripts as single classic files. VITE_PART picks one.
const part = process.env.VITE_PART ?? 'pages';

const content = (name: string) => ({
  emptyOutDir: false,
  outDir: here('dist/chrome'),
  target: 'es2022',
  rollupOptions: { input: here(`src/${name}.ts`), output: { format: 'iife' as const, entryFileNames: `${name}.js`, codeSplitting: false } },
});

const builds: Record<string, object> = {
  pages: {
    outDir: here('dist/chrome'),
    emptyOutDir: true,
    target: 'es2022',
    rollupOptions: { input: { vault: here('vault.html'), popup: here('popup.html'), confirm: here('confirm.html'), offscreen: here('offscreen.html') } },
  },
  background: {
    outDir: here('dist/chrome'),
    emptyOutDir: false,
    target: 'es2022',
    assetsInlineLimit: 0,
    rollupOptions: { input: here('src/background.ts'), output: { format: 'es' as const, entryFileNames: 'background.js', assetFileNames: 'wmpass_wasm_bg.wasm', codeSplitting: false } },
  },
  content: content('content'),
  'page-world': content('page-world'),
};

export default defineConfig({
  root: here('.'),
  publicDir: part === 'pages' ? here('../ui/public') : false,
  plugins: [svelte()],
  resolve: { alias: { $ui: here('../ui/src'), $help: here('../help') } },
  build: builds[part],
});
