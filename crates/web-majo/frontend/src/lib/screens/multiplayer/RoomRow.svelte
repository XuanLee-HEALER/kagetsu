<script>
  import { PIG } from '../../core/pigments.js';

  export let active = false;
  export let host = '';
  export let mode = '';
  export let players = 0;
  export let max = 0;
  export let addr = '';
  export let rules = '';
  export let highlight = '';

  $: isZT = highlight === 'zt';
  $: outerStyle = `display: grid; grid-template-columns: auto 1fr auto auto auto; gap: 16px; align-items: center; padding: 14px 18px; background: ${active ? 'var(--accent-soft)' : 'var(--bg-surface0)'}; border: 1px solid ${active ? 'var(--border-focus)' : 'var(--border-default)'}; border-radius: var(--radius-lg);`;
  $: badgeStyle = `width: 36px; height: 36px; border-radius: var(--radius-md); background: ${isZT ? 'rgba(91,138,184,0.18)' : 'var(--bg-surface1)'}; border: 1px solid ${isZT ? PIG.ngonpo : 'var(--border-default)'}; display: flex; align-items: center; justify-content: center; color: ${isZT ? PIG.ngonpo : 'var(--fg-secondary)'}; font-family: var(--font-mono); font-size: 10px; font-weight: 600;`;
  $: hostStyle = `font-family: var(--font-serif); font-size: var(--text-lg); color: ${active ? PIG.gser : 'var(--fg-primary)'}; letter-spacing: var(--tracking-tight); line-height: 1.2;`;
  $: modeColor = isZT ? PIG.ngonpo : mode === 'Standard' ? 'var(--fg-secondary)' : PIG.gser;
  $: modeStyle = `color: ${modeColor}; font-size: 13px; font-weight: 500;`;
  $: playersStyle = `font-family: var(--font-mono); font-size: var(--text-md); color: ${players === max ? 'var(--fg-disabled)' : 'var(--fg-primary)'};`;
</script>

<div style={outerStyle}>
  <div style={badgeStyle}>
    {mode === 'ZeroTrust' ? 'ZT' : 'P2P'}
  </div>
  <div>
    <div style={hostStyle}>{host}</div>
    <div style="color: var(--fg-tertiary); font-size: 12px; font-family: var(--font-mono); margin-top: 2px;">{addr}</div>
  </div>
  <div style="color: var(--fg-secondary); font-size: 13px;">{rules}</div>
  <div style={modeStyle}>{mode}</div>
  <div style={playersStyle}>
    {players}/{max}
  </div>
</div>
