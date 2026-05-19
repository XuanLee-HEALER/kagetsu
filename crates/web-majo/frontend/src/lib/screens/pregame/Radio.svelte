<script>
  import { PIG } from '../../core/pigments.js';

  export let name = '';
  export let label = '';
  export let sub = '';
  export let active = false;

  $: outerStyle = `display: grid; grid-template-columns: auto 1fr; align-items: center; gap: 12px; padding: 10px 14px; background: ${active ? 'var(--accent-soft)' : 'var(--bg-surface0)'}; border: 1px solid ${active ? 'var(--border-focus)' : 'var(--border-default)'}; border-radius: var(--radius-md); cursor: pointer;`;
  $: ringStyle = `width: 16px; height: 16px; border-radius: 50%; border: 1.5px solid ${active ? PIG.gser : 'var(--fg-disabled)'}; display: flex; align-items: center; justify-content: center;`;
  $: dotStyle = `width: 7px; height: 7px; border-radius: 50%; background: ${PIG.gser};`;
  $: labelStyle = `color: var(--fg-primary); font-size: 14px; font-weight: ${active ? 500 : 400};`;
</script>

<label style={outerStyle} data-radio-name={name}>
  <span style={ringStyle}>
    {#if active}
      <span style={dotStyle}></span>
    {/if}
  </span>
  <div>
    <div style={labelStyle}>{label}</div>
    {#if sub}
      <div style="color: var(--fg-tertiary); font-size: 12px; margin-top: 2px;">{sub}</div>
    {/if}
  </div>
</label>
