<script>
  import { PIG } from '../../core/pigments.js';

  export let label = '';
  export let value = '';
  /** @type {'warning'|null|undefined} */
  export let tone = null;
  export let mono = false;

  $: valueColor = tone === 'warning' ? PIG.likhri : 'var(--fg-primary)';
  $: valueFont = mono ? 'var(--font-mono)' : 'var(--font-sans)';
  $: valueTracking = mono ? '0' : 'var(--tracking-tight)';
</script>

<!-- Compact label + value pair used in the top status bar. -->
<div style="display: flex; flex-direction: column; gap: 2px; line-height: 1;">
  {#if label}
    <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-wide); text-transform: uppercase; color: var(--fg-tertiary); font-size: 10px;">{label}</span>
  {/if}
  <span style="font-size: var(--text-md); font-weight: 500; color: {valueColor}; font-family: {valueFont}; letter-spacing: {valueTracking};">{value}</span>
</div>
