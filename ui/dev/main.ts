// The design preview: the real screens and the real engine, in a plain browser tab.

import { boot } from '$ui/boot';
import { wasmHost } from '$ui/hosts/wasm';

void wasmHost().then(boot);

// Sample items, for looking at the screens. Only this preview has it.
import { bytesOf, pass } from '$ui/engine/pass.svelte';
import { ui } from '$ui/vault/ui.svelte';
import { prefs } from '$ui/engine/prefs.svelte';

(window as any).wm = { pass, ui, prefs, bytesOf };

import { unzip, zipStored } from '$ui/engine/bundle';
import { PAGES } from '$ui/settings/pages';
import { help } from '$ui/help/help.svelte';
Object.assign((window as any).wm, { unzip, zipStored, PAGES, help });
