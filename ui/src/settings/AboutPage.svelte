<script lang="ts">
  import { host } from '../engine/host';
  import { overlays } from '../parts/floating.svelte';

  let version = $state('');
  $effect(() => {
    void host().desktop?.version().then(
      (v) => (version = v),
      () => undefined,
    );
  });
</script>

<div class="stack" style="max-width:520px">
  <p style="font-size:var(--fs-l);font-weight:600">WM Pass {version}</p>
  <p class="dim">Part of the WM Suite. Free and open source under the MIT license. It sends nothing anywhere unless you turn on a feature that has to.</p>
  <p><button type="button" class="link" onclick={() => void host().openUrl('https://github.com/wasi-master/wm-pass').catch((failure: any) => overlays.toast(failure?.message ?? 'The page could not be opened.'))}>Source code</button></p>
</div>
