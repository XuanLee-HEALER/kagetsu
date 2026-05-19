<script>
  // Protocol-step row in side panel — P{n} index + status glyph + name.
  import { PIG } from '../../core/pigments.js';

  export let n = 0;
  export let name = '';
  /** @type {'done'|'active'|'idle'} */
  export let status = 'idle';

  $: color = status === 'done' ? PIG.ljangkhu : status === 'active' ? PIG.gser : 'var(--fg-disabled)';
  $: glyph = status === 'done' ? '●' : status === 'active' ? '◆' : '○';
  $: rowStyle = `display: grid; grid-template-columns: auto auto 1fr; align-items: center; gap: 8px; padding: 4px 6px; background: ${status === 'active' ? 'var(--accent-soft)' : 'transparent'}; border-radius: var(--radius-sm);`;
  $: nameStyle = `font-size: 12px; color: ${status === 'idle' ? 'var(--fg-disabled)' : status === 'active' ? PIG.gser : 'var(--fg-secondary)'}; font-weight: ${status === 'active' ? 500 : 400};`;
</script>

<div style={rowStyle}>
  <span style="font-family: var(--font-mono); font-size: 10px; color: var(--fg-disabled);">P{n}</span>
  <span style="color: {color}; font-size: 10px;">{glyph}</span>
  <span style={nameStyle}>{name}</span>
</div>
