<script>
  import { PIG } from '../../core/pigments.js';

  export let n = '';
  export let label = '';
  export let active = false;

  $: outerStyle = `display: grid; grid-template-columns: auto 1fr; align-items: center; gap: 12px; padding: 10px 20px; border-left: 2px solid ${active ? PIG.gser : 'transparent'}; background: ${active ? 'var(--accent-soft)' : 'transparent'}; color: ${active ? PIG.gser : 'var(--fg-secondary)'}; font-size: 13px;`;
  $: numStyle = `font-family: var(--font-mono); font-size: 11px; color: ${active ? PIG.gser : 'var(--fg-disabled)'};`;
  $: labelStyle = `font-weight: ${active ? 600 : 400};`;
</script>

<div style={outerStyle}>
  <span style={numStyle}>{n}</span>
  <span style={labelStyle}>{label}</span>
</div>
