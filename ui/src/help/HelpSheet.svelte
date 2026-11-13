<script lang="ts">
  import type { Component } from 'svelte';
  import type { Effect, Topic } from '$help';
  import Btn from '../parts/Btn.svelte';
  import Dialog from '../parts/Dialog.svelte';
  import Icon from '../parts/Icon.svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import { host } from '../engine/host';
  import { pass } from '../engine/pass.svelte';
  import { prefs, type PrefKey } from '../engine/prefs.svelte';
  import { ui } from '../vault/ui.svelte';
  import Figure from './Figure.svelte';
  import Rich from './Rich.svelte';
  import { help } from './help.svelte';

  // The guide's page about one thing: what it is, why, how, when, what each choice does, a picture
  // or a live preview, and what it does not do. Links open the next topic here, with a way back.
  let { simple = false }: { simple?: boolean } = $props();

  const topic = $derived(help.current);
  const pref = $derived(topic?.pref && prefs.has(topic.pref) ? (topic.pref as PrefKey) : null);
  const now = $derived(pref ? prefs.v[pref] : undefined);
  const fallback = $derived(pref ? prefs.defaultOf(pref) : undefined);
  /** The choice the preview shows: the one picked here, or the setting as it is. */
  let picked = $state<Effect['value'] | undefined>(undefined);
  $effect(() => {
    void topic?.id;
    picked = undefined;
  });
  const showing = $derived(picked ?? now);

  type PreviewTable = Record<string, Component<{ value?: unknown; pref?: string }>>;
  let previews = $state.raw<PreviewTable | null>(null);
  $effect(() => {
    if (topic?.preview && !previews && !simple) void import('./previews/index').then(
        (m) => (previews = m.PREVIEWS),
        () => undefined,
      );
  });
  const Preview = $derived(topic?.preview && previews ? previews[topic.preview] : null);

  const unlocked = $derived(pass.state.kind === 'unlocked');
  const canOpen = $derived(!simple && unlocked && !!topic?.page && topic.kind !== 'page' && ui.section !== `settings/${topic.page}`);
  const canGoToPage = $derived(!simple && unlocked && topic?.kind === 'page' && ui.section !== `settings/${topic.page}`);
  const canTour = $derived(!simple && unlocked && !!topic?.tour && help.tours().some((t) => t.id === topic!.tour));

  function same(a: unknown, b: unknown) {
    return a === b;
  }

  function apply(value: Effect['value']) {
    if (!pref) return;
    if (typeof value !== typeof prefs.defaultOf(pref)) return;
    prefs.set(pref, value as never);
    picked = undefined;
  }

  function keyed(event: KeyboardEvent) {
    // Escape closes the sheet, and must not also clear the vault's search behind it.
    if (event.key === 'Escape') event.stopPropagation();
    if (event.altKey && event.key === 'ArrowLeft' && help.stack.length > 1) {
      event.preventDefault();
      help.back();
    }
  }

  /** The arrow keys move through the choices as through any group of radio buttons, and choose. */
  function radioKey(event: KeyboardEvent) {
    const step = event.key === 'ArrowDown' || event.key === 'ArrowRight' ? 1 : event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 0;
    if (!step || event.altKey) return;
    const radios = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    const at = radios.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0) return;
    event.preventDefault();
    const next = radios[(at + step + radios.length) % radios.length];
    next.focus();
    next.click();
  }

  /** Related topics here, one per title: a page and its row can share a name. */
  function related(t: Topic) {
    const seen = new Set<string>([t.title]);
    return (t.related ?? [])
      .map((id) => help.topic(id))
      .filter((r): r is Topic => !!r && !seen.has(r.title) && (seen.add(r.title), true));
  }
</script>

{#if topic}
  <Dialog wide class="help-sheet" onclose={() => help.close()} onkeydown={keyed}>
    {#snippet head()}
      <div class="gap" style="align-items:flex-start">
        {#if help.stack.length > 1}<IconBtn icon="arrow_back" label="Back" onclick={() => help.back()} />{/if}
        <div class="grow">
          <div class="dim small help-where">{help.where(topic)}</div>
          <div>{topic.title}</div>
        </div>
        <IconBtn icon="close" label="Close" onclick={() => help.close()} />
      </div>
    {/snippet}

    {#key topic.id}
      <div class="help-body">
        <Rich text={topic.what} lead />

        {#if topic.figure && !Preview}
          <Figure id={topic.figure} />
        {/if}

        {#if topic.why}
          <section><h3>Why it is there</h3><Rich text={topic.why} /></section>
        {/if}
        {#if topic.how}
          <section><h3>How it works</h3><Rich text={topic.how} /></section>
        {/if}
        {#if topic.when}
          <section><h3>When to use it</h3><Rich text={topic.when} /></section>
        {/if}

        {#if topic.effects?.length}
          <section>
            <h3>What each choice does</h3>
            {#if Preview}
              <div class="preview"><Preview value={showing} pref={topic.pref} /></div>
            {/if}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="help-choices" role={pref ? 'radiogroup' : undefined} aria-label={pref ? topic.title : undefined} onkeydown={pref ? radioKey : undefined}>
              {#each topic.effects as effect, index (String(effect.value))}
                {@const current = pref !== null && same(effect.value, now)}
                {@const usual = pref !== null && same(effect.value, fallback)}
                {#if pref}
                  <button
                    type="button"
                    class="help-choice"
                    role="radio"
                    aria-checked={same(effect.value, showing)}
                    tabindex={(topic.effects.some((e) => same(e.value, showing)) ? same(effect.value, showing) : index === 0) ? 0 : -1}
                    onclick={() => (picked = effect.value)}
                  >
                    <span class="mark">{#if current}<Icon name="check" size={16} /><span class="sr-only">In use. </span>{/if}</span>
                    <span class="grow">
                      <span class="name">{effect.label}{#if usual}<span class="dimmer small"> · usual</span>{/if}</span>
                      <Rich text={effect.text} />
                    </span>
                  </button>
                {:else}
                  <div class="help-choice">
                    <span class="mark"></span>
                    <span class="grow"><span class="name">{effect.label}</span><Rich text={effect.text} /></span>
                  </div>
                {/if}
              {/each}
            </div>
            {#if pref && picked !== undefined && !same(picked, now)}
              <div class="gap" style="padding-top:8px">
                <Btn label="Use this" variant="primary" onclick={() => apply(picked!)} />
                <Btn label="Keep it as it is" variant="quiet" onclick={() => (picked = undefined)} />
              </div>
            {/if}
          </section>
        {:else if Preview}
          <div class="preview"><Preview value={showing} pref={topic.pref} /></div>
        {/if}

        {#if topic.figure && Preview}
          <Figure id={topic.figure} />
        {/if}

        {#if topic.caveats}
          <section class="help-caveats"><h3>Limits</h3><Rich text={topic.caveats} /></section>
        {/if}

        {#if related(topic).length}
          <section>
            <h3>Related</h3>
            <div class="gap wrap">
              {#each related(topic) as other (other.id)}
                <button type="button" class="chip" onclick={() => help.open(other.id)}>{other.title}</button>
              {/each}
            </div>
          </section>
        {/if}

        {#if topic.docs && !simple}
          <p class="small"><button type="button" class="link" onclick={() => void host().openUrl(`https://wmpass.pages.dev${topic.docs}`).catch(() => undefined)}>More on the WM Pass website</button></p>
        {/if}
      </div>
    {/key}

    {#snippet foot()}
      {#if canTour}<Btn label="Show me" icon="touch_app" onclick={() => help.startTour(topic.tour!)} />{/if}
      {#if canOpen}<Btn label="Open the setting" onclick={() => help.show(topic)} />{/if}
      {#if canGoToPage}<Btn label="Open the page" onclick={() => help.show(topic)} />{/if}
      <span class="grow"></span>
      <Btn label="Close" variant="primary" onclick={() => help.close()} />
    {/snippet}
  </Dialog>
{/if}
