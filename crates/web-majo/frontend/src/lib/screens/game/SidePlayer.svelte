<script>
  import { PIG } from '../../core/pigments.js';
  import Mini from './Mini.svelte';

  export let p = {};
  export let you = false;

  $: bg = you ? 'var(--accent-soft)' : 'transparent';
  $: borderColor = you ? 'var(--border-focus)' : 'var(--border-subtle)';
  $: seatColor = you ? PIG.gser : p.riichi ? PIG.mtshal : 'var(--fg-primary)';
  $: scoreColor = you ? PIG.gser : 'var(--fg-primary)';
  $: scoreText = p.score != null ? p.score.toLocaleString() : '';
</script>

<!-- One score row inside the side panel — seat / name / score / status badges. -->
<div style="display: grid; grid-template-columns: auto auto 1fr auto; align-items: center; gap: 10px; padding: 6px 10px; background: {bg}; border: 1px solid {borderColor}; border-radius: var(--radius-md);">
  <span style="font-family: var(--font-serif); font-size: var(--text-md); color: {seatColor}; line-height: 1;">{p.seat}</span>
  <span style="color: var(--fg-tertiary); font-size: 12px;">{p.name}</span>
  <span style="font-family: var(--font-mono); font-size: var(--text-base); text-align: right; color: {scoreColor};">{scoreText}</span>
  <div style="display: flex; gap: 4px; min-width: 36px; justify-content: flex-end;">
    {#if p.dealer}<Mini t="庄" tone="accent" />{/if}
    {#if p.riichi}<Mini t="立" tone="danger" />{/if}
  </div>
</div>
