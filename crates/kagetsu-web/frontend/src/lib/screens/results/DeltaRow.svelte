<script>
  import { PIG } from '../../core/pigments.js';

  export let seat = '';
  export let delta = '';
  export let before = 0;
  export let after = 0;
  export let you = false;

  $: positive = delta.startsWith('+');
  $: seatColor = you ? PIG.gser : 'var(--fg-secondary)';
  $: deltaColor = positive ? PIG.gser : PIG.mtshal;
</script>

<div style="display: grid; grid-template-columns: 1fr auto auto; align-items: center; gap: 14px; padding: 4px 0;">
  <span style="color: {seatColor}; font-size: 13px;">{seat}</span>
  <span style="font-family: var(--font-mono); font-size: 14px; color: {deltaColor}; font-weight: 600;">{delta}</span>
  <span style="font-family: var(--font-mono); font-size: 12px; color: var(--fg-tertiary); min-width: 90px; text-align: right;">
    {before.toLocaleString()} → {after.toLocaleString()}
  </span>
</div>
