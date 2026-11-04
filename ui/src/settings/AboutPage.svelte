<script lang="ts">
  import { host } from '../engine/host';

  let version = $state('');
  $effect(() => {
    void host().desktop?.version().then((v) => (version = v));
  });
</script>

<div class="stack" style="max-width:520px">
  <p style="font-size:var(--fs-l);font-weight:600">WM Pass {version}</p>
  <p class="dim">Part of the WM Suite. Free and open source under the MIT license. It sends nothing anywhere unless you turn on a feature that has to.</p>
  <p><button type="button" class="link" onclick={() => host().openUrl('https://github.com/wasi-master/wm-pass')}>Source code</button></p>
</div>
