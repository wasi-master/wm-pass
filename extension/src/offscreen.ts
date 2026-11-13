// A hidden page Chrome lends for the moment it takes to clear the clipboard.

export {};

declare const chrome: any;

chrome.runtime.onMessage.addListener((message: any, sender: any, respond: (r: unknown) => void) => {
  // Only the background asks: a content script's message reaches this page too, and must not
  // empty the clipboard of whatever the person copied.
  if (message?.type !== 'offscreen.clear' || sender.id !== chrome.runtime.id || sender.tab) return false;
  emptyClipboard();
  respond({});
  return true;
});

/**
 * Leaves the clipboard empty. A copy needs something selected to go at all, so the text it would
 * copy is replaced on its way: a space put there instead would be pasted, into a password box say.
 */
function emptyClipboard() {
  const replace = (event: ClipboardEvent) => {
    event.clipboardData?.setData('text/plain', '');
    event.preventDefault();
  };
  document.addEventListener('copy', replace, { once: true });
  const box = document.createElement('textarea');
  box.value = ' ';
  document.body.append(box);
  box.select();
  document.execCommand('copy');
  box.remove();
  document.removeEventListener('copy', replace);
}
