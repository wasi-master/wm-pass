// Whether the screens are showing the message about an internal error (internal.ts). While they
// are, nothing reaches the session and no other message or question shows.

export { isInternal, stoppedError } from './internal';

export const stopped = $state({ on: false });
