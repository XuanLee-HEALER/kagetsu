<script>
  export let size = 20;
</script>

<!-- Simplified vajra silhouette: two diamond pommels + center sphere. -->
<svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="var(--accent)" stroke-width="1.2">
  <circle cx="10" cy="10" r="2" fill="var(--accent)" />
  <path d="M10 2 L8 5 L10 8 L12 5 Z" />
  <path d="M10 18 L8 15 L10 12 L12 15 Z" />
  <path d="M2 10 L5 8 L8 10 L5 12 Z" />
  <path d="M18 10 L15 8 L12 10 L15 12 Z" />
</svg>
