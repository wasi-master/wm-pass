import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

export default defineConfig({
  root: here('.'),
  publicDir: here('../ui/public'),
  plugins: [svelte()],
  resolve: { alias: { $ui: here('../ui/src') } },
  server: { port: 5184, strictPort: true, fs: { allow: [here('..')] } },
  build: {
    outDir: here('dist'),
    emptyOutDir: true,
    target: 'es2022',
    rollupOptions: { input: { index: here('index.html'), palette: here('palette.html') } },
  },
});
