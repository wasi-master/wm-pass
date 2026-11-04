// Kept throwaway addresses: what the vault holds for each, the services they come from, and the
// small things done with a mail once it is open.

import { pass } from '../engine/pass.svelte';

/** A throwaway address the vault keeps until it is closed, like a chat kept in a list. */
export interface KeptInbox {
  id: string;
  provider: string;
  base: string;
  address: string;
  label: string;
  created_ms: number;
}

export interface MailRow {
  id: string;
  from: string;
  subject: string;
  preview: string;
  at_ms: number;
}

export interface Mail {
  from: string;
  to: string;
  subject: string;
  text: string;
  html: string | null;
  at_ms: number;
}

export interface Provider {
  id: string;
  name: string;
  about: string;
  /** A server the person runs, at an address they give. */
  own?: boolean;
}

export const PROVIDERS: Provider[] = [
  { id: 'mailtm', name: 'mail.tm', about: 'Free and public.' },
  { id: 'mailgw', name: 'mail.gw', about: 'Free and public, on other domains than mail.tm.' },
  { id: 'guerrilla', name: 'Guerrilla Mail', about: 'Free and public. It deletes mail after an hour.' },
  { id: 'custom', name: 'Your own mail.tm server', about: 'Any server that speaks the mail.tm API, at the address you give.', own: true },
  { id: 'inbucket', name: 'Inbucket', about: 'A test mail server you run. Every address on its domain lands there.', own: true },
];

export const INBOX_WARNING =
  'Makes throwaway addresses on a mail service, for sign-ups that only need a confirmation mail. The service, and possibly others, can read what arrives. Never use one for an account you want to keep.';

export function providerName(id: string) {
  return PROVIDERS.find((p) => p.id === id)?.name ?? id;
}

export function isOwn(id: string) {
  return PROVIDERS.find((p) => p.id === id)?.own === true;
}

export function titleOf(inbox: KeptInbox) {
  return inbox.label || inbox.address;
}

export async function keptInboxes(): Promise<KeptInbox[]> {
  return (await pass.net<KeptInbox[]>('inbox.sessions', false)).json;
}

/** An engine message as a sentence. */
export function sentence(message: unknown) {
  const text = String(message ?? '') || 'No answer';
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** Today's mail by the clock, older mail by the date. */
export function mailTime(ms: number) {
  if (ms <= 0) return '';
  const then = new Date(ms);
  const now = new Date();
  const today = then.toDateString() === now.toDateString();
  return today ? then.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }) : then.toLocaleDateString(undefined, { dateStyle: 'medium' });
}

export function mailDate(mail: Mail) {
  return mail.at_ms > 0 ? new Date(mail.at_ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '';
}

/** Numbers that look like a sign-up code: four to eight digits standing alone, and not a year. */
export function codesIn(mail: Mail): string[] {
  const pattern = /(?<![\d.,:/-])\d{4,8}(?![\d.,:/-])/g;
  const found = [...mail.subject.matchAll(pattern), ...mail.text.matchAll(pattern)]
    .map((m) => m[0])
    .filter((code) => !(code.length === 4 && Number(code) >= 1900 && Number(code) <= 2100));
  return [...new Set(found)].slice(0, 3);
}

/** Headers, then the text: for the vault, the clipboard and a forward. */
export function mailAsText(mail: Mail) {
  const lines = [`From: ${mail.from}`];
  if (mail.to) lines.push(`To: ${mail.to}`);
  if (mail.at_ms > 0) lines.push(`Date: ${mailDate(mail)}`);
  if (mail.subject) lines.push(`Subject: ${mail.subject}`);
  return `${lines.join('\n')}\n\n${mail.text}`;
}

export function mailFileName(mail: Mail) {
  const stem =
    mail.subject
      .replace(/[^\p{L}\p{N} _-]/gu, '')
      .trim()
      .slice(0, 60) || 'mail';
  return mail.html ? `${stem}.html` : `${stem}.txt`;
}

function escapeHtml(text: string) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

/** The file a mail is saved as: its HTML under a header block, or its text. */
export function mailFile(mail: Mail) {
  if (!mail.html) return mailAsText(mail);
  const head = [
    ['From', mail.from],
    ['To', mail.to],
    ['Date', mailDate(mail)],
    ['Subject', mail.subject],
  ]
    .filter(([name, value]) => value || name === 'From' || name === 'Subject')
    .map(([name, value]) => `<div><b>${name}:</b> ${escapeHtml(value)}</div>`)
    .join('');
  return (
    `<!doctype html><meta charset="utf-8"><title>${escapeHtml(mail.subject)}</title>` +
    `<div style="font:14px sans-serif;border-bottom:1px solid #ccc;padding:8px 0;margin-bottom:12px">${head}</div>${mail.html}`
  );
}

/** A mail app's forward, handed to the mail program the system uses. */
export function forwardLink(mail: Mail) {
  // Long bodies are cut, since some systems refuse a very long address.
  const body = mailAsText(mail);
  const short = body.length > 1800 ? `${body.slice(0, 1800)}\n…` : body;
  return `mailto:?subject=${encodeURIComponent(`Fwd: ${mail.subject}`)}&body=${encodeURIComponent(short)}`;
}

/**
 * A mail's HTML made inert before it is drawn: nothing that runs, nothing that loads by itself,
 * nothing that names another place to go. The frame it goes in adds its own walls.
 */
export function framedMail(html: string, pictures: boolean) {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  doc.querySelectorAll('script, noscript, iframe, frame, frameset, object, embed, applet, base, link, meta, portal, template').forEach((node) => node.remove());
  for (const node of doc.querySelectorAll('*')) {
    for (const attribute of [...node.attributes]) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith('on') || name === 'ping' || name === 'formaction' || name === 'action') node.removeAttribute(attribute.name);
    }
  }
  const images = pictures ? 'https: data:' : 'data:';
  const head =
    `<meta charset="utf-8">` +
    `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src ${images}; form-action 'none'; base-uri 'none'">` +
    `<meta http-equiv="x-dns-prefetch-control" content="off">` +
    `<meta name="referrer" content="no-referrer">` +
    `<style>html,body{margin:0;background:#fff;color:#111}body{padding:8px;font:14px/1.45 -apple-system,system-ui,sans-serif;word-wrap:break-word;overflow-wrap:anywhere}` +
    `img{max-width:100% !important;height:auto !important}table{max-width:100% !important}td,th{word-break:break-word}pre{white-space:pre-wrap}</style>`;
  return `<!doctype html><html><head>${head}${doc.head.innerHTML}</head>${doc.body.outerHTML}</html>`;
}
