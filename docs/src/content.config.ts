import { defineCollection } from 'astro:content';
import { z } from 'astro/zod';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

/** The app a guide under guides/import is about, for its logo and the list on the import page. */
const source = z.object({
  name: z.string(),
  kind: z.enum(['manager', 'authenticator']),
  /** Simple Icons slug in data/brands, when it has the app. */
  logo: z.string().optional(),
  /** Hex colour without the #, for apps without a logo. */
  color: z.string().optional(),
  /** Where the app can export, in the order the guide's tabs show them. */
  exportOn: z.array(z.string()),
  /** The file to make, as the guide says it. */
  choose: z.string(),
});

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema({ extend: z.object({ source: source.optional() }) }) }),
};
