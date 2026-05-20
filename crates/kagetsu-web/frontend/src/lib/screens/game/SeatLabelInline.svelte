<script>
  import { PIG } from '../../core/pigments.js';

  export let seat = '';
  export let name = '';
  export let score = 0;
  export let riichi = false;
  export let dealer = false;
  export let you = false;
  /** Extra CSS appended to outer style. */
  export let style = '';

  $: borderColor = you ? 'var(--border-focus)' : 'var(--border-default)';
  $: seatColor = you ? PIG.gser : dealer ? PIG.gser : 'var(--fg-primary)';
  $: scoreColor = you ? PIG.gser : 'var(--fg-primary)';
  $: scoreText = score != null ? score.toLocaleString() : '';
</script>

<!-- Compact seat label — always upright, parked at a table corner. -->
<div style="display: flex; align-items: center; gap: 10px; padding: 5px 12px; background: var(--bg-deep); border: 1px solid {borderColor}; border-radius: var(--radius-md); white-space: nowrap; {style}">
  <span style="font-family: var(--font-serif); font-size: var(--text-lg); font-weight: 500; color: {seatColor}; line-height: 1;">{seat}</span>
  <div style="display: flex; flex-direction: column; gap: 0; line-height: 1.15;">
    <div style="font-size: 11px; color: var(--fg-tertiary); display: flex; gap: 6px; align-items: center;">
      {name}
      {#if dealer}<span style="color: {PIG.gser}; font-weight: 600;">庄</span>{/if}
      {#if riichi}<span style="color: {PIG.mtshal}; font-weight: 600;">立直</span>{/if}
    </div>
    <div style="font-size: var(--text-md); font-weight: 600; font-family: var(--font-mono); color: {scoreColor};">{scoreText}</div>
  </div>
</div>
