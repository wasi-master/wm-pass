// The whole app, in a tab.

import { boot } from '../../ui/src/boot';
import { pass } from '../../ui/src/engine/pass.svelte';
import { extensionHost } from './host';

// A tab can close with no warning, so nothing waits to be written.
pass.saveDelayMs = 0;
void extensionHost().then(boot);
