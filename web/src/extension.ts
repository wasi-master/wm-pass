// The WM Pass extension, when this browser has it. It answers this site and no other, and says
// nothing about any vault: only that it is there, and, when asked, it opens its own vault page in
// this tab. That page's code comes signed from the browser's store, not from this site.

const runtime = () => (globalThis as any).chrome?.runtime;

/** The extension's ids: the store's, which the manifest key fixes, and any a build adds. */
const IDS = ['blpbdcnicidapbjjopepnaofggclabom', ...String(import.meta.env.VITE_WMPASS_EXTENSION_IDS ?? '').split(',').filter(Boolean)];

function ask(id: string, message: unknown): Promise<any> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), 800);
    try {
      runtime().sendMessage(id, message, (reply: any) => {
        clearTimeout(timer);
        void runtime().lastError;
        resolve(reply ?? null);
      });
    } catch {
      clearTimeout(timer);
      resolve(null);
    }
  });
}

/** The id of the extension in this browser, or null. */
export async function findExtension(): Promise<string | null> {
  if (!runtime()?.sendMessage) return null;
  for (const id of IDS) if ((await ask(id, { type: 'web.hello' }))?.ok) return id;
  return null;
}

/** Swaps this tab for the extension's own vault page. */
export function openInExtension(id: string) {
  return ask(id, { type: 'web.open' });
}
