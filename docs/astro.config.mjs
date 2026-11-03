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
      customCss: ['./src/styles/guide.css'],
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
        {
          // Written by scripts/build-help.mjs from the guide inside the app.
          label: 'Guide',
          items: [
            { slug: 'guide' },
            { slug: 'guide/screens' },
            { slug: 'guide/how-it-works' },
            { slug: 'guide/tours' },
            { label: 'Settings', collapsed: true, items: [{ autogenerate: { directory: 'guide/settings' } }] },
          ],
        },
        { label: 'Security', items: [{ autogenerate: { directory: 'security' } }] },
      ],
    }),
  ],
});
