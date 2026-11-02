import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

export default defineConfig({
  root: here('.'),
  publicDir: here('../public'),
  plugins: [svelte()],
  resolve: { alias: { $ui: here('../src') } },
  server: { port: 5183, strictPort: true, fs: { allow: [here('../..')] } },
});
