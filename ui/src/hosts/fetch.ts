// The `fetch` the WebAssembly session sends its requests through. It omits cookies and does not
// follow redirects, so a key sent to a service never travels on to wherever that service points;
// which addresses are allowed at all is the Rust policy check's to say.

export async function wasmFetch(method: string, url: string, headers: [string, string][], body: Uint8Array | null, timeoutMs: number) {
  const stop = new AbortController();
  const timer = setTimeout(() => stop.abort(), timeoutMs || 120_000);
  try {
    const reply = await fetch(url, {
      method,
      headers,
      body: body as BodyInit | null,
      credentials: 'omit',
      redirect: 'manual',
      cache: 'no-store',
      referrerPolicy: 'no-referrer',
      signal: stop.signal,
    });
    return {
      status: reply.status,
      headers: Array.from(reply.headers.entries()),
      body: new Uint8Array(await reply.arrayBuffer()),
      retry_after: Number(reply.headers.get('retry-after')) || undefined,
    };
  } finally {
    clearTimeout(timer);
  }
}
