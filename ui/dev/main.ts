// The design preview: the real screens and the real engine, in a plain browser tab.

import { boot } from '$ui/boot';
import { wasmHost } from '$ui/hosts/wasm';

void wasmHost().then(boot);
