// Everything the guide knows, in one place. The app loads this lazily, the first time someone asks
// for help, and scripts/build-help.mjs turns it into the phone's help.json and the docs pages.

import type { Figure, Rich, Shell, Topic, TopicText, Tour } from './schema.ts';
import { linksIn } from './rich.ts';
import { FIGURES } from './figures.ts';
import { TOURS } from './tours.ts';
import { features } from './topics/features.ts';
import { screens } from './topics/screens.ts';
import { security } from './topics/security.ts';
import { protection } from './topics/protection.ts';
import { vault } from './topics/vault.ts';
import { backup } from './topics/backup.ts';
import { tools } from './topics/tools.ts';
import { look } from './topics/look.ts';
import { lists } from './topics/lists.ts';
import { editing } from './topics/editing.ts';
import { computer } from './topics/computer.ts';
import { app } from './topics/app.ts';
import { android } from './topics/android.ts';

export * from './schema.ts';
export * from './rich.ts';
export { FIGURES, TOURS };

export const TOPICS = [
  ...features,
  ...screens,
  ...security,
  ...protection,
  ...vault,
  ...backup,
  ...tools,
  ...look,
  ...lists,
  ...editing,
  ...computer,
  ...app,
  ...android,
] as const;

export type TopicId = (typeof TOPICS)[number]['id'];

/** Every setting the guide explains. A settings row for a pref outside this list does not type-check. */
export type HelpedPref = Extract<TopicId, `setting.${string}`> extends `setting.${infer K}` ? K : never;

const ALL: readonly Topic[] = TOPICS;
const BY_ID = new Map<string, Topic>(ALL.map((t) => [t.id, t]));

export function topicById(id: string): Topic | undefined {
  return BY_ID.get(id);
}

export function availableOn(topic: Topic, shell: Shell): boolean {
  return !topic.shells || topic.shells.includes(shell);
}

/** A topic as one platform shows it: its own title, page and words where they differ. */
export function resolve(topic: Topic, shell: Shell): Topic {
  const extra = topic.on?.[shell];
  return extra ? { ...topic, ...extra } : topic;
}

export function toursFor(shell: Shell): readonly Tour[] {
  return TOURS.filter((t) => !t.shells || t.shells.includes(shell));
}

export function figureById(id: string): Figure | undefined {
  return FIGURES.find((f) => f.id === id);
}

const TEXT_FIELDS = ['what', 'why', 'how', 'when', 'caveats'] as const;

function textsOf(t: Partial<TopicText>): Rich[] {
  const out: Rich[] = [];
  for (const key of TEXT_FIELDS) {
    const value = t[key];
    if (value !== undefined) out.push(value);
  }
  for (const effect of t.effects ?? []) out.push(effect.text);
  return out;
}

/**
 * What the types cannot check: links between topics, references to tours and figures, and titles
 * that the phone looks topics up by. Returns one line per problem.
 */
export function validate(): string[] {
  const problems: string[] = [];
  const seen = new Set<string>();
  const tours = new Set<string>(TOURS.map((t) => t.id));
  const figures = new Set<string>(FIGURES.map((f) => f.id));
  for (const topic of ALL) {
    if (seen.has(topic.id)) problems.push(`${topic.id}: the id is used twice`);
    seen.add(topic.id);
    if (topic.kind === 'setting' && !topic.pref) problems.push(`${topic.id}: a setting needs its pref`);
    if (topic.kind === 'setting' && topic.pref && topic.id !== `setting.${topic.pref}`) problems.push(`${topic.id}: its id should be setting.${topic.pref}`);
    const variants: Partial<TopicText>[] = [topic, ...Object.values(topic.on ?? {})];
    for (const variant of variants) {
      for (const rich of textsOf(variant)) {
        for (const id of linksIn(rich)) if (!BY_ID.has(id)) problems.push(`${topic.id}: links to ${id}, which does not exist`);
      }
      for (const id of variant.related ?? []) if (!BY_ID.has(id)) problems.push(`${topic.id}: related ${id} does not exist`);
      if (variant.tour && !tours.has(variant.tour)) problems.push(`${topic.id}: tour ${variant.tour} does not exist`);
      if (variant.figure && !figures.has(variant.figure)) problems.push(`${topic.id}: figure ${variant.figure} does not exist`);
    }
  }
  // The phone finds a row's topic by its title on its page, so two topics there cannot share one.
  for (const shell of ['desktop', 'extension', 'android'] as const) {
    const titles = new Map<string, string>();
    for (const topic of ALL) {
      if (!availableOn(topic, shell)) continue;
      const shown = resolve(topic, shell);
      if (shown.kind === 'page' || shown.kind === 'screen' || shown.kind === 'feature') continue;
      const key = `${shown.page ?? ''}/${shown.title}`;
      const other = titles.get(key);
      if (other) problems.push(`${topic.id}: on ${shell}, “${shown.title}” on ${shown.page} is also ${other}`);
      titles.set(key, topic.id);
    }
  }
  for (const tour of TOURS) {
    if (tour.steps.length === 0) problems.push(`${tour.id}: has no steps`);
  }
  for (const figure of FIGURES) {
    const where = new Map<string, number>();
    figure.rows.forEach((row, i) => row.forEach((box) => where.set(box.id, i)));
    for (const edge of figure.edges) {
      const a = where.get(edge.from);
      const b = where.get(edge.to);
      if (a === undefined || b === undefined) problems.push(`${figure.id}: edge ${edge.from} to ${edge.to} names a box that is not there`);
      else if (Math.abs(a - b) > 1) problems.push(`${figure.id}: edge ${edge.from} to ${edge.to} skips a row`);
    }
  }
  return problems;
}
