import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://wmpass.pages.dev',
  telemetry: false,
  integrations: [
    starlight({
      title: 'WM Pass',
      description: 'A password manager that keeps its vault on your own devices.',
      components: { PageTitle: './src/components/PageTitle.astro' },
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/wasi-master/wm-pass' }],
      sidebar: [
        { label: 'Start', items: [{ autogenerate: { directory: 'start' } }] },
        { label: 'Apps', items: [{ autogenerate: { directory: 'apps' } }] },
        {
          label: 'Guides',
          items: [
            'guides/import-export',
            { label: 'Leaving another app', collapsed: true, items: [{ autogenerate: { directory: 'guides/import' } }] },
            'guides/backup-sync',
            'guides/duress',
            'guides/emergency-access',
          ],
        },
        { label: 'Security', items: [{ autogenerate: { directory: 'security' } }] },
      ],
    }),
  ],
});
