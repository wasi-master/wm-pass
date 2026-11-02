// A hidden page Chrome lends for the moment it takes to clear the clipboard.

export {};

declare const chrome: any;

chrome.runtime.onMessage.addListener((message: any, _sender: any, respond: (r: unknown) => void) => {
  if (message?.type !== 'offscreen.clear') return false;
  const box = document.createElement('textarea');
  box.value = ' ';
  document.body.append(box);
  box.select();
  document.execCommand('copy');
  box.remove();
  respond({});
  return true;
});
