// Forwarding services. A service's key lives in the vault itself: it is a secret like any other.

import { bytesOf, consumeText, pass } from '../engine/pass.svelte';

export interface AliasService {
  id: string;
  name: string;
  note: string;
  keyTitle: string;
}

export const ALIAS_SERVICES: AliasService[] = [
  { id: 'simplelogin', name: 'SimpleLogin', note: 'Create an API key under API Keys in your SimpleLogin account.', keyTitle: 'SimpleLogin API key' },
  { id: 'addy', name: 'addy.io', note: 'Create an API key in addy.io’s settings.', keyTitle: 'addy.io API key' },
  { id: 'firefox-relay', name: 'Firefox Relay', note: 'Copy the API key from your Firefox Relay settings.', keyTitle: 'Firefox Relay API key' },
  { id: 'duckduckgo', name: 'DuckDuckGo', note: 'Experimental. DuckDuckGo has no public API, so this uses the private one its extension calls. Copy the token from your Email Protection dashboard. It may stop working.', keyTitle: 'DuckDuckGo Email token' },
];

/** The key for a service, as bytes the caller wipes. */
export async function serviceKey(title: string): Promise<Uint8Array | null> {
  const entry = pass.entries.find((e) => e.kind === 'api' && e.title === title);
  if (!entry) return null;
  return bytesOf(await pass.reveal('entry.reveal', { id: entry.id, field: 'key' }));
}

export async function saveServiceKey(title: string, value: string) {
  const id = (await pass.change<{ id: string }>('entry.create', { kind: 'api', title })).json.id;
  const bytes = bytesOf(value);
  await pass.change('entry.set_secret', { id, field: 'key' }, { secret: bytes });
  bytes.fill(0);
}

export { consumeText };
