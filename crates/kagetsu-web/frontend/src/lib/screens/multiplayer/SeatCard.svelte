<script>
  import { PIG } from '../../core/pigments.js';

  export let seat = '';
  export let name = '';
  export let host = false;
  export let you = false;
  export let empty = false;

  $: outerStyle = `padding: 24px; background: ${you ? 'var(--accent-soft)' : empty ? 'transparent' : 'var(--bg-surface0)'}; border: 1px ${empty ? 'dashed' : 'solid'} ${you ? 'var(--border-focus)' : 'var(--border-default)'}; border-radius: var(--radius-lg); display: flex; align-items: center; gap: 18px; min-height: 100px;`;
  $: seatBoxStyle = `width: 56px; height: 56px; border-radius: var(--radius-md); background: ${empty ? 'transparent' : 'var(--bg-base)'}; border: 1px ${empty ? 'dashed' : 'solid'} ${you ? PIG.gser : 'var(--border-default)'}; display: flex; align-items: center; justify-content: center; font-family: var(--font-serif); font-size: var(--text-2xl); color: ${you ? PIG.gser : empty ? 'var(--fg-disabled)' : 'var(--fg-primary)'};`;
  $: nameStyle = `font-size: var(--text-md); color: ${empty ? 'var(--fg-disabled)' : 'var(--fg-primary)'}; font-style: ${empty ? 'italic' : 'normal'};`;
  $: hostStyle = `color: ${PIG.gser}; font-weight: 600;`;
  $: readyStyle = `color: ${PIG.ljangkhu};`;
</script>

<div style={outerStyle}>
  <div style={seatBoxStyle}>{seat}</div>
  <div style="flex: 1;">
    <div style={nameStyle}>{empty ? 'Empty · 等待加入' : name}</div>
    <div style="color: var(--fg-tertiary); font-size: 12px; margin-top: 4px; display: flex; gap: 8px;">
      {#if host}
        <span style={hostStyle}>HOST</span>
      {/if}
      {#if you}
        <span>YOU</span>
      {/if}
      {#if empty}
        <span>or fill AI</span>
      {:else}
        <span style={readyStyle}>● ready</span>
      {/if}
    </div>
  </div>
</div>
