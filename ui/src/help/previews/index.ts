// Live parts of the real interface for the guide, loaded together the first time a topic with a
// preview opens. Each is given the setting it explains and the choice to show, and reads the rest
// of the settings as they are.

import type { Component } from 'svelte';
import CodePreview from './CodePreview.svelte';
import DarkPreview from './DarkPreview.svelte';
import DensityPreview from './DensityPreview.svelte';
import GeneratorPreview from './GeneratorPreview.svelte';
import IconsPreview from './IconsPreview.svelte';
import RowPreview from './RowPreview.svelte';
import SecretPreview from './SecretPreview.svelte';
import SettingsIconPreview from './SettingsIconPreview.svelte';
import SidebarPreview from './SidebarPreview.svelte';
import SiteIconPreview from './SiteIconPreview.svelte';
import TextSizePreview from './TextSizePreview.svelte';
import WindowPreview from './WindowPreview.svelte';

type Preview = Component<{ value?: unknown; pref?: string }>;

export const PREVIEWS: Record<string, Preview> = {
  'preview.row': RowPreview,
  'preview.code': CodePreview,
  'preview.generator': GeneratorPreview,
  'preview.secret': SecretPreview,
  'preview.site_icon': SiteIconPreview,
  'preview.settings_icon': SettingsIconPreview,
  'preview.icons': IconsPreview,
  'preview.density': DensityPreview,
  'preview.text_size': TextSizePreview,
  'preview.dark': DarkPreview,
  'preview.sidebar': SidebarPreview,
  'preview.window': WindowPreview as Preview,
};
