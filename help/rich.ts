// Turns the guide's rich text into blocks and runs. The app draws them, the docs write them as
// Markdown, and the phone gets them already split, so nothing ever becomes HTML from a string.

import type { Rich } from './schema.ts';

export type Run =
  | { readonly t: 'text'; readonly s: string }
  | { readonly t: 'strong'; readonly s: string }
  | { readonly t: 'em'; readonly s: string }
  | { readonly t: 'link'; readonly id: string; readonly s?: string }
  | { readonly t: 'key'; readonly action: string };

export type Block = { readonly t: 'p'; readonly runs: readonly Run[] } | { readonly t: 'ul'; readonly items: readonly (readonly Run[])[] };

const TOKEN = /\[\[([a-z0-9_.:-]+)(?:\|([^\]]+))?\]\]|\{\{key:([a-z_]+)\}\}|\*\*([^*]+)\*\*|\*([^*]+)\*/g;

export function runs(text: string): Run[] {
  const out: Run[] = [];
  let at = 0;
  for (const match of text.matchAll(TOKEN)) {
    const start = match.index ?? 0;
    if (start > at) out.push({ t: 'text', s: text.slice(at, start) });
    const [, link, words, key, strong, em] = match;
    if (link) out.push(words ? { t: 'link', id: link, s: words } : { t: 'link', id: link });
    else if (key) out.push({ t: 'key', action: key });
    else if (strong) out.push({ t: 'strong', s: strong });
    else if (em) out.push({ t: 'em', s: em });
    at = start + match[0].length;
  }
  if (at < text.length) out.push({ t: 'text', s: text.slice(at) });
  return out;
}

export function paragraphs(rich: Rich | undefined): readonly string[] {
  if (rich === undefined) return [];
  return typeof rich === 'string' ? [rich] : rich;
}

export function blocks(rich: Rich | undefined): Block[] {
  const out: Block[] = [];
  for (const paragraph of paragraphs(rich)) {
    if (paragraph.startsWith('- ')) {
      const last = out[out.length - 1];
      const item = runs(paragraph.slice(2));
      if (last?.t === 'ul') out[out.length - 1] = { t: 'ul', items: [...last.items, item] };
      else out.push({ t: 'ul', items: [item] });
    } else {
      out.push({ t: 'p', runs: runs(paragraph) });
    }
  }
  return out;
}

/** Every topic a piece of text links to. */
export function linksIn(rich: Rich | undefined): string[] {
  const out: string[] = [];
  for (const paragraph of paragraphs(rich)) for (const run of runs(paragraph)) if (run.t === 'link') out.push(run.id);
  return out;
}

/** Every shortcut a piece of text names. */
export function keysIn(rich: Rich | undefined): string[] {
  const out: string[] = [];
  for (const paragraph of paragraphs(rich)) for (const run of runs(paragraph)) if (run.t === 'key') out.push(run.action);
  return out;
}

/**
 * The words alone, for search and for places that can only show plain text. Links read as their
 * own words, or as the linked topic's title when [titleOf] knows it.
 */
export function plain(rich: Rich | undefined, titleOf: (id: string) => string | undefined = () => undefined, keyOf: (action: string) => string = (a) => a): string {
  return paragraphs(rich)
    .map((paragraph) =>
      runs(paragraph.startsWith('- ') ? paragraph.slice(2) : paragraph)
        .map((run) => (run.t === 'link' ? (run.s ?? titleOf(run.id) ?? run.id) : run.t === 'key' ? keyOf(run.action) : run.s))
        .join(''),
    )
    .join('\n');
}
