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
  return chrome.runtime.sendMessage(message).then((reply: any) => {
    if (reply?.failed) throw new Error(reply.failed);
    return reply?.ok as T;
  });
}

// ---- passkeys, relayed from the page's world ----

window.addEventListener('message', (event) => {
  if (event.source !== window || !event.data?.wmpassAsk) return;
  const { wmpassAsk: id, op, options } = event.data;
  send({ type: 'cs.passkey', op, options })
    .catch(() => ({ fallback: true }))
    .then((answer) => window.postMessage({ wmpassReply: id, answer }, location.origin));
});

// ---- finding fields ----

function visible(el: HTMLElement) {
  const rect = el.getBoundingClientRect();
  if (rect.width < 8 || rect.height < 8) return false;
  for (let node: HTMLElement | null = el; node; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (style.visibility === 'hidden' || style.display === 'none' || Number(style.opacity) < 0.2) return false;
  }
  return true;
}

/** Whether a person can see and is pointing at this thing, not something laid over it. */
function onTop(el: Element, x: number, y: number) {
  const top = document.elementFromPoint(x, y);
  return top === el || el.contains(top) || (top instanceof Element && top.contains(el) && top !== document.documentElement);
}

const textTypes = new Set(['text', 'email', 'tel', '']);

function isPassword(el: Element): el is HTMLInputElement {
  return el instanceof HTMLInputElement && el.type === 'password' && !el.disabled && !el.readOnly;
}

function looksLikeUsername(el: Element): el is HTMLInputElement {
  if (!(el instanceof HTMLInputElement) || !textTypes.has(el.type) || el.disabled || el.readOnly) return false;
  const words = `${el.name} ${el.id} ${el.autocomplete} ${el.placeholder} ${el.getAttribute('aria-label') ?? ''}`.toLowerCase();
  if (/search|query|coupon|captcha|otp|code/.test(words)) return false;
  return /user|login|email|e-mail|account|identifier|phone|mail/.test(words) || el.type === 'email';
}

function looksLikeCode(el: Element): el is HTMLInputElement {
  if (!(el instanceof HTMLInputElement) || !textTypes.has(el.type) && el.type !== 'number' || el.disabled || el.readOnly) return false;
  const words = `${el.name} ${el.id} ${el.autocomplete} ${el.placeholder}`.toLowerCase();
  return el.autocomplete === 'one-time-code' || /\b(otp|totp|2fa|mfa|two.?factor|verification.?code|auth.?code)\b/.test(words);
}

function fieldsAround(anchor: HTMLInputElement) {
  const scope: ParentNode = anchor.form ?? document;
  const inputs = Array.from(scope.querySelectorAll('input')).filter((el) => visible(el));
  const password = inputs.find(isPassword) ?? null;
  let username: HTMLInputElement | null = null;
  if (password) {
    const before = inputs.slice(0, inputs.indexOf(password)).reverse();
    username = before.find(looksLikeUsername) ?? before.find((el) => el instanceof HTMLInputElement && textTypes.has(el.type)) ?? null;
  } else {
    username = inputs.find(looksLikeUsername) ?? null;
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

function fillFields(anchor: HTMLInputElement | null, fill: { username: string; password: string; code: string | null }) {
  const start = anchor ?? (Array.from(document.querySelectorAll('input')).find((el) => visible(el) && (isPassword(el) || looksLikeUsername(el))) as HTMLInputElement | undefined);
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
.menu { width: 300px; max-height: 280px; overflow-y: auto; padding: 4px; }
.row { display: block; width: 100%; padding: 6px 8px; border: 0; border-radius: 4px; background: transparent; color: inherit; text-align: left; cursor: pointer; }
.row:hover, .row:focus-visible { background: rgba(13, 148, 136, .14); outline: 0; }
.title { display: block; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sub { display: block; color: #5f5f5f; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.note { padding: 6px 8px; color: #5f5f5f; }
.warn { padding: 6px 8px; color: #a04b00; }
.bar { top: 12px; right: 12px; width: 320px; padding: 10px 12px; display: grid; gap: 8px; }
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

const KEY_ICON = '<svg viewBox="0 -960 960 960" aria-hidden="true"><path d="M280-400q-33 0-56.5-23.5T200-480q0-33 23.5-56.5T280-560q33 0 56.5 23.5T360-480q0 33-23.5 56.5T280-400Zm0 160q-100 0-170-70T40-480q0-100 70-170t170-70q67 0 121.5 33t86.5 87h352l120 120-180 180-80-60-80 60-85-60h-47q-32 54-86.5 87T280-240Zm0-80q56 0 98.5-34t56.5-86h125l58 41 82-61 71 55 75-75-40-40H435q-14-52-56.5-86T280-640q-66 0-113 47t-47 113q0 66 47 113t113 47Z"/></svg>';

let host: HTMLElement | null = null;
let root: ShadowRoot | null = null;
let current: HTMLInputElement | null = null;
let shownAt = 0;

function ensureHost() {
  if (host?.isConnected && root) return root;
  host = document.createElement('wm-pass-menu');
  // Closed, so the page's scripts cannot read or press what is inside.
  root = host.attachShadow({ mode: 'closed' });
  const style = document.createElement('style');
  style.textContent = MENU_CSS;
  root.append(style);
  host.style.cssText = 'all: initial !important; position: fixed !important; inset: 0 auto auto 0 !important; z-index: 2147483647 !important; opacity: 1 !important; transform: none !important; filter: none !important; pointer-events: auto !important; display: block !important;';
  document.documentElement.append(host);
  return root;
}

function clearMenu() {
  root?.querySelectorAll('.key, .menu').forEach((el) => el.remove());
}

function positionNear(el: HTMLElement, anchor: HTMLElement) {
  const rect = anchor.getBoundingClientRect();
  el.style.left = `${Math.min(rect.left, innerWidth - 310)}px`;
  el.style.top = `${rect.bottom + 4 + 280 > innerHeight ? Math.max(4, rect.top - 284) : rect.bottom + 4}px`;
}

/** Everything a click must pass before anything is filled. */
function safeClick(event: MouseEvent, field: HTMLInputElement) {
  if (!event.isTrusted) return false;
  if (performance.now() - shownAt < HOVER_DELAY_MS) return false;
  if (!visible(field)) return false;
  const rect = field.getBoundingClientRect();
  if (!onTop(field, rect.left + rect.width / 2, rect.top + rect.height / 2) && !(document.activeElement === field)) return false;
  if (!host || document.elementFromPoint(event.clientX, event.clientY) !== host) return false;
  for (let node: Element | null = document.documentElement; node; node = node === document.documentElement ? document.body : null) {
    const style = getComputedStyle(node);
    if (Number(style.opacity) < 0.95 || style.filter !== 'none') return false;
  }
  return true;
}

function note(menu: HTMLElement, text: string, kind = 'note') {
  const line = document.createElement('div');
  line.className = kind;
  line.textContent = text;
  menu.append(line);
}

async function openMenu(field: HTMLInputElement) {
  const box = ensureHost();
  clearMenu();
  const menu = document.createElement('div');
  menu.className = 'menu';
  menu.setAttribute('role', 'listbox');
  positionNear(menu, field);
  box.append(menu);
  shownAt = performance.now();
  const answer = await send<{ source: string; locked: boolean; rows: Row[]; foreign: string | null }>({ type: 'cs.site' }).catch(() => null);
  if (!answer) {
    note(menu, 'WM Pass could not answer.');
    return;
  }
  if (answer.locked) {
    const open = document.createElement('button');
    open.className = 'row';
    open.textContent = 'WM Pass is locked. Open it';
    open.addEventListener('click', (event) => {
      if (event.isTrusted) void send({ type: 'cs.open' });
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
    button.setAttribute('role', 'option');
    const title = document.createElement('span');
    title.className = 'title';
    title.textContent = row.title;
    const sub = document.createElement('span');
    sub.className = 'sub';
    sub.textContent = row.subtitle;
    button.append(title, sub);
    button.addEventListener('click', async (event) => {
      if (!safeClick(event, field)) return;
      clearMenu();
      try {
        const { fill } = await send<{ fill: any }>({ type: 'cs.fill', id: row.id });
        fillFields(field, fill);
      } catch (failure: any) {
        const after = ensureHost();
        const message = document.createElement('div');
        message.className = 'menu';
        positionNear(message, field);
        note(message, failure.message === 'refused' ? 'Not filled.' : failure.message, 'warn');
        after.append(message);
      }
    });
    menu.append(button);
  }
}

function showKey(field: HTMLInputElement) {
  const box = ensureHost();
  clearMenu();
  current = field;
  const key = document.createElement('button');
  key.className = 'key';
  key.title = 'Fill from WM Pass';
  key.setAttribute('aria-label', 'Fill from WM Pass');
  key.innerHTML = KEY_ICON;
  const rect = field.getBoundingClientRect();
  key.style.left = `${rect.right - 26}px`;
  key.style.top = `${rect.top + (rect.height - 22) / 2}px`;
  key.addEventListener('mousedown', (event) => event.preventDefault());
  key.addEventListener('click', (event) => {
    if (event.isTrusted) void openMenu(field);
  });
  box.append(key);
}

document.addEventListener(
  'focusin',
  (event) => {
    const el = event.target;
    if (!(el instanceof HTMLInputElement)) return;
    if ((isPassword(el) || looksLikeUsername(el) || looksLikeCode(el)) && visible(el)) showKey(el);
  },
  true,
);

document.addEventListener(
  'pointerdown',
  (event) => {
    if (host && event.composedPath().includes(host)) return;
    if (event.target !== current) clearMenu();
  },
  true,
);

addEventListener('scroll', () => clearMenu(), { passive: true, capture: true });
addEventListener('resize', () => clearMenu(), { passive: true });

// ---- filling at the popup's request ----

chrome.runtime.onMessage.addListener((message: any, sender: any) => {
  if (sender.id !== chrome.runtime.id || sender.tab) return;
  if (message?.type === 'fill') fillFields(current, message.fill);
});

// ---- offering to save ----

let lastSubmitted = '';

function noticeSubmit(anchor: HTMLInputElement) {
  const { username, password } = fieldsAround(anchor);
  if (!password?.value || password.value === lastSubmitted) return;
  lastSubmitted = password.value;
  void send({ type: 'cs.submitted', username: username?.value ?? '', password: password.value }).catch(() => {});
}

document.addEventListener(
  'submit',
  (event) => {
    const form = event.target;
    if (form instanceof HTMLFormElement) {
      const field = form.querySelector('input[type=password]');
      if (field instanceof HTMLInputElement) noticeSubmit(field);
    }
  },
  true,
);

document.addEventListener(
  'click',
  (event) => {
    const button = (event.target as Element | null)?.closest?.('button, input[type=submit], [role=button]');
    if (!button) return;
    const field = (button.closest('form') ?? document).querySelector('input[type=password]');
    if (field instanceof HTMLInputElement && field.value) noticeSubmit(field);
  },
  true,
);

function offerBar(offer: { kind: string; username: string }) {
  const box = ensureHost();
  const bar = document.createElement('div');
  bar.className = 'bar';
  const words = document.createElement('div');
  words.textContent = offer.kind === 'update' ? `Update the saved password for ${offer.username || 'this login'}?` : `Save this login${offer.username ? ` for ${offer.username}` : ''}?`;
  const actions = document.createElement('div');
  actions.className = 'actions';
  const button = (label: string, answer: string, main = false) => {
    const b = document.createElement('button');
    b.className = main ? 'btn main' : 'btn';
    b.textContent = label;
    b.addEventListener('click', (event) => {
      if (!event.isTrusted) return;
      bar.remove();
      void send({ type: 'cs.offer.answer', answer });
    });
    return b;
  };
  actions.append(button('Never here', 'never'), button('Not now', 'no'), button(offer.kind === 'update' ? 'Update' : 'Save', 'save', true));
  bar.append(words, actions);
  box.append(bar);
}

if (window === window.top) {
  void send<{ kind: string; username: string } | null>({ type: 'cs.offer' })
    .then((offer) => {
      if (offer) offerBar(offer);
    })
    .catch(() => {});
}
