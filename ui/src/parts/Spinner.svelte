<script lang="ts">
  // Something is on its way. A quiet ring, for a row or a heading that is waiting for an answer.
  let { size = 16, label = 'Working' }: { size?: number; label?: string } = $props();
</script>

<span class="spinner" role="status" aria-label={label} style="width:{size}px;height:{size}px"></span>

<style>
  .spinner {
    display: inline-block;
    flex: none;
    box-sizing: border-box;
    border-radius: 50%;
    border: 2px solid var(--s3);
    border-top-color: var(--primary);
    animation: turn 800ms linear infinite;
  }
  :global(:root[data-motion='off']) .spinner {
    animation-duration: 2400ms;
  }
  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
</style>
