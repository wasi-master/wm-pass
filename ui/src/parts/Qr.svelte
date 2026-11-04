<script lang="ts">
  import { pass } from '../engine/pass.svelte';
  import { bytesOf } from '../engine/frame';

  // A QR code, drawn here from the engine's matrix. Nothing leaves this device to make it.
  // `from` asks the open vault for a code it builds itself, so a secret in it never comes out as text.
  let { text = '', from, size = 220 }: { text?: string; from?: { method: string; args: unknown }; size?: number } = $props();
  let matrix = $state<{ size: number; rows: string } | null>(null);
  let tooLong = $state(false);

  $effect(() => {
    if (from) {
      pass
        .view<{ size: number; rows: string }>(from.method, from.args)
        .then((view) => {
          matrix = view;
          tooLong = false;
        })
        .catch(() => (tooLong = true));
      return;
    }
    const bytes = bytesOf(text);
    pass
      .engine<{ size: number; rows: string }>('qr.encode', {}, { secret: bytes })
      .then((reply) => {
        matrix = reply.json;
        tooLong = false;
      })
      .catch(() => (tooLong = true))
      .finally(() => bytes.fill(0));
  });

  const path = $derived.by(() => {
    if (!matrix) return '';
    let d = '';
    for (let y = 0; y < matrix.size; y++) {
      for (let x = 0; x < matrix.size; x++) {
        if (matrix.rows[y * matrix.size + x] === '1') d += `M${x} ${y}h1v1h-1z`;
      }
    }
    return d;
  });
</script>

{#if matrix}
  <svg viewBox="-3 -3 {matrix.size + 6} {matrix.size + 6}" width={size} height={size} role="img" aria-label="QR code" style="background:#fff;border-radius:var(--r-m);display:block;shape-rendering:crispEdges">
    <path d={path} fill="#000" />
  </svg>
{:else if tooLong}
  <p class="dim small">This is too long for a QR code. Copy it instead.</p>
{/if}
