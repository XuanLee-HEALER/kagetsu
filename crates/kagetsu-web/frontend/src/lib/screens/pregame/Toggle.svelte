<script>
  import { PIG } from '../../core/pigments.js';

  export let label = '';
  export let sub = '';
  export let on = false;

  $: labelStyle = `color: var(--fg-primary); font-size: 14px; font-weight: ${on ? 500 : 400};`;
  $: trackStyle = `width: 36px; height: 20px; border-radius: 10px; background: ${on ? PIG.gser : 'var(--bg-surface2)'}; position: relative; transition: background 200ms;`;
  $: knobStyle = `position: absolute; top: 2px; left: ${on ? 18 : 2}px; width: 16px; height: 16px; border-radius: 50%; background: ${on ? 'var(--fg-on-accent)' : 'var(--fg-primary)'}; transition: left 200ms;`;
</script>

<label style="display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 12px; padding: 10px 14px; background: var(--bg-surface0); border: 1px solid var(--border-default); border-radius: var(--radius-md); cursor: pointer;">
  <div>
    <div style={labelStyle}>{label}</div>
    {#if sub}
      <div style="color: var(--fg-tertiary); font-size: 12px; margin-top: 2px;">{sub}</div>
    {/if}
  </div>
  <span style={trackStyle}>
    <span style={knobStyle}></span>
  </span>
</label>
