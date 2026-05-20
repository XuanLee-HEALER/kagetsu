<script>
  import { PIG } from '../../core/pigments.js';

  /** @type {{ rank: number, seat: string, name: string, base: number, uma: string, final: number, you?: boolean }} */
  export let r;

  const colors = ['', PIG.gser, PIG.dri_med, PIG.dul_ba, 'var(--fg-disabled)'];

  $: bg = r.you ? 'var(--accent-soft)' : 'var(--bg-surface0)';
  $: borderColor = r.you ? 'var(--border-focus)' : 'var(--border-default)';
  $: rankColor = colors[r.rank];
  $: seatColor = r.you ? PIG.gser : 'var(--fg-primary)';
  $: finalColor = r.you ? PIG.gser : 'var(--fg-primary)';
  $: umaColor = r.rank <= 2 ? PIG.ljangkhu : PIG.mtshal;
</script>

<div style="display: grid; grid-template-columns: auto auto 1fr auto auto; align-items: center; gap: 18px; padding: 16px 22px; background: {bg}; border: 1px solid {borderColor}; border-radius: var(--radius-lg);">
  <span style="font-family: var(--font-serif); font-size: var(--text-4xl); font-weight: 300; color: {rankColor}; line-height: 1; min-width: 40px;">{r.rank}</span>
  <span style="font-family: var(--font-serif); font-size: var(--text-xl); color: {seatColor};">{r.seat}</span>
  <div>
    <div style="font-size: var(--text-md); color: var(--fg-primary);">{r.name}</div>
    <div style="font-size: 11px; color: var(--fg-tertiary); margin-top: 2px;">
      base {r.base.toLocaleString()} · uma {r.uma}
    </div>
  </div>
  <span style="font-family: var(--font-mono); font-size: var(--text-xl); color: {finalColor}; font-weight: 600;">{r.final.toLocaleString()}</span>
  <span style="font-family: var(--font-mono); font-size: 12px; color: {umaColor}; font-weight: 600;">{r.uma}</span>
</div>
