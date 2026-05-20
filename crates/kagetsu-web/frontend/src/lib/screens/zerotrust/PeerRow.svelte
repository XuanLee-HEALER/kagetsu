<script>
  // Peer node row — colored status dot, seat glyph, name + pk, rtt.
  import { PIG } from '../../core/pigments.js';

  export let seat = '';
  export let name = '';
  export let status = 'ok';
  export let pk = '';
  export let rtt = '';
  export let you = false;

  $: ok = status === 'ok';
  $: rowStyle = `display: grid; grid-template-columns: auto auto 1fr auto; align-items: center; gap: 10px; padding: 6px 10px; background: ${you ? 'var(--accent-soft)' : 'transparent'}; border: 1px solid ${you ? 'var(--border-focus)' : 'var(--border-subtle)'}; border-radius: var(--radius-md);`;
  $: dotStyle = `width: 7px; height: 7px; border-radius: 50%; background: ${ok ? PIG.ljangkhu : PIG.mtshal};${ok ? ` box-shadow: 0 0 6px ${PIG.ljangkhu};` : ''}`;
  $: seatStyle = `font-family: var(--font-serif); font-size: var(--text-md); color: ${you ? PIG.gser : 'var(--fg-primary)'};`;
</script>

<div style={rowStyle}>
  <span style={dotStyle}></span>
  <span style={seatStyle}>{seat}</span>
  <div>
    <div style="font-size: 12px; color: var(--fg-primary);">{name}</div>
    <div style="font-family: var(--font-mono); font-size: 10px; color: var(--fg-tertiary);">{pk}</div>
  </div>
  <span style="font-family: var(--font-mono); font-size: 11px; color: var(--fg-tertiary);">{rtt}</span>
</div>
