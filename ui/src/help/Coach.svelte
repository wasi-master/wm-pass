<script lang="ts">
  import { tick } from 'svelte';
  import type { Step } from '$help';
  import Btn from '../parts/Btn.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import { pass } from '../engine/pass.svelte';
  import { ui } from '../vault/ui.svelte';
  import Rich from './Rich.svelte';
  import { help } from './help.svelte';

  // A tour: the window dims except for one part of it, and a note beside that part says what it is.
  // Four bands make the dim around a clear hole, rather than a clip path, because the three
  // webviews this runs in disagree about clicks through clipped shapes. Nothing moves unless the
  // highlighted part does.

  const PAD = 6;
  const tour = $derived(help.tour);
  const step = $derived<Step | null>(tour ? (tour.steps[help.step] ?? null) : null);
  const last = $derived(tour ? help.step >= tour.steps.length - 1 : true);

  let rect = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  let view = $state({ w: window.innerWidth, h: window.innerHeight });
  let card = $state<HTMLElement>();
  let cardSize = $state({ w: 300, h: 140 });
  let next = $state<HTMLButtonElement>();
  let anchor: HTMLElement | null = null;
  let openedEditor = false;
  let observer: ResizeObserver | null = null;
  let frame = 0;

  function find(id: string) {
    return document.querySelector<HTMLElement>(`[data-help-anchor="${CSS.escape(id)}"]`);
  }

  async function waitFor(id: string, ms = 600) {
    const until = performance.now() + ms;
    for (;;) {
      const found = find(id);
      if (found && found.getClientRects().length > 0) return found;
      if (performance.now() > until) return null;
      await new Promise((r) => requestAnimationFrame(() => r(null)));
    }
  }

  function measure() {
    view = { w: window.innerWidth, h: window.innerHeight };
    if (!anchor || !anchor.isConnected) {
      rect = null;
      return;
    }
    const box = anchor.getBoundingClientRect();
    const x = Math.max(4, box.left - PAD);
    const y = Math.max(4, box.top - PAD);
    rect = { x, y, w: Math.min(view.w - 4, box.right + PAD) - x, h: Math.min(view.h - 4, box.bottom + PAD) - y };
  }

  function later() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(measure);
  }

  async function prepare(s: Step) {
    if (s.before?.go && ui.section !== s.before.go) ui.go(s.before.go);
    if (s.before?.open === 'sample') {
      const first = pass.entries[0];
      if (first && ui.selected !== first.id) ui.open(first.id);
    }
    if (s.before?.open === 'editor' && !ui.editing) {
      ui.add('login');
      openedEditor = true;
    }
    await tick();
  }

  // Every time the step changes: get the window ready, find the part, and point at it.
  $effect(() => {
    const s = step;
    const index = help.step;
    if (!s) return;
    let stale = false;
    void (async () => {
      await prepare(s);
      // A dialog sits above everything, so the sheet and the welcome are closed by now; give them
      // a frame to go.
      await new Promise((r) => requestAnimationFrame(() => r(null)));
      const found = await waitFor(s.anchor);
      if (stale) return;
      if (!found) {
        // A part that is not on screen here, such as the sidebar in a narrow window, is skipped.
        if (index < (tour?.steps.length ?? 0) - 1) help.step = index + 1;
        else finish(true);
        return;
      }
      anchor = found;
      found.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      observer?.disconnect();
      observer = new ResizeObserver(later);
      observer.observe(found);
      measure();
      await tick();
      next?.focus();
    })();
    return () => {
      stale = true;
    };
  });

  $effect(() => {
    if (!tour) return;
    window.addEventListener('resize', later);
    document.addEventListener('scroll', later, true);
    return () => {
      window.removeEventListener('resize', later);
      document.removeEventListener('scroll', later, true);
      observer?.disconnect();
      observer = null;
      anchor = null;
      rect = null;
    };
  });

  $effect(() => {
    if (card) cardSize = { w: card.offsetWidth, h: card.offsetHeight };
  });

  function finish(done: boolean) {
    if (openedEditor && ui.editing && !ui.editorDirty) ui.editing = null;
    openedEditor = false;
    help.endTour(done);
  }

  function go(by: number) {
    if (!tour) return;
    const to = help.step + by;
    if (to < 0) return;
    if (to >= tour.steps.length) finish(true);
    else help.step = to;
  }

  function keyed(event: KeyboardEvent) {
    if (!tour) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      finish(false);
      return;
    }
    if (event.key === 'Tab' && card && !step?.tryIt) {
      // Keep the keyboard in the note while the rest of the window is dimmed.
      const stops = [...card.querySelectorAll<HTMLElement>('button:not(:disabled)')];
      if (stops.length === 0) return;
      const at = stops.indexOf(document.activeElement as HTMLElement);
      event.preventDefault();
      const to = event.shiftKey ? (at <= 0 ? stops.length - 1 : at - 1) : at === stops.length - 1 ? 0 : at + 1;
      stops[to].focus();
    }
  }

  /** Where the note goes: beside the part, on whichever side has room. */
  const place = $derived.by(() => {
    if (!rect) return { left: (view.w - cardSize.w) / 2, top: (view.h - cardSize.h) / 2 };
    const gap = 12;
    const room = {
      bottom: view.h - (rect.y + rect.h) - gap,
      top: rect.y - gap,
      right: view.w - (rect.x + rect.w) - gap,
      left: rect.x - gap,
    };
    const fits = (side: keyof typeof room) => (side === 'top' || side === 'bottom' ? room[side] >= cardSize.h : room[side] >= cardSize.w);
    const order: (keyof typeof room)[] = [step?.placement ?? 'bottom', 'bottom', 'top', 'right', 'left'];
    const side = order.find(fits) ?? 'bottom';
    const clampX = (x: number) => Math.min(Math.max(8, x), view.w - cardSize.w - 8);
    const clampY = (y: number) => Math.min(Math.max(8, y), view.h - cardSize.h - 8);
    if (side === 'bottom') return { left: clampX(rect.x), top: clampY(rect.y + rect.h + gap) };
    if (side === 'top') return { left: clampX(rect.x), top: clampY(rect.y - gap - cardSize.h) };
    if (side === 'right') return { left: clampX(rect.x + rect.w + gap), top: clampY(rect.y) };
    return { left: clampX(rect.x - gap - cardSize.w), top: clampY(rect.y) };
  });
</script>

<svelte:window onkeydowncapture={keyed} />

{#if tour && step}
  <div class="coach" aria-hidden="false">
    {#if rect}
      <div class="coach-band" style="left:0;top:0;width:100%;height:{rect.y}px"></div>
      <div class="coach-band" style="left:0;top:{rect.y + rect.h}px;width:100%;bottom:0"></div>
      <div class="coach-band" style="left:0;top:{rect.y}px;width:{rect.x}px;height:{rect.h}px"></div>
      <div class="coach-band" style="left:{rect.x + rect.w}px;top:{rect.y}px;right:0;height:{rect.h}px"></div>
      {#if !step.tryIt}<div class="coach-cover" style="left:{rect.x}px;top:{rect.y}px;width:{rect.w}px;height:{rect.h}px"></div>{/if}
      <div class="coach-ring" style="left:{rect.x}px;top:{rect.y}px;width:{rect.w}px;height:{rect.h}px"></div>
    {:else}
      <div class="coach-band" style="inset:0"></div>
    {/if}
    <div bind:this={card} class="coach-card" role="dialog" aria-label={tour.title} aria-live="polite" style="left:{place.left}px;top:{place.top}px">
      <div class="gap" style="align-items:flex-start">
        <div class="grow">
          <div class="dimmer small">{tour.title} · {help.step + 1} of {tour.steps.length}</div>
          <div style="font-weight:600;padding-top:2px">{step.title}</div>
        </div>
        <IconBtn icon="close" label="End the tour" class="tiny" size={16} onclick={() => finish(false)} />
      </div>
      <div class="dim"><Rich text={step.body} /></div>
      {#if step.tryIt}<p class="dimmer small">Go ahead and try it. The tour waits.</p>{/if}
      <div class="gap" style="justify-content:flex-end;padding-top:2px">
        {#if help.step > 0}<Btn label="Back" variant="quiet" onclick={() => go(-1)} />{/if}
        <button bind:this={next} type="button" class="btn primary" onclick={() => go(1)}>{last ? 'Done' : 'Next'}</button>
      </div>
    </div>
  </div>
{/if}
