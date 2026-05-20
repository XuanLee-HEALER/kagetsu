<script>
  import { PIG } from '../../core/pigments.js';

  export let seat = '';
  export let name = '';
  export let host = false;
  export let empty = false;

  $: outerStyle = `display: grid; grid-template-columns: auto 1fr auto; align-items: center; gap: 12px; padding: 8px 12px; background: ${empty ? 'transparent' : 'var(--bg-surface0)'}; border: 1px solid ${empty ? 'var(--border-subtle)' : 'var(--border-default)'}; border-radius: var(--radius-md); border-style: ${empty ? 'dashed' : 'solid'};`;
  $: seatStyle = `font-family: var(--font-serif); font-size: var(--text-md); color: ${empty ? 'var(--fg-disabled)' : 'var(--fg-primary)'};`;
  $: nameStyle = `color: ${empty ? 'var(--fg-disabled)' : 'var(--fg-primary)'}; font-size: 13px; font-style: ${empty ? 'italic' : 'normal'};`;
  $: hostStyle = `font-size: 10px; color: ${PIG.gser}; font-weight: 600; padding: 1px 6px; border: 1px solid ${PIG.gser}; border-radius: 2px;`;
</script>

<div style={outerStyle}>
  <span style={seatStyle}>{seat}</span>
  <span style={nameStyle}>{empty ? 'Empty · 等待' : name}</span>
  {#if host}
    <span style={hostStyle}>HOST</span>
  {/if}
</div>
