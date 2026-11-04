<script lang="ts">
  import { host } from '../engine/host';
  import { framedMail } from './inbox';

  // A mail's HTML, drawn the way a mail app draws it, minus what lets a sender act. The frame's
  // sandbox lets no script run, no form send and no window open; the page inside carries a policy
  // that loads nothing from anywhere until pictures are allowed, and then only https pictures.
  // The frame shares this page's origin only so this page can reach in: it takes each click on a
  // link and hands the address to the browser, and it sizes the frame to what is drawn.
  let { html, pictures }: { html: string; pictures: boolean } = $props();

  let frame = $state<HTMLIFrameElement>();
  let height = $state(120);
  const doc = $derived(framedMail(html, pictures));

  function loaded() {
    const inner = frame?.contentDocument;
    if (!inner) return;
    inner.addEventListener('click', (event) => {
      const link = (event.target as Element | null)?.closest?.('a[href]');
      event.preventDefault();
      if (!link) return;
      const address = link.getAttribute('href')?.trim() ?? '';
      if (/^(https?:|mailto:)/i.test(address)) void host().openUrl(address).catch(() => undefined);
    });
    inner.addEventListener('submit', (event) => event.preventDefault());
    const fit = () => (height = Math.max(60, inner.documentElement.scrollHeight));
    fit();
    const watch = new ResizeObserver(fit);
    watch.observe(inner.documentElement);
    frame?.addEventListener('load', () => watch.disconnect(), { once: true });
  }
</script>

<iframe
  bind:this={frame}
  class="mail-frame"
  title="The mail"
  sandbox="allow-same-origin"
  referrerpolicy="no-referrer"
  srcdoc={doc}
  style="height:{height}px"
  onload={loaded}
></iframe>

<style>
  .mail-frame {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 10px;
    background: #fff;
  }
</style>
