<script>
  import { PIG } from './pigments.js';

  export let label = '';
  export let primary = false;
  export let kb = '';

  $: bg = primary ? PIG.gser : 'var(--bg-surface0)';
  $: fg = primary ? 'var(--fg-on-accent)' : 'var(--fg-primary)';
  $: border = primary ? PIG.gser : 'var(--border-default)';
  $: kbBg = primary ? 'rgba(12,12,20,0.18)' : 'var(--bg-surface1)';

  $: btnStyle = `height: 40px; padding: 0 20px; background: ${bg}; color: ${fg}; border: 1px solid ${border}; border-radius: var(--radius-md); font-size: 14px; font-weight: 500; font-family: var(--font-sans); display: flex; align-items: center; gap: 10px; cursor: pointer;`;
  $: kbStyle = `font-family: var(--font-mono); font-size: 11px; opacity: 0.7; padding: 1px 6px; background: ${kbBg}; border-radius: var(--radius-xs);`;
</script>

<button style={btnStyle}>
  {label}
  {#if kb}
    <span style={kbStyle}>{kb}</span>
  {/if}
</button>
