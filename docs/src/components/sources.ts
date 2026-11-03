import { getCollection } from 'astro:content';

/** Every guide under guides/import, by name. */
export async function sources(kind?: 'manager' | 'authenticator') {
  const entries = await getCollection('docs', (entry) => entry.id.startsWith('guides/import/') && entry.data.source !== undefined);
  return entries
    .map((entry) => ({ href: `/${entry.id}/`, ...entry.data.source! }))
    .filter((source) => kind === undefined || source.kind === kind)
    .sort((a, b) => a.name.localeCompare(b.name, 'en', { numeric: true }));
}
