import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://wmpass.pages.dev',
  telemetry: false,
  integrations: [
    starlight({
      title: 'WM Pass',
      description: 'A password manager that keeps its vault on your own devices.',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/wasi-master/wm-pass' }],
      sidebar: [
        { label: 'Start', items: [{ autogenerate: { directory: 'start' } }] },
        { label: 'Apps', items: [{ autogenerate: { directory: 'apps' } }] },
        { label: 'Guides', items: [{ autogenerate: { directory: 'guides' } }] },
        { label: 'Security', items: [{ autogenerate: { directory: 'security' } }] },
      ],
    }),
  ],
});
