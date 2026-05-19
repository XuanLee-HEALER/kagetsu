<script>
  import Eyebrow from '../../core/Eyebrow.svelte';

  export let label = '';
  export let value = '';
  export let sub = '';
  export let mono = false;
  export let muted = false;

  $: opacity = muted ? 0.5 : 1;
  $: valueFont = mono ? 'var(--font-mono)' : 'var(--font-serif)';
  $: valueWeight = mono ? 600 : 400;
</script>

<div style="padding: 18px; background: var(--bg-surface0); border: 1px solid var(--border-default); border-radius: var(--radius-lg); opacity: {opacity};">
  <Eyebrow style="font-size: 10px;">{label}</Eyebrow>
  <div style="margin-top: 8px; font-family: {valueFont}; font-size: var(--text-3xl); font-weight: {valueWeight}; letter-spacing: var(--tracking-tight); color: var(--fg-primary); line-height: 1;">{value}</div>
  <div style="font-size: 12px; color: var(--fg-tertiary); margin-top: 6px;">{sub}</div>
</div>
