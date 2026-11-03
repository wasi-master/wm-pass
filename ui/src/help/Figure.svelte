<script lang="ts">
  import type { Figure } from '$help';
  import Icon from '../parts/Icon.svelte';
  import { help } from './help.svelte';

  // A diagram from the guide: rows of plain boxes, with arrows drawn between them once the boxes
  // have found their size in the current font. Nothing moves.
  let { id }: { id: string } = $props();

  // Only drawn inside the guide, so the content is already loaded; importing it here would put the
  // whole guide in the main bundle.
  const figure = $derived(help.content?.figureById(id) as Figure | undefined);

  let frame = $state<HTMLElement>();
  type Line = { d: string; dashed: boolean; label?: string; x: number; y: number; side: boolean };
  let lines = $state<Line[]>([]);
  let size = $state({ w: 0, h: 0 });

  function measure() {
    if (!frame || !figure) return;
    const base = frame.getBoundingClientRect();
    const box = (name: string) => frame!.querySelector<HTMLElement>(`[data-box="${CSS.escape(name)}"]`)?.getBoundingClientRect();
    const out: Line[] = [];
    for (const edge of figure.edges) {
      const a = box(edge.from);
      const b = box(edge.to);
      if (!a || !b) continue;
      const sameRow = Math.abs(a.top - b.top) < 4;
      if (sameRow) {
        const leftToRight = a.left < b.left;
        const x1 = (leftToRight ? a.right : a.left) - base.left;
        const x2 = (leftToRight ? b.left : b.right) - base.left;
        const y = a.top + a.height / 2 - base.top;
        out.push({ d: `M${x1} ${y}H${x2}`, dashed: !!edge.dashed, label: edge.label, x: (x1 + x2) / 2, y: y - 4, side: false });
      } else {
        const down = a.top < b.top;
        const x1 = a.left + a.width / 2 - base.left;
        const x2 = b.left + b.width / 2 - base.left;
        const y1 = (down ? a.bottom : a.top) - base.top;
        const y2 = (down ? b.top : b.bottom) - base.top;
        const mid = (y1 + y2) / 2;
        const d = Math.abs(x1 - x2) < 2 ? `M${x1} ${y1}V${y2}` : `M${x1} ${y1}V${mid}H${x2}V${y2}`;
        out.push({ d, dashed: !!edge.dashed, label: edge.label, x: x2 + 6, y: mid, side: true });
      }
    }
    lines = out;
    size = { w: base.width, h: base.height };
  }

  $effect(() => {
    void figure;
    if (!frame) return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(frame);
    measure();
    return () => observer.disconnect();
  });
</script>

{#if figure}
  <figure class="figure" aria-label={figure.caption}>
    <div class="figure-frame" bind:this={frame}>
      {#each figure.rows as row, i (i)}
        <div class="figure-row">
          {#each row as box (box.id)}
            <div class="figure-box {box.tone ?? ''}" data-box={box.id}>
              {#if box.icon}<span class="figure-icon"><Icon name={box.icon} size={16} /></span>{/if}
              <span class="figure-label">{box.label}</span>
              {#if box.sub}<span class="figure-sub">{box.sub}</span>{/if}
            </div>
          {/each}
        </div>
      {/each}
      <svg class="figure-lines" width={size.w} height={size.h} viewBox="0 0 {size.w} {size.h}" aria-hidden="true">
        <defs>
          <marker id="arrow-{id}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M0 0L8 4L0 8z" fill="currentColor" />
          </marker>
        </defs>
        {#each lines as line}
          <path d={line.d} fill="none" stroke="currentColor" stroke-width="1.3" stroke-dasharray={line.dashed ? '4 3' : undefined} marker-end="url(#arrow-{id})" />
        {/each}
      </svg>
      {#each lines as line}
        {#if line.label}
          <span class="figure-edge" class:side={line.side} style="left:{line.x}px;top:{line.y}px">{line.label}</span>
        {/if}
      {/each}
    </div>
    <figcaption>{figure.caption}</figcaption>
  </figure>
{/if}
