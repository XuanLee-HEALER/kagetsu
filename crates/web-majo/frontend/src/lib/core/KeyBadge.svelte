<script>
  import { PIG } from './pigments.js';

  export let k = '';
  export let label = '';
  /** @type {'default'|'primary'|'danger'|'ok'|'info'} */
  export let tone = 'default';
  export let disabled = false;
  /** @type {'sm'|'md'} */
  export let size = 'md';

  const TONES = {
    default: { color: 'var(--fg-primary)', border: 'var(--border-default)', bg: 'var(--bg-surface0)' },
    primary: { color: PIG.gser, border: 'rgba(210,180,80,0.55)', bg: 'rgba(210,180,80,0.10)' },
    danger: { color: PIG.mtshal, border: PIG.mtshal, bg: 'rgba(187,68,65,0.10)' },
    ok: { color: PIG.ljangkhu, border: PIG.ljangkhu, bg: 'rgba(82,117,89,0.12)' },
    info: { color: PIG.ngonpo, border: PIG.ngonpo, bg: 'rgba(91,138,184,0.12)' },
  };

  $: ts = TONES[tone];
  $: sizing = size === 'sm'
    ? { kp: '1px 6px', kf: 11, lf: 11 }
    : { kp: '2px 8px', kf: 12, lf: 13 };

  $: wrapStyle = `display: inline-flex; align-items: center; gap: 8px; opacity: ${disabled ? 0.4 : 1}; font-family: var(--font-sans);`;
  $: keyStyle = `border: 1px solid ${ts.border}; background: ${ts.bg}; color: ${ts.color}; padding: ${sizing.kp}; border-radius: var(--radius-sm); font-weight: 600; font-size: ${sizing.kf}px; font-family: var(--font-mono); min-width: 18px; text-align: center; display: inline-block;`;
  $: labelStyle = `color: ${disabled ? 'var(--fg-disabled)' : 'var(--fg-secondary)'}; font-size: ${sizing.lf}px;`;
</script>

<span style={wrapStyle}>
  <span style={keyStyle}>{k}</span>
  {#if label}
    <span style={labelStyle}>{label}</span>
  {/if}
</span>
