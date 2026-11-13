<script lang="ts">
  import type { Topic } from '$help';
  import Field from '../parts/Field.svelte';
  import Group from '../parts/Group.svelte';
  import Icon from '../parts/Icon.svelte';
  import Row from '../parts/Row.svelte';
  import { GROUPS, PAGES, type PageInfo } from '../settings/pages';
  import { available } from '../settings/shown';
  import Choice from '../settings/Choice.svelte';
  import Toggle from '../settings/Toggle.svelte';
  import { help } from './help.svelte';
  import { plural } from '../engine/text';

  // Settings, Guide: every topic in one place. Tours first, then the screens, then how things
  // work, then each settings page and what is on it.
  let query = $state('');
  let open = $state<string[]>([]);

  $effect(() => {
    void help.load().catch(() => undefined);
  });

  // The same pages the settings list has here: a Mac's widgets are not on Windows, for one.
  function shown(page: PageInfo) {
    return available(page) && page.id !== 'guide';
  }

  const all = $derived.by<Topic[]>(() => {
    const content = help.content;
    if (!content) return [];
    return (content.TOPICS as readonly Topic[]).filter((t) => content.availableOn(t, help.shell)).map((t) => content.resolve(t, help.shell));
  });
  const screens = $derived(all.filter((t) => t.kind === 'screen'));
  const ideas = $derived(all.filter((t) => (t.kind === 'feature' || t.kind === 'concept') && !t.id.startsWith('group.')));
  const found = $derived(query.trim() ? help.search(query) : []);

  function onPage(id: string) {
    return all.filter((t) => t.page === id && t.kind !== 'page' && t.kind !== 'feature' && t.kind !== 'screen');
  }

  function toggle(id: string) {
    open = open.includes(id) ? open.filter((o) => o !== id) : [...open, id];
  }
</script>

<div class="stack guide-list" style="gap:0">
  <p class="dim" style="padding-bottom:14px">What every part of WM Pass does, and why. The same pages open from the <span class="kbd">?</span> beside a setting and from {help.keyLabel('help') || 'the Help menu'} anywhere.</p>

  <div style="padding-bottom:16px"><Field bind:value={query} placeholder="Search the guide" /></div>

  {#if query.trim()}
    <Group title={found.length ? `${found.length} found` : 'Nothing found'}>
      {#each found as topic (topic.id)}
        <Row title={topic.title} sub={help.where(topic)} chevron onclick={() => help.open(topic.id)} />
      {/each}
    </Group>
  {:else if help.content}
    <Group title="Start here">
      <Row title="Show the welcome again" sub="Where the everyday things are." icon="sentiment_satisfied" help="action.show_welcome" chevron onclick={() => (help.welcome = true)} />
      <Row title="Take the tour" sub="The parts of this window, one at a time." icon="touch_app" help="action.guide_take_tour" chevron onclick={() => help.startWindowTour()} />
    </Group>

    <Group title="Tours">
      {#each help.tours().filter((t) => t.id !== 'tour.window' && t.id !== 'tour.window_narrow') as tour (tour.id)}
        <Row title={tour.title} sub={help.done(tour.id) ? 'Done' : plural(tour.steps.length, 'step')} icon={help.done(tour.id) ? 'check_circle' : 'touch_app'} chevron onclick={() => help.startTour(tour.id)} />
      {/each}
    </Group>

    <Group title="Screens">
      {#each screens as topic (topic.id)}
        <Row title={topic.title} chevron onclick={() => help.open(topic.id)} />
      {/each}
    </Group>

    <Group title="How WM Pass works">
      {#each ideas as topic (topic.id)}
        <Row title={topic.title} chevron onclick={() => help.open(topic.id)} />
      {/each}
    </Group>

    {#each GROUPS as group}
      {@const pages = PAGES.filter((p) => p.group === group && shown(p))}
      {#if pages.length > 0}
        <Group title="Settings, {group}">
          {#each pages as page (page.id)}
            {@const topics = onPage(page.id)}
            {@const expanded = open.includes(page.id)}
            <div class="set-row act" role="presentation">
              <button type="button" class="hit" aria-expanded={expanded} aria-label="{page.title}, {topics.length} topics" onclick={() => toggle(page.id)}></button>
              <span class="text">
                <span style="display:block">{page.title}</span>
                <span class="sub" style="display:block">{topics.length === 1 ? '1 topic' : `${topics.length} topics`}</span>
              </span>
              {#if help.topic(`page.${page.id}`)}
                <button type="button" class="btn quiet" onclick={() => help.open(`page.${page.id}`)}>About the page</button>
              {/if}
              <span class="dimmer" style="display:flex;transform:rotate({expanded ? 180 : 0}deg);transition:transform var(--t-med)"><Icon name="expand_more" size={18} /></span>
            </div>
            {#if expanded}
              {#each topics as topic (topic.id)}
                <Row title={topic.title} sub={topic.kind === 'setting' ? null : topic.kind === 'action' ? 'Button' : null} chevron onclick={() => help.open(topic.id)} />
              {/each}
            {/if}
          {/each}
        </Group>
      {/if}
    {/each}

    <Group title="The guide itself">
      <Choice title="Help buttons" icon="help" pref="help_buttons" options={[['always', 'Always'], ['hover', 'On hover'], ['off', 'Off']]} />
      <Toggle title="Tips on new pages" sub="A line about a page the first time you open it." icon="info" pref="help_hints" />
    </Group>
  {/if}
</div>
