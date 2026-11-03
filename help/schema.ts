// The shape of everything the in-app guide says. This folder is plain data: Vite bundles it into
// the desktop app and the extension, and scripts/build-help.mjs reads it with Node to write the
// phone's help.json and the docs site's Guide pages. So, for Node's type stripping:
//
// - every relative import ends in `.ts`;
// - no `enum` and no `namespace`;
// - anything from ui/src is `import type` only.
//
// How to write a topic
//
// - Plain words, the way the rest of the app talks. A label names the thing; a button starts with
//   its verb. British spelling. Settings paths are written with commas: "Settings, Security".
// - `what` is one or two sentences: what it is and what it does. Everything else is optional and
//   only says what a person could not guess.
// - A network topic says exactly what is sent, and to whom.
// - A protection topic says what it does not protect against.
// - No defaults in prose. The phone and the computer differ, and the sheet marks the default from
//   the platform's own table.
// - No marketing voice, no exclamation marks, no "simply" or "just".
//
// Rich text, used by every text field:
//
// - A string is one paragraph; an array is several.
// - A paragraph starting with "- " is a list item; consecutive items make one list.
// - [[topic.id]] links to another topic, drawn with that topic's title.
//   [[topic.id|words]] uses the given words instead.
// - {{key:action}} is the keyboard shortcut for an action, as the person has set it.
// - **Words** names something on screen, such as a button.
// - *words* is emphasis, used rarely.

import type { PrefKey } from '../ui/src/engine/prefs.svelte.ts';
import type { IconName } from '../ui/src/engine/icons.svelte.ts';

export type { PrefKey, IconName };

export type Shell = 'desktop' | 'extension' | 'android';

export type Rich = string | readonly string[];

export type TopicKind = 'setting' | 'action' | 'page' | 'screen' | 'feature' | 'concept';

/** A value a setting can take, and what happens when it does. */
export interface Effect {
  readonly value: string | number | boolean;
  readonly label: string;
  readonly text: Rich;
}

export interface TopicText {
  readonly title: string;
  /** The settings page it lives on, for "Open the setting" and the breadcrumb. */
  readonly page?: string;
  /** One or two sentences: what it is and what it does. */
  readonly what: Rich;
  /** The problem it solves. */
  readonly why?: Rich;
  /** How it works, for someone new to it. */
  readonly how?: Rich;
  /** When to use it, and when not to. */
  readonly when?: Rich;
  /** What each value does. */
  readonly effects?: readonly Effect[];
  /** Limits, platform notes, and what it does not protect against. */
  readonly caveats?: Rich;
  readonly figure?: FigureId;
  readonly preview?: PreviewId;
  readonly tour?: TourId;
  readonly related?: readonly string[];
  /** A hand-written page on the docs site, such as '/guides/duress/'. */
  readonly docs?: string;
  /** Extra words the search should find it by. */
  readonly words?: string;
}

export interface Topic extends TopicText {
  /**
   * setting.<pref>, action.<name>, page.<id>, screen.<name>, feature.<name>, concept.<name>, or
   * group.<page>.<name> for a group heading on a settings page (kind 'concept').
   */
  readonly id: string;
  readonly kind: TopicKind;
  /** The setting this explains, for the "Now" and "Default" marks and the option picker. */
  readonly pref?: PrefKey | AndroidPref;
  /** Where it exists. Absent means everywhere. */
  readonly shells?: readonly Shell[];
  /** What is different on one platform: its title, page, words or figure. */
  readonly on?: Partial<Record<Shell, Partial<TopicText>>>;
}

/** Settings only the phone has. Their keys are the phone's. */
export type AndroidPref =
  | 'accent'
  | 'auto_tag'
  | 'card_reader'
  | 'ui_style'
  | 'haptics'
  | 'settings_cards'
  | 'filter_chips'
  | 'search_at_bottom'
  | 'swipe_right'
  | 'swipe_left'
  | 'left_handed'
  | 'widgets'
  | 'nav_labels'
  | 'show_fab'
  | 'fab_action'
  | 'lock_screen_off'
  | 'autofill_inline'
  | 'autofill_save'
  | 'keyboard_access'
  | 'wear_access'
  | 'panic_action'
  | 'panic_app'
  | 'shortcuts';

// ---- tours ----

/** A place on screen a tour can point at. Svelte marks it with data-help-anchor. */
export type AnchorId =
  | 'sidebar.tabs'
  | 'sidebar.folders'
  | 'sidebar.lock'
  | 'sidebar.settings'
  | 'list.search'
  | 'list.add'
  | 'list.rows'
  | 'detail.actions'
  | 'detail.fields'
  | 'detail.more'
  | 'editor.name'
  | 'editor.fields'
  | 'editor.folds'
  | 'editor.save'
  | 'codes.list'
  | 'codes.add'
  | 'generator.value'
  | 'generator.modes'
  | 'generator.controls'
  | 'generator.save'
  | 'health.score'
  | 'health.groups'
  | 'settings.search'
  | 'settings.list'
  | 'settings.guide'
  | 'narrow.tabs'
  | 'group:security.locking'
  | 'group:security.master'
  | 'group:security.methods'
  | 'group:security.duress'
  | 'group:security.emergency'
  | 'group:security.network'
  | 'group:backup.places'
  | 'group:backup.schedule'
  | 'group:backup.manual'
  | 'group:sync.lan'
  | 'group:sync.places'
  | 'group:sync.accounts'
  | 'group:browser.connect'
  | 'group:browser.setup'
  | 'group:browser.paired'
  | 'group:desktop.window'
  | 'group:desktop.palette'
  | 'group:desktop.autotype';

export interface Step {
  readonly anchor: AnchorId;
  readonly title: string;
  readonly body: Rich;
  readonly placement?: 'top' | 'bottom' | 'left' | 'right';
  /** What to do before pointing: go to a section, or open a sample item or a new editor. */
  readonly before?: { readonly go?: string; readonly open?: 'sample' | 'editor' };
  /** The highlighted control stays clickable. */
  readonly tryIt?: boolean;
}

export type TourId =
  | 'tour.window'
  | 'tour.window_narrow'
  | 'tour.home'
  | 'tour.first_item'
  | 'tour.codes'
  | 'tour.generator'
  | 'tour.health'
  | 'tour.security'
  | 'tour.backup'
  | 'tour.sync'
  | 'tour.browser'
  | 'tour.palette';

export interface Tour {
  readonly id: TourId;
  readonly title: string;
  readonly intro?: Rich;
  readonly shells?: readonly Shell[];
  /** Only on a wide window, or only on a narrow one. */
  readonly width?: 'wide' | 'narrow';
  readonly steps: readonly Step[];
}

// ---- figures ----

export type FigureId =
  | 'fig.vault'
  | 'fig.lock'
  | 'fig.clipboard'
  | 'fig.duress'
  | 'fig.kit'
  | 'fig.shelf'
  | 'fig.backup'
  | 'fig.sync'
  | 'fig.lan'
  | 'fig.vaults'
  | 'fig.bridge'
  | 'fig.ssh'
  | 'fig.autotype'
  | 'fig.breach'
  | 'fig.alias';

export interface FigureBox {
  readonly id: string;
  readonly label: string;
  readonly sub?: string;
  /** Only where the box is a device, a file or a service. */
  readonly icon?: IconName;
  readonly tone?: 'muted' | 'danger' | 'warn' | 'good';
}

/**
 * Rows of boxes, top to bottom, with arrows. An edge joins two boxes side by side in one row, or a
 * box to one in the next row. Each platform lays it out with its own fonts, so nothing here is a
 * coordinate.
 */
export interface Figure {
  readonly id: FigureId;
  readonly caption: string;
  readonly rows: readonly (readonly FigureBox[])[];
  readonly edges: readonly { readonly from: string; readonly to: string; readonly label?: string; readonly dashed?: boolean }[];
}

// ---- previews ----

/** A live part of the real interface, with sample data. Each platform draws the ones it has. */
export type PreviewId =
  | 'preview.row'
  | 'preview.code'
  | 'preview.generator'
  | 'preview.secret'
  | 'preview.site_icon'
  | 'preview.settings_icon'
  | 'preview.density'
  | 'preview.text_size'
  | 'preview.dark'
  | 'preview.icons'
  | 'preview.sidebar'
  | 'preview.window';
