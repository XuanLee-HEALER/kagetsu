<script>
  // Protocol step pill — P{step}/{total-1} label + 8 progress dots.
  import { PIG } from '../../core/pigments.js';

  export let step = 0;
  export let label = '';
  const total = 8;
</script>

<div style="display: flex; align-items: center; gap: 10px; padding: 5px 12px; background: var(--bg-surface0); border: 1px solid var(--border-default); border-radius: var(--radius-pill);">
  <span style="font-family: var(--font-mono); font-size: 11px; color: {PIG.gser}; font-weight: 600;">P{step}/{total - 1}</span>
  <span style="color: var(--fg-secondary); font-size: 12px;">{label}</span>
  <div style="display: flex; gap: 2px;">
    {#each Array(8) as _, i}
      <span style="width: 4px; height: 4px; border-radius: 50%; background: {i <= step ? PIG.gser : 'var(--bg-surface2)'};"></span>
    {/each}
  </div>
</div>
