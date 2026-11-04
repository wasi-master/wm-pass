// Hands a page to the system's print dialog, where it can go to a printer or be saved as a PDF,
// without writing a file first. The page is put into this window for printing only: it is never
// drawn on screen, runs no script, loads nothing, and is taken out again once the dialog is done.

const SHEET = 'print-sheet';

/** Prints `html`, and returns what clears it, for a page that is left before the dialog closes. */
export async function printPage(html: string): Promise<() => void> {
  // Parsing does not run scripts; anything that could load or run is dropped all the same.
  const doc = new DOMParser().parseFromString(html, 'text/html');
  doc.querySelectorAll('script, iframe, object, embed, link, img, video, audio, base').forEach((n) => n.remove());

  const holder = document.createElement('div');
  holder.className = SHEET;
  const root = holder.attachShadow({ mode: 'closed' });
  const style = document.createElement('style');
  // The page's own rules, with its body standing for the page inside the sheet.
  const own = [...doc.querySelectorAll('style')].map((s) => s.textContent ?? '').join('\n');
  style.textContent = `${own.replace(/(^|[}\s,])body(?=\s*[{,])/g, '$1.page')}\n.page{background:#fff;color:#000}`;
  const page = document.createElement('div');
  page.className = 'page';
  page.append(...[...doc.body.childNodes].map((n) => document.adoptNode(n)));
  root.append(style, page);

  const rules = document.createElement('style');
  rules.textContent =
    `@media screen{.${SHEET}{display:none!important}}` +
    `@media print{body>:not(.${SHEET}){display:none!important}html,body{background:#fff!important;height:auto!important;overflow:visible!important}}`;

  const title = document.title;
  document.title = doc.title || title;
  document.body.append(rules, holder);

  let timer: ReturnType<typeof setTimeout> | undefined;
  let cleared = false;
  const clear = () => {
    if (cleared) return;
    cleared = true;
    page.replaceChildren();
    holder.remove();
    rules.remove();
    document.title = title;
    clearTimeout(timer);
    window.removeEventListener('afterprint', clear);
    document.removeEventListener('pointerdown', clear, true);
    document.removeEventListener('keydown', clear, true);
  };
  window.addEventListener('afterprint', clear);
  // Where the dialog does not say when it closes, the page goes after ten minutes at most.
  timer = setTimeout(clear, 10 * 60_000);
  try {
    // On a Mac the desktop app answers with a promise, and the dialog draws the page after it.
    await (window.print() as unknown);
  } catch (failure) {
    clear();
    throw failure;
  }
  // The window takes no clicks or keys while the dialog is up, so the next one means it closed.
  document.addEventListener('pointerdown', clear, true);
  document.addEventListener('keydown', clear, true);
  return clear;
}
