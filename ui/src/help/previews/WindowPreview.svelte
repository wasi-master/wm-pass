<script lang="ts">
  import { help } from '../help.svelte';
  import WindowMock from './WindowMock.svelte';

  // The three parts of the window. Each one opens its own page of the guide.
  const topics = { sidebar: 'screen.sidebar', list: 'screen.list', item: 'screen.item' } as const;
</script>

<div class="mock-frame"><WindowMock onpart={(part) => help.open(topics[part])} /></div>
<p class="preview-caption">Click a part to read about it.</p>
