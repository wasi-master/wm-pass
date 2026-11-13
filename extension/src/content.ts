// Runs beside the page, so nothing here may trust it. No field is ever filled on load: a fill
// starts from a real click on a menu the page cannot reach into, after checks that the menu and
// the field are really visible, because a page can stack invisible things to steal a click.

export {};

declare const chrome: any;

interface Row {
  id: string;
  title: string;
  subtitle: string;
}

const HOVER_DELAY_MS = 400;

function send<T = any>(message: Record<string, unknown>): Promise<T> {
  // After the extension updates, this old script's runtime is gone and sendMessage throws at
  // once instead of rejecting, which would skip every catch below.
  let sent: Promise<any>;
  try {
    sent = chrome.runtime.sendMessage(message);
  } catch (failure) {
    return Promise.reject(failure);
  }
  return sent.then((reply: any) => {
    if (reply?.failed) throw new Error(reply.failed);
    return reply?.ok as T;
  });
}

// ---- passkeys, relayed from the page's world ----

// Any script on the page can post this message, not only the page-world shim, so it gets the
// same checks: a real click just now, and one request at a time.
let passkeyPending: string | null = null;
window.addEventListener('message', (event) => {
  // A script left behind by an update stays quiet, so the one in its place answers.
  // A sandboxed frame has no origin to answer to, and the shim there asks nothing.
  if (event.source !== window || !alive() || location.origin === 'null') return;
  // The site gave up on the request it made: the window asking the person goes too, and nothing
  // is made or signed for a page no longer waiting.
  if (event.data?.wmpassCancel && event.data.wmpassCancel === passkeyPending) {
    void send({ type: 'cs.passkey.cancel' }).catch(() => {});
    return;
  }
  if (!event.data?.wmpassAsk) return;
  const { wmpassAsk: id, op, options } = event.data;
  const reply = (answer: unknown) => window.postMessage({ wmpassReply: id, answer }, location.origin);
  window.postMessage({ wmpassAck: id }, location.origin);
  if (passkeyPending || !(navigator as any).userActivation?.isActive) {
    reply({ fallback: true });
    return;
  }
  passkeyPending = String(id);
  send({ type: 'cs.passkey', op, options })
    .catch(() => ({ fallback: true }))
    .then((answer) => {
      passkeyPending = null;
      reply(answer);
    });
});

// ---- finding fields ----

/** The element above this one, through the edge of a shadow root to the element that holds it. */
function above(node: Element): Element | null {
  return node.parentElement ?? ((node.getRootNode() as ShadowRoot).host || null);
}

function visible(el: HTMLElement) {
  const rect = el.getBoundingClientRect();
  if (rect.width < 8 || rect.height < 8) return false;
  // Pushed off the page's top or left edge, as boxes kept for bots to fill are: no scroll brings
  // it into sight.
  if (rect.right + scrollX <= 0 || rect.bottom + scrollY <= 0) return false;
  // Up through any shadow root too: a faded element that holds one fades the field inside it.
  for (let node: Element | null = el; node; node = above(node)) {
    const style = getComputedStyle(node);
    if (style.visibility === 'hidden' || style.display === 'none' || Number(style.opacity) < 0.2) return false;
  }
  return true;
}

/** The document, or the shadow root a field sits in, which answers for what is inside it. */
function rootOf(el: Element): Document | ShadowRoot {
  const root = el.getRootNode();
  return root instanceof ShadowRoot ? root : document;
}

/** Whether a person can see and is pointing at this thing, not something laid over it. */
function onTop(el: Element, x: number, y: number) {
  const top = rootOf(el).elementFromPoint(x, y);
  return top === el || el.contains(top) || (top instanceof Element && top.contains(el) && top !== document.documentElement);
}

/**
 * Every input in a scope, and in the open shadow roots inside it, in the order they are on the
 * page. Sites built from web components keep their sign-in boxes in shadow roots, where a plain
 * query of the document never looks.
 */
function inputsIn(scope: ParentNode): HTMLInputElement[] {
  const found: HTMLInputElement[] = [];
  const walk = (node: ParentNode) => {
    for (const el of Array.from(node.querySelectorAll('*'))) {
      if (el instanceof HTMLInputElement) found.push(el);
      if (el.shadowRoot) walk(el.shadowRoot);
    }
  };
  walk(scope);
  return found;
}

/** Whether an element is inside another, counting the elements that hold shadow roots. */
function holds(outer: Element, el: Element) {
  for (let node: Element | null = el; node; node = above(node)) if (node === outer) return true;
  return false;
}

/** Where an event really started, inside an open shadow root too, rather than the element holding it. */
function startOf(event: Event): EventTarget | null {
  return event.composedPath()[0] ?? event.target;
}

const textTypes = new Set(['text', 'email', 'tel', '']);

function isPassword(el: Element): el is HTMLInputElement {
  return el instanceof HTMLInputElement && el.type === 'password' && !el.disabled && !el.readOnly;
}

/** A text box that says it is for something else, such as a search. */
function saysNotLogin(el: HTMLInputElement) {
  const words = `${el.name} ${el.id} ${el.autocomplete} ${el.placeholder} ${el.getAttribute('aria-label') ?? ''}`.toLowerCase();
  return /search|query|coupon|captcha|otp|code/.test(words);
}

function looksLikeUsername(el: Element): el is HTMLInputElement {
  if (!(el instanceof HTMLInputElement) || !textTypes.has(el.type) || el.disabled || el.readOnly) return false;
  if (saysNotLogin(el)) return false;
  const words = `${el.name} ${el.id} ${el.autocomplete} ${el.placeholder} ${el.getAttribute('aria-label') ?? ''}`.toLowerCase();
  return /user|login|email|e-mail|account|identifier|phone|mail/.test(words) || el.type === 'email';
}

/**
 * A box that names itself a username or an email, rather than only something near one, such as a
 * phone number or an account number on a sign-up form, which sits between the email and the
 * password as often as not.
 */
function saysUsername(el: HTMLInputElement) {
  const words = `${el.name} ${el.id} ${el.placeholder} ${el.getAttribute('aria-label') ?? ''}`.toLowerCase();
  return el.type === 'email' || /\b(username|email|webauthn)\b/i.test(el.autocomplete) || /user.?name|login|e-?mail|identifier/.test(words);
}

function looksLikeCode(el: Element): el is HTMLInputElement {
  if (!(el instanceof HTMLInputElement) || !textTypes.has(el.type) && el.type !== 'number' || el.disabled || el.readOnly) return false;
  const words = `${el.name} ${el.id} ${el.autocomplete} ${el.placeholder}`.toLowerCase();
  // Names such as otp_code, otpCode and mfaCode count: only the start of the word is checked.
  return el.autocomplete === 'one-time-code' || /(?:^|[^a-z0-9])(otp|totp|2fa|mfa)|two.?factor|verification.?code|auth.?code|one.?time.?(code|pass)/.test(words);
}

function fieldsAround(anchor: HTMLInputElement) {
  const scope: ParentNode = anchor.form ?? document;
  const inputs = inputsIn(scope).filter((el) => visible(el));
  const password = inputs.find(isPassword) ?? null;
  // The username box the person chose, rather than another that also looks like one.
  const chosen = looksLikeUsername(anchor) && inputs.includes(anchor) ? anchor : null;
  let username: HTMLInputElement | null = null;
  if (password) {
    const before = inputs.slice(0, inputs.indexOf(password)).reverse();
    // With no box that says it is for a username, the nearest text box before the password, but
    // never one that says it is for something else, such as the site's search.
    username =
      (chosen && before.includes(chosen) ? chosen : null) ??
      before.find((el) => looksLikeUsername(el) && saysUsername(el)) ??
      before.find(looksLikeUsername) ??
      before.find((el) => textTypes.has(el.type) && !el.disabled && !el.readOnly && !saysNotLogin(el) && !looksLikeCode(el)) ??
      null;
  } else {
    username = chosen ?? inputs.find((el) => looksLikeUsername(el) && saysUsername(el)) ?? inputs.find(looksLikeUsername) ?? null;
  }
  return { username, password, code: inputs.find(looksLikeCode) ?? null };
}

function setValue(el: HTMLInputElement, value: string) {
  el.focus();
  // Frameworks watch the element's own setter, so go around it.
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  setter?.call(el, value);
  el.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, inputType: 'insertReplacementText' }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
}

/**
 * Whether a box gets the key: a password or a code box, or a username box beside a password, or
 * on its own as a sign-in's first step, but not one for a newsletter or an invite.
 */
function offersFill(el: HTMLInputElement) {
  if (isPassword(el) || looksLikeCode(el)) return true;
  if (!looksLikeUsername(el)) return false;
  return !!fieldsAround(el).password || looksLikeSignInStep(el, false);
}

function fillFields(given: HTMLInputElement | null, fill: { username: string; password: string; code: string | null }) {
  // A field a single-page app has since drawn again is no longer on the page, and one since
  // hidden, such as the first step of a two-step form, is no guide to which form is in use.
  const anchor = given?.isConnected && visible(given) ? given : null;
  // The field last pointed at, when it belongs to a sign-in form. Otherwise the form with a
  // password field, rather than whatever text box comes first, such as a newsletter's.
  const inputs = inputsIn(document).filter((el) => visible(el));
  // A username box alone counts only on a page with no password field: the first step of a
  // two-step sign-in.
  const anchorFits =
    anchor && (isPassword(anchor) || looksLikeCode(anchor) || fieldsAround(anchor).password || (looksLikeUsername(anchor) && !inputs.some(isPassword)));
  // With no password on the page, the first step of a sign-in, never a newsletter's or an invite's
  // email box, which takes someone's address for a list rather than for signing in.
  const start =
    (anchorFits ? anchor : null) ??
    inputs.find(isPassword) ??
    inputs.find((el) => looksLikeUsername(el) && looksLikeSignInStep(el, false)) ??
    inputs.find(looksLikeCode);
  if (!start) return false;
  const { username, password, code } = fieldsAround(start);
  if (username && fill.username) setValue(username, fill.username);
  if (password && fill.password) setValue(password, fill.password);
  if (code && fill.code) setValue(code, fill.code);
  fill.password = '';
  return !!(username || password || code);
}

// ---- the menu ----

const MENU_CSS = `
:host { all: initial; }
* { box-sizing: border-box; font: 13px/1.35 -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif; }
.key { position: fixed; width: 22px; height: 22px; padding: 3px; border: 0; border-radius: 4px; background: transparent; color: #0d9488; cursor: pointer; }
.key:hover { background: rgba(13, 148, 136, .14); }
.key svg { display: block; width: 16px; height: 16px; fill: currentColor; }
.menu, .bar { position: fixed; background: #fff; color: #1a1a1a; border: 1px solid #c8c8c8; border-radius: 6px; box-shadow: 0 2px 10px rgba(0,0,0,.18); }
.menu { width: 300px; max-width: calc(100vw - 8px); max-height: 280px; overflow-y: auto; padding: 4px; }
.row { display: block; width: 100%; padding: 6px 8px; border: 0; border-radius: 4px; background: transparent; color: inherit; text-align: left; cursor: pointer; }
.row:hover, .row:focus-visible { background: rgba(13, 148, 136, .14); outline: 0; }
.title { display: block; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sub { display: block; color: #5f5f5f; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.note { padding: 6px 8px; color: #5f5f5f; }
.warn { padding: 6px 8px; color: #a04b00; }
.bar { padding: 10px 12px; display: grid; gap: 8px; }
.actions { display: flex; gap: 6px; justify-content: flex-end; }
.btn { height: 26px; padding: 0 10px; border: 1px solid #c8c8c8; border-radius: 4px; background: #f6f6f6; color: inherit; cursor: pointer; }
.btn.main { background: #0d9488; border-color: #0d9488; color: #fff; }
@media (prefers-color-scheme: dark) {
  .menu, .bar { background: #252525; color: #eee; border-color: #444; }
  .sub, .note { color: #aaa; }
  .warn { color: #e0a060; }
  .btn { background: #333; border-color: #555; }
  .btn.main { background: #0d9488; border-color: #0d9488; color: #fff; }
  .key { color: #2dd4bf; }
}`;

// Built as nodes, not markup: nothing in a content script goes through innerHTML.
const SVG = 'http://www.w3.org/2000/svg';
function keyIcon(): SVGSVGElement {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 -960 960 960');
  svg.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS(SVG, 'path');
  path.setAttribute('d', 'M280-400q-33 0-56.5-23.5T200-480q0-33 23.5-56.5T280-560q33 0 56.5 23.5T360-480q0 33-23.5 56.5T280-400Zm0 160q-100 0-170-70T40-480q0-100 70-170t170-70q67 0 121.5 33t86.5 87h352l120 120-180 180-80-60-80 60-85-60h-47q-32 54-86.5 87T280-240Zm0-80q56 0 98.5-34t56.5-86h125l58 41 82-61 71 55 75-75-40-40H435q-14-52-56.5-86T280-640q-66 0-113 47t-47 113q0 66 47 113t113 47Z');
  svg.append(path);
  return svg;
}

let host: HTMLElement | null = null;
let root: ShadowRoot | null = null;
let current: HTMLInputElement | null = null;
let shownAt = 0;

/**
 * Where the menu must sit to be seen and pressed: in the modal dialog or open popover that holds
 * the field, which the browser draws above the whole page. A modal dialog also makes everything
 * outside it inert, so a menu left on the page would be under it and dead to clicks.
 */
function layerOf(field: Element | null): Element {
  for (let node = field; node; node = above(node)) {
    if (!(node instanceof HTMLDialogElement) && !node.hasAttribute('popover')) continue;
    for (const open of [':modal', ':popover-open']) {
      try {
        if (node.matches(open)) return node;
      } catch {
        // A browser that does not know the selector has no such layer either.
      }
    }
  }
  return document.documentElement;
}

/**
 * Where the menu's fixed positions count from: the viewport, or a dialog drawn with a transform,
 * as dialogs centred with translate(-50%, -50%) are, which fixed things inside it are placed by.
 */
function origin() {
  const rect = host?.getBoundingClientRect();
  return { x: rect?.left ?? 0, y: rect?.top ?? 0 };
}

function ensureHost(field: Element | null = null) {
  const layer = layerOf(field);
  if (host?.isConnected && root) {
    if (host.parentElement !== layer) layer.append(host);
    return root;
  }
  host = document.createElement('wm-pass-menu');
  // Closed, so the page's scripts cannot read or press what is inside.
  root = host.attachShadow({ mode: 'closed' });
  const style = document.createElement('style');
  style.textContent = MENU_CSS;
  root.append(style);
  host.style.cssText = 'all: initial !important; position: fixed !important; inset: 0 auto auto 0 !important; z-index: 2147483647 !important; opacity: 1 !important; transform: none !important; filter: none !important; pointer-events: auto !important; display: block !important;';
  layer.append(host);
  return root;
}

function clearMenu() {
  root?.querySelectorAll('.key, .menu').forEach((el) => el.remove());
}

function positionNear(el: HTMLElement, anchor: HTMLElement) {
  const rect = anchor.getBoundingClientRect();
  const from = origin();
  // Its own height once drawn: above a field low in the window, a menu of one login sits just
  // over the field, not the height of a full menu away from it.
  const height = el.isConnected && el.offsetHeight ? el.offsetHeight : 280;
  // Never off the left edge, as a frame narrower than the menu would otherwise put it.
  el.style.left = `${Math.max(4, Math.min(rect.left, innerWidth - 310)) - from.x}px`;
  el.style.top = `${(rect.bottom + 4 + height > innerHeight ? Math.max(4, rect.top - height - 4) : rect.bottom + 4) - from.y}px`;
}

/** Placed again as what is in it changes, such as the logins arriving in a menu first empty. */
function keepNear(el: HTMLElement, anchor: HTMLElement) {
  if (typeof ResizeObserver === 'undefined') return;
  new ResizeObserver(() => {
    if (el.isConnected && anchor.isConnected) positionNear(el, anchor);
  }).observe(el);
}

/** Everything a click must pass before anything is filled. */
function safeClick(event: MouseEvent, field: HTMLInputElement) {
  // Asked of the tree the menu is in, which is a shadow root's where the dialog it sits in is.
  if (!host || (host.getRootNode() as Document | ShadowRoot).elementFromPoint(event.clientX, event.clientY) !== host) return false;
  return safeChoice(event, field, true);
}

/**
 * What a click or a key press on the menu must pass: a real event, a menu shown long enough to
 * be seen, and a field that is really there. A key press reaches the menu only from the field
 * itself, so it needs no pointer over it.
 */
function safeChoice(event: Event, field: HTMLInputElement, pointer: boolean) {
  if (!event.isTrusted) return false;
  if (performance.now() - shownAt < HOVER_DELAY_MS) return false;
  if (!visible(field)) return false;
  const rect = field.getBoundingClientRect();
  if (pointer && !onTop(field, rect.left + rect.width / 2, rect.top + rect.height / 2) && !(rootOf(field).activeElement === field)) return false;
  return menuShown();
}

/**
 * Whether the menu can really be seen: nothing that holds it, such as the page itself or the
 * dialog it was put in, is faded or filtered to hide what the click lands on.
 */
function menuShown() {
  if (!host) return false;
  for (let node: Element | null = above(host); node; node = above(node)) {
    const style = getComputedStyle(node);
    if (Number(style.opacity) < 0.95 || style.filter !== 'none') return false;
  }
  if (document.body && !holds(document.body, host)) {
    const style = getComputedStyle(document.body);
    if (Number(style.opacity) < 0.95 || style.filter !== 'none') return false;
  }
  return true;
}

function note(menu: HTMLElement, text: string, kind = 'note') {
  const line = document.createElement('div');
  line.className = kind;
  // Said aloud as it comes, since the focus stays in the field while the menu answers.
  line.setAttribute('role', kind === 'warn' ? 'alert' : 'status');
  line.textContent = text;
  menu.append(line);
}

async function openMenu(field: HTMLInputElement, byKeyboard = false) {
  const box = ensureHost(field);
  clearMenu();
  const menu = document.createElement('div');
  menu.className = 'menu';
  // Plain buttons in a named group: a listbox may hold only options, and this one holds notes too.
  menu.setAttribute('role', 'group');
  menu.setAttribute('aria-label', 'Logins from WM Pass');
  positionNear(menu, field);
  box.append(menu);
  keepNear(menu, field);
  shownAt = performance.now();
  const answer = await send<{ source: string; locked: boolean; rows: Row[]; foreign: string | null }>({ type: 'cs.site' }).catch(() => null);
  if (!answer) {
    note(menu, 'WM Pass could not answer.');
    return;
  }
  if (answer.locked && answer.source === 'desktop') {
    note(menu, 'The WM Pass app is locked. Unlock it there, then try again.');
    return;
  }
  if (answer.locked) {
    const open = document.createElement('button');
    open.className = 'row';
    open.textContent = answer.source === 'none' ? 'No vault is open. Open WM Pass' : 'WM Pass is locked. Open it';
    open.addEventListener('click', (event) => {
      if (event.isTrusted) void send({ type: 'cs.open' }).catch(() => {});
    });
    menu.append(open);
    return;
  }
  if (!answer.rows.length) {
    note(menu, 'Nothing saved for this site.');
    return;
  }
  if (answer.foreign) note(menu, `This form is from ${answer.foreign}, inside another site. Choosing below fills it there.`, 'warn');
  for (const row of answer.rows) {
    const button = document.createElement('button');
    button.className = 'row';
    const title = document.createElement('span');
    title.className = 'title';
    title.textContent = row.title;
    const sub = document.createElement('span');
    sub.className = 'sub';
    sub.textContent = row.subtitle;
    button.append(title, sub);
    const say = (text: string) => {
      const after = ensureHost(field);
      const message = document.createElement('div');
      message.className = 'menu';
      positionNear(message, field);
      note(message, text, 'warn');
      after.append(message);
      keepNear(message, field);
    };
    const choose = async () => {
      clearMenu();
      try {
        const { fill } = await send<{ fill: any }>({ type: 'cs.fill', id: row.id });
        // The form can go while the login is fetched, as a single-page app draws it again.
        if (!fillFields(field, fill)) say('There is no sign-in form here to fill now.');
      } catch (failure: any) {
        say(failure?.message === 'refused' ? 'Not filled.' : (failure?.message ?? 'Not filled.'));
      }
    };
    button.addEventListener('click', (event) => {
      if (event.detail === 0) return; // Enter or Space on the row: handled as a key below.
      if (safeClick(event, field)) void choose();
    });
    button.addEventListener('keydown', (event) => {
      // Space presses a button as Enter does, but its click comes with no pointer and is let go.
      if (event.key !== 'Enter' && event.key !== ' ') return;
      // The page hears keys from inside the menu too, and some send their form on Enter, which
      // would go before the login is in it.
      event.stopPropagation();
      if (safeChoice(event, field, false)) {
        event.preventDefault();
        void choose();
      }
    });
    menu.append(button);
  }
  // From the keyboard: arrows move between the logins, Escape goes back to the field.
  menu.addEventListener('keydown', (event) => {
    const rows = Array.from(menu.querySelectorAll<HTMLButtonElement>('button.row'));
    const at = rows.indexOf(root?.activeElement as HTMLButtonElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      rows[Math.max(0, Math.min(rows.length - 1, at + (event.key === 'ArrowDown' ? 1 : -1)))]?.focus();
    } else if (event.key === 'Escape') {
      // Only the menu goes, not a dialog of the site's that holds the form.
      event.preventDefault();
      event.stopPropagation();
      clearMenu();
      field.focus();
    }
  });
  // The wait for a click counts from when the logins are there to see, not from the empty menu:
  // a slow answer, such as from a worker just woken, would otherwise leave no wait at all.
  if (menu.isConnected) shownAt = performance.now();
  if (byKeyboard) menu.querySelector<HTMLButtonElement>('button.row')?.focus();
}

function showKey(field: HTMLInputElement) {
  const box = ensureHost(field);
  clearMenu();
  current = field;
  const key = document.createElement('button');
  key.className = 'key';
  key.title = 'Fill from WM Pass';
  key.setAttribute('aria-label', 'Fill from WM Pass');
  key.append(keyIcon());
  const rect = field.getBoundingClientRect();
  // Clear of anything the site already put at the field's right end, such as its eye button.
  let right = rect.right - 4;
  const middle = rect.top + rect.height / 2;
  for (let tries = 0; tries < 3; tries++) {
    const there = rootOf(field).elementFromPoint(right - 11, middle);
    if (!there || there === field || field.contains(there) || there.contains(field) || there === host) break;
    const theirs = there.getBoundingClientRect();
    if (theirs.left <= rect.left + rect.width / 2) break;
    right = theirs.left - 2;
  }
  const from = origin();
  key.style.left = `${right - 22 - from.x}px`;
  key.style.top = `${rect.top + (rect.height - 22) / 2 - from.y}px`;
  key.addEventListener('mousedown', (event) => event.preventDefault());
  key.addEventListener('click', (event) => {
    if (event.isTrusted) void openMenu(field);
  });
  box.append(key);
  keyAt = `${rect.left},${rect.top},${rect.width}`;
  watchField();
}

/**
 * While the key or menu is up, a look twice a second at the field it belongs to. A single-page
 * app that draws another screen removes the field without any focus moving, which would leave the
 * key floating over the new one; and a banner or error shown above the form moves the field from
 * under its key without a scroll.
 */
let keyAt = '';
let watching = 0;
function watchField() {
  if (watching) return;
  watching = window.setInterval(() => {
    const field = current;
    if (!field || !root?.querySelector('.key, .menu')) {
      clearInterval(watching);
      watching = 0;
      return;
    }
    if (!field.isConnected || !visible(field)) {
      clearMenu();
      current = null;
      return;
    }
    const rect = field.getBoundingClientRect();
    if (root.querySelector('.key') && `${rect.left},${rect.top},${rect.width}` !== keyAt) showKey(field);
  }, 500);
}

// Alt+ArrowDown in a field with the key opens the menu with the first login chosen. Plain
// ArrowDown too, where the field holds nothing yet and the arrow would do nothing.
document.addEventListener(
  'keydown',
  (event) => {
    if (!event.isTrusted || startOf(event) !== current || !current) return;
    // Escape in the field puts away a menu opened with a click, which leaves the focus there, and
    // brings back the key, so ArrowDown opens it again. The site's own Escape, such as one that
    // closes the dialog the form is in, waits for a press with no menu up.
    if (event.key === 'Escape' && root?.querySelector('.menu')) {
      event.preventDefault();
      event.stopPropagation();
      showKey(current);
      return;
    }
    if (event.key !== 'ArrowDown') return;
    // With a menu opened by a click, the arrow goes into it, as it would in the browser's own.
    const first = root?.querySelector<HTMLButtonElement>('.menu button.row');
    if (first) {
      event.preventDefault();
      first.focus();
      return;
    }
    if (!event.altKey && (current.value || current.list || current.getAttribute('role') === 'combobox')) return;
    if (!root?.querySelector('.key')) return;
    event.preventDefault();
    void openMenu(current, true);
  },
  true,
);

// WM Pass's own vault, the web vault and the dev servers that serve it, asks for the vault's own
// master password: nothing there is for filling or saving. The manifest leaves out the published
// web vault; this covers the dev servers too, and a copy of this script the popup put there.
const ownVault =
  (/(^|\.)wmpass\.pages\.dev$/.test(location.hostname) && location.pathname.startsWith('/vault')) ||
  (/^(localhost|127\.0\.0\.1)$/.test(location.hostname) && /^518[34]$/.test(location.port));

// A script left behind by an update of the extension can reach nothing, so it stays out of the
// way: the browser gives a reloaded page, or a fill from the popup, a working one.
const alive = () => !!chrome.runtime?.id && !ownVault;

// Focus that moves between two boxes of one shadow root never leaves it, so the document hears
// nothing of it. Each shadow root a box was focused in is listened to as well.
const heardIn = new WeakSet<ShadowRoot>();
let lastFocus: Event | null = null;

/**
 * Listens in the shadow root an element was focused in. Any box, not only a sign-in box: the name
 * box of a sign-up form focused first would otherwise leave the email box after it unheard.
 */
function listenIn(el: Element) {
  // And in each shadow root around that one, where a component nests another.
  for (let shadow = el.getRootNode(); shadow instanceof ShadowRoot; shadow = shadow.host.getRootNode()) {
    if (heardIn.has(shadow)) continue;
    heardIn.add(shadow);
    shadow.addEventListener('focusin', focused, true);
  }
}

function focused(event: Event) {
  // Heard once, though it may pass both a shadow root and the document.
  if (event === lastFocus) return;
  lastFocus = event;
  // The field itself, where it sits in a shadow root. Focus inside the menu's own closed root
  // still reads as the menu.
  const el = startOf(event);
  if (!alive() || (host && el === host)) return;
  if (el instanceof Element) listenIn(el);
  if (el instanceof HTMLInputElement && visible(el) && offersFill(el)) showKey(el);
  // Tabbing on to anything else takes the key and menu away with it, rather than leaving them
  // over a field no longer in use.
  else if (current) {
    clearMenu();
    current = null;
  }
}
document.addEventListener('focusin', focused, true);

document.addEventListener(
  'pointerdown',
  (event) => {
    if (host && event.composedPath().includes(host)) return;
    if (startOf(event) !== current) clearMenu();
  },
  true,
);

// A field focused before this script ran, by autofocus most often, had its focusin already.
if (alive()) {
  let el = document.activeElement;
  while (el?.shadowRoot?.activeElement) el = el.shadowRoot.activeElement;
  if (el) listenIn(el);
  if (el instanceof HTMLInputElement && visible(el) && offersFill(el)) showKey(el);
}

// When the page moves, a menu goes and the key follows its field, which focusing it often scrolls
// into view. Only a scroll that moves the field counts: a carousel elsewhere leaves both be.
let moving = 0;
function pageMoved(event: Event) {
  const target = event.target;
  if (current && target instanceof Element && !holds(target, current)) return;
  if (moving) return;
  moving = requestAnimationFrame(() => {
    moving = 0;
    if (current && rootOf(current).activeElement === current && visible(current)) showKey(current);
    else clearMenu();
  });
}
addEventListener('scroll', pageMoved, { passive: true, capture: true });
addEventListener('resize', pageMoved, { passive: true });

// ---- filling at the popup's request ----

chrome.runtime.onMessage.addListener((message: any, sender: any, respond: (r: unknown) => void) => {
  if (sender.id !== chrome.runtime.id || sender.tab) return;
  // Answered, so the popup can say when there was nothing on the page to fill. The login was
  // matched to the address this frame had when the popup looked: a frame that has since gone to
  // another site gets nothing, where the browser cannot pin the message to that one page.
  if (message?.type === 'fill') {
    if (typeof message.origin === 'string' && message.origin !== location.origin) respond({ filled: false });
    else respond({ filled: fillFields(current, message.fill) });
  }
  // After a sign-in that loaded no page: a single-page app, or a form in a frame.
  if (message?.type === 'offer.check' && window === window.top) void askOffer(true);
});

// ---- offering to save ----

let lastSubmitted = '';
let lastSubmittedAt = 0;
// One press of Sign in can reach here as a click, an Enter and a submit together: those count once.
// A send of the same login later, as after an offer timed out, counts again.
const SAME_SEND_MS = 5_000;

/**
 * The password a send would save. On a change-password form the first box holds the current
 * password: the new one is in the box that says so, or else the last, where it is typed again.
 */
function newPasswordIn(first: HTMLInputElement) {
  const boxes = inputsIn(first.form ?? document).filter((el) => isPassword(el) && visible(el));
  const says = boxes.find((el) => el.autocomplete === 'new-password' || /new/i.test(`${el.name} ${el.id}`));
  return says ?? (boxes.length >= 2 ? boxes[boxes.length - 1] : first);
}

function noticeSubmit(anchor: HTMLInputElement) {
  if (!alive()) return;
  const around = fieldsAround(anchor);
  // A form with no username to see, such as a change of password or the second step of a sign-in,
  // often still says whose it is in a hidden box kept for password managers.
  const username =
    around.username?.value ||
    Array.from((anchor.form ?? document).querySelectorAll<HTMLInputElement>('input[autocomplete~="username" i]')).find((el) => el.value)?.value ||
    '';
  const password = around.password && newPasswordIn(around.password);
  // The same password again with another username, as after a mistyped one, is another sign-in.
  const sent = `${username}\n${password?.value ?? ''}`;
  if (!password?.value || (sent === lastSubmitted && performance.now() - lastSubmittedAt < SAME_SEND_MS)) return;
  lastSubmitted = sent;
  lastSubmittedAt = performance.now();
  void send({ type: 'cs.submitted', username, password: password.value }).catch(() => {});
}

/**
 * Whether an email or username box with no password beside it is a sign-in's first step, and not
 * an invite, a share, a newsletter or a contact form, whose address is someone else's. Outside any
 * form, it must also say it is for signing in.
 */
function looksLikeSignInStep(field: HTMLInputElement, outsideForm: boolean, button: Element | null = null) {
  const form = field.form;
  const words = [
    field.name, field.id, field.autocomplete, field.placeholder, field.getAttribute('aria-label'),
    form?.id, form?.name, form?.getAttribute('action'), form?.className,
    // The button pressed says it too, as Subscribe does under a box named only "email".
    button?.id, button?.getAttribute('aria-label'), (button?.textContent || (button as HTMLInputElement | null)?.value || '').slice(0, 60),
  ].join(' ');
  if (/invite|share|subscri|newsletter|recipient|friend|contact|referr|gift|search/i.test(words)) return false;
  if (!outsideForm) return true;
  return /\busername\b|webauthn/i.test(field.autocomplete) || /login|log-in|signin|sign-in|sign_in|identifier|account|auth|user/i.test(words);
}

/**
 * The first step of a two-step sign-in sends a username with no password beside it. The
 * background keeps it a short while, for the password step after it, which has none to save.
 */
function noticeUsername(scope: ParentNode, button: Element | null = null) {
  if (!alive()) return;
  const field = inputsIn(scope).find((el) => looksLikeUsername(el) && el.value.trim() && visible(el));
  if (!field || fieldsAround(field).password || !looksLikeSignInStep(field, scope === document, button)) return;
  void send({ type: 'cs.username', username: field.value.trim() }).catch(() => {});
}

document.addEventListener(
  'submit',
  (event) => {
    const form = event.target;
    if (form instanceof HTMLFormElement) {
      // In the form's shadow roots too, where a web component keeps its password box.
      const field = inputsIn(form).find((el) => el.type === 'password' && el.value) ?? inputsIn(form).find((el) => el.type === 'password');
      if (field) noticeSubmit(field);
      noticeUsername(form, (event as SubmitEvent).submitter ?? null);
    }
  },
  true,
);

/**
 * A button beside the form that sends nothing: one that shows or hides the password, or one that
 * leads away, such as Forgot password. Pressed with a password typed, it would otherwise read as a
 * sign-in, and the next page would offer to save a login that never signed in. A button that
 * really sends its form is always a send, whatever it says.
 */
function showsOrLeaves(button: Element) {
  if ((button instanceof HTMLButtonElement || button instanceof HTMLInputElement) && button.type === 'submit' && button.form) return false;
  const words = [button.id, button.getAttribute('class'), button.getAttribute('aria-label'), button.getAttribute('title'), (button.textContent ?? '').slice(0, 60)].join(' ');
  return /\b(show|hide|reveal|peek|eye|forgot|cancel)\b|toggle|visibility/i.test(words);
}

document.addEventListener(
  'click',
  (event) => {
    const button = (startOf(event) as Element | null)?.closest?.('button, input[type=submit], [role=button]');
    if (!button || showsOrLeaves(button)) return;
    const scope = button.closest('form') ?? document;
    const field = inputsIn(scope).find((el) => el.type === 'password' && el.value);
    if (field) noticeSubmit(field);
    else noticeUsername(scope, button);
  },
  true,
);

// Enter in a password field outside any form sends nothing a submit listener could see, and nor
// does one in a form with no submit button, which the site sends from its own script.
document.addEventListener(
  'keydown',
  (event) => {
    if (event.key !== 'Enter' || !event.isTrusted) return;
    const field = startOf(event);
    if (isPassword(field as Element) && (field as HTMLInputElement).value) noticeSubmit(field as HTMLInputElement);
    else if (looksLikeUsername(field as Element)) noticeUsername((field as HTMLInputElement).form ?? document);
  },
  true,
);

function offerBar(offer: { kind: string; username: string; left?: number; at?: number }) {
  // Beside the field in use, so a modal dialog still open, as for a code after the password, has
  // the bar above it rather than under it, and keeps the key where it is.
  const box = ensureHost(current);
  const bar = document.createElement('div');
  bar.className = 'bar';
  // At the top right of the window, and no wider than a phone's.
  const from = origin();
  const across = document.documentElement.clientWidth || innerWidth;
  const width = Math.min(320, across - 24);
  bar.style.width = `${width}px`;
  bar.style.left = `${across - 12 - width - from.x}px`;
  bar.style.top = `${12 - from.y}px`;
  // Named, so a screen reader says whose question this is before the page's own content.
  bar.setAttribute('role', 'dialog');
  bar.setAttribute('aria-label', 'WM Pass');
  const words = document.createElement('div');
  // Said aloud when it comes, as nothing moves the focus to it.
  words.setAttribute('role', 'status');
  words.textContent = offer.kind === 'update' ? `Update the saved password for ${offer.username || 'this login'}?` : `Save this login${offer.username ? ` for ${offer.username}` : ''}?`;
  const actions = document.createElement('div');
  actions.className = 'actions';
  // Once Save is pressed, the bar stays until the save is done or its failure is read.
  let saving = false;
  const button = (label: string, answer: string, main = false) => {
    const b = document.createElement('button');
    b.className = main ? 'btn main' : 'btn';
    b.textContent = label;
    b.addEventListener('click', (event) => {
      if (!event.isTrusted) return;
      // Save changes the vault, so it gets the menu's checks: up long enough to be read, and
      // really seen where it was pressed, not faded or covered by the page to take a click.
      // A click from Enter or Space has no pointer to check.
      if (answer === 'save') {
        const pointed = event.detail === 0 || (host?.getRootNode() as Document | ShadowRoot | undefined)?.elementFromPoint(event.clientX, event.clientY) === host;
        if (performance.now() - barAt < HOVER_DELAY_MS || !pointed || !menuShown()) return;
      }
      if (answer !== 'save') {
        bar.remove();
        void send({ type: 'cs.offer.answer', answer, at: offer.at }).catch(() => {});
        return;
      }
      // Kept up until the save is done, so a failure is said rather than lost.
      saving = true;
      actions.querySelectorAll('button').forEach((each) => (each.disabled = true));
      words.textContent = 'Saving…';
      send({ type: 'cs.offer.answer', answer, at: offer.at })
        .then(() => bar.remove())
        .catch((failure: any) => {
          words.textContent = failure?.message ?? 'That could not be saved.';
          words.className = 'warn';
          actions.replaceChildren(button('Close', 'no'));
        });
    });
    return b;
  };
  actions.append(button('Never here', 'never'), button('Not now', 'no'), button(offer.kind === 'update' ? 'Update' : 'Save', 'save', true));
  bar.append(words, actions);
  box.append(bar);
  const barAt = performance.now();
  if (offer.left && offer.left > 0) setTimeout(() => saving || bar.remove(), offer.left);
}

// One bar at a time, though a single-page app may sign in more than once.
const barUp = () => !!root?.querySelector('.bar');

async function askOffer(nudged: boolean) {
  if (barUp()) return;
  // A page that still asks for a password after the submit is most likely the same form again,
  // after a wrong password: no offer then, or a typo would replace the saved password.
  const asksPassword = inputsIn(document).some((el) => el.type === 'password' && visible(el));
  // Asked again from the same page, a form still there may be a slow sign-in still on its way:
  // the next page decides.
  if (nudged && asksPassword) return;
  const offer = await send<{ kind: string; username: string; left: number; at: number } | null>({ type: 'cs.offer', asksPassword }).catch(() => null);
  if (offer && !barUp()) offerBar(offer);
}

if (window === window.top && alive()) void askOffer(false);
