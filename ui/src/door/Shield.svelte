<script lang="ts">
  // The logo, on this screen and nowhere else. Same geometry as tools/icon/gen_icon.py.
  let { size = 52, refused = false }: { size?: number; refused?: boolean } = $props();
  const id = `m${Math.random().toString(36).slice(2, 8)}`;
</script>

<svg width={size} height={size} viewBox="20 12 68 90" role="img" aria-label="WM Pass" style="display:block">
  <defs>
    <mask id="{id}body">
      <rect x="0" y="0" width="108" height="108" fill="black" />
      <path d="M31.4,45 H76.6 A6.4,6.4 0 0 1 83,51.4 V73 C83,85 67,93 54,99 C41,93 25,85 25,73 V51.4 A6.4,6.4 0 0 1 31.4,45 Z" fill="white" />
      <path d="M52.7,80.36 A3.6,3.6 0 1 1 55.3,80.36 L56,86.6 H52 Z" fill="black" />
      <path d="M43,15.9 H65 A11.1,11.1 0 0 1 76.1,27 V41 A11.1,11.1 0 0 1 65,52.1 H60.58 L54,63.2 L47.42,52.1 H43 A11.1,11.1 0 0 1 31.9,41 V27 A11.1,11.1 0 0 1 43,15.9 Z" fill="black" />
    </mask>
    <linearGradient id="{id}g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#10B981" />
      <stop offset="1" stop-color="#0D9488" />
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="108" height="108" fill={refused ? 'var(--danger)' : `url(#${id}g)`} mask="url(#{id}body)" style="transition:fill var(--t-med)" />
  <path d="M43,17.5 H65 A9.5,9.5 0 0 1 74.5,27 V41 A9.5,9.5 0 0 1 65,50.5 H58.9 L54,60 L49.1,50.5 H43 A9.5,9.5 0 0 1 33.5,41 V27 A9.5,9.5 0 0 1 43,17.5 Z" fill={refused ? 'var(--danger)' : '#10B981'} />
  <path d="M45.63,39.30 L40.06,35.51V26.13L42.81 28.36V34.05L45.51 35.86V28.50L48.33 26.51V35.89L51.13 34.02V28.28L53.95 26.09V35.45L48.26 39.30Z M54.79,27.09H66.77Q67.94 27.38 67.94 28.59V36.68L64.55 38.76V29.58H63.13V36.55L59.60 38.90V29.58H58.18V38.49L54.79 40.71Z" fill="#fff" />
</svg>
