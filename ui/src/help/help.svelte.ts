// The guide: which topic is open, which tour is running, and what this install has already seen.
// The words themselves live in help/ at the root of the repository and load the first time
// someone asks for them, so the guide costs nothing until it is used.

import type { Shell, Topic, TopicId, Tour, TourId } from '$help';
import { host } from '../engine/host';
import { prefs } from '../engine/prefs.svelte';
import { chordLabel } from '../engine/text';
import { ui } from '../vault/ui.svelte';
import { highlight } from '../settings/nav.svelte';
import { PAGES } from '../settings/pages';

type Content = typeof import('$help');

export type { TopicId, TourId };

class Help {
  /** The content, once loaded. */
  content = $state.raw<Content | null>(null);
  /** Topics opened in the sheet, the last one showing. Empty means the sheet is closed. */
  stack = $state<string[]>([]);
  tour = $state.raw<Tour | null>(null);
  step = $state(0);
  welcome = $state(false);
  private loading: Promise<Content> | null = null;

  /** The platform the topics are read for. The browser preview is the extension's screens. */
  get shell(): Shell {
    const kind = host().kind;
    return kind === 'desktop' ? 'desktop' : 'extension';
  }

  load(): Promise<Content> {
    this.loading ??= import('$help').then((content) => (this.content = content));
    return this.loading;
  }

  /** A topic as this platform shows it, or nothing if it is not here. */
  topic(id: string): Topic | undefined {
    const content = this.content;
    if (!content) return undefined;
    const topic = content.topicById(id);
    if (!topic || !content.availableOn(topic, this.shell)) return undefined;
    return content.resolve(topic, this.shell);
  }

  title(id: string): string | undefined {
    return this.topic(id)?.title;
  }

  /** Where a topic lives, as the settings search writes it: "Settings, Security". */
  where(topic: Topic): string {
    if (topic.kind === 'screen') return 'Screens';
    if (topic.kind === 'feature' || topic.kind === 'concept') {
      if (!topic.page) return 'How WM Pass works';
    }
    const page = topic.page ? PAGES.find((p) => p.id === topic.page) : undefined;
    if (!page) return 'Guide';
    const parent = page.parent ? PAGES.find((p) => p.id === page.parent) : undefined;
    return ['Settings', parent?.title, topic.kind === 'page' ? null : page.title].filter(Boolean).join(', ');
  }

  keyLabel(action: string): string {
    const chord = prefs.shortcuts[action];
    return chord ? chordLabel(chord, host().os) : '';
  }

  get current(): Topic | undefined {
    const id = this.stack[this.stack.length - 1];
    return id ? this.topic(id) : undefined;
  }

  // ---- the sheet ----

  async open(id: TopicId | string) {
    await this.load();
    if (!this.topic(id)) return;
    if (this.stack[this.stack.length - 1] !== id) this.stack = [...this.stack, id];
  }

  back() {
    this.stack = this.stack.slice(0, -1);
  }

  close() {
    this.stack = [];
  }

  /** The topic for whatever the window is showing. */
  openHere() {
    void this.open(this.here());
  }

  here(): string {
    const section = ui.section;
    if (section.startsWith('settings/')) return `page.${section.slice(9)}`;
    if (section === 'settings') return 'page.guide';
    if (ui.editing) return 'screen.editor';
    if (section === 'codes' || section === 'generator' || section === 'health' || section === 'trash' || section === 'shelf') return `screen.${section}`;
    if (ui.selected && (ui.showDetail || !ui.narrow)) return 'screen.item';
    return 'screen.list';
  }

  /** Goes to the row a topic explains, and points at it the way the settings search does. */
  show(topic: Topic) {
    if (!topic.page) return;
    this.close();
    ui.go(`settings/${topic.page}`);
    if (topic.kind !== 'page') setTimeout(() => highlight.point(topic.title), 60);
  }

  // ---- search ----

  search(query: string): Topic[] {
    const content = this.content;
    const q = query.trim().toLowerCase();
    if (!content || !q) return [];
    const words = q.split(/\s+/);
    const out: { topic: Topic; rank: number }[] = [];
    for (const raw of content.TOPICS as readonly Topic[]) {
      if (!content.availableOn(raw, this.shell)) continue;
      const topic = content.resolve(raw, this.shell);
      const title = topic.title.toLowerCase();
      const text = `${title} ${topic.words ?? ''} ${content.plain(topic.what)}`.toLowerCase();
      if (!words.every((w) => text.includes(w))) continue;
      out.push({ topic, rank: title.startsWith(q) ? 0 : title.includes(q) ? 1 : 2 });
    }
    return out.sort((a, b) => a.rank - b.rank || a.topic.title.localeCompare(b.topic.title)).map((o) => o.topic);
  }

  // ---- tours ----

  tours(): readonly Tour[] {
    const content = this.content;
    if (!content) return [];
    return content.toursFor(this.shell).filter((t) => !t.width || (t.width === 'narrow') === ui.narrow);
  }

  async startTour(id: TourId) {
    const content = await this.load();
    const tour = content.TOURS.find((t) => t.id === id);
    if (!tour) return;
    this.close();
    this.welcome = false;
    this.step = 0;
    this.tour = tour;
  }

  /** The tour for the window as it is now: wide or narrow. */
  startWindowTour() {
    void this.startTour(ui.narrow ? 'tour.window_narrow' : 'tour.window');
  }

  endTour(finished: boolean) {
    const tour = this.tour;
    this.tour = null;
    this.step = 0;
    if (finished && tour) prefs.toggleInList('help_tours_done', tour.id, true);
  }

  done(id: string) {
    return prefs.listOf('help_tours_done').includes(id);
  }

  // ---- first times ----

  seen(key: string) {
    return prefs.listOf('help_seen').includes(key);
  }

  markSeen(key: string) {
    if (!this.seen(key)) prefs.toggleInList('help_seen', key, true);
  }

  dismissWelcome() {
    this.welcome = false;
    prefs.set('help_welcome_seen', true);
  }
}

export const help = new Help();
