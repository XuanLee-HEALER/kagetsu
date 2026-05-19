<script>
  import { PIG } from '../../core/pigments.js';
  import Tile from '../../core/Tile.svelte';
  import WallSlot from './WallSlot.svelte';

  export let remaining = 47;
  export let total = 70;
  export let dora = undefined;
  export let junme = 0;
  export let round = { wind: '東', num: 1, honba: 0 };
  export let size = 168;

  const SLOTS_PER_SIDE = 11;
  const TOTAL_SLOTS = SLOTS_PER_SIDE * 4;
  const wallThickness = 14;
  const margin = wallThickness + 2;

  $: filled = Math.max(0, Math.round((remaining / total) * TOTAL_SLOTS));

  // Drawing order goes CCW from a starting corner. The "filled" slots are
  // 0..filled-1; remaining slots empty out at the draw cursor.
  $: isFilled = (i) => i < filled;
  $: cursorSide = filled < SLOTS_PER_SIDE ? 'top'
    : filled < SLOTS_PER_SIDE * 2 ? 'right'
    : filled < SLOTS_PER_SIDE * 3 ? 'bottom'
    : 'left';

  $: tileCountColor = remaining < 16 ? PIG.likhri : 'var(--fg-primary)';

  const sides = Array.from({ length: SLOTS_PER_SIDE });
</script>

<!-- 4 sides of stacked tile-back slots + center plate. -->
<div style="position: relative; width: {size}px; height: {size}px;">
  <!-- top wall -->
  <div style="position: absolute; top: 0; left: {wallThickness}px; right: {wallThickness}px; height: {wallThickness}px; display: flex; gap: 1px; align-items: center; justify-content: center;">
    {#each sides as _, i}
      <WallSlot filled={isFilled(i)} variant="horizontal" cursor={i === filled && cursorSide === 'top'} />
    {/each}
  </div>

  <!-- right wall -->
  <div style="position: absolute; right: 0; top: {wallThickness}px; bottom: {wallThickness}px; width: {wallThickness}px; display: flex; flex-direction: column; gap: 1px; align-items: center; justify-content: center;">
    {#each sides as _, i}
      {@const idx = SLOTS_PER_SIDE + i}
      <WallSlot filled={isFilled(idx)} variant="vertical" cursor={idx === filled && cursorSide === 'right'} />
    {/each}
  </div>

  <!-- bottom wall (reversed direction) -->
  <div style="position: absolute; bottom: 0; left: {wallThickness}px; right: {wallThickness}px; height: {wallThickness}px; display: flex; gap: 1px; align-items: center; justify-content: center;">
    {#each sides as _, i}
      {@const idx = SLOTS_PER_SIDE * 2 + (SLOTS_PER_SIDE - 1 - i)}
      <WallSlot filled={isFilled(idx)} variant="horizontal" cursor={idx === filled && cursorSide === 'bottom'} />
    {/each}
  </div>

  <!-- left wall (reversed) -->
  <div style="position: absolute; left: 0; top: {wallThickness}px; bottom: {wallThickness}px; width: {wallThickness}px; display: flex; flex-direction: column; gap: 1px; align-items: center; justify-content: center;">
    {#each sides as _, i}
      {@const idx = SLOTS_PER_SIDE * 3 + (SLOTS_PER_SIDE - 1 - i)}
      <WallSlot filled={isFilled(idx)} variant="vertical" cursor={idx === filled && cursorSide === 'left'} />
    {/each}
  </div>

  <!-- center plate inside the wall -->
  <div style="position: absolute; inset: {margin}px; background: var(--bg-deepest); border: 1px solid var(--border-default); border-radius: var(--radius-md); padding: 10px 12px; box-sizing: border-box; display: flex; flex-direction: column; justify-content: space-between; align-items: center; gap: 4px; box-shadow: var(--shadow-2);">
    <div style="display: flex; align-items: baseline; gap: 6px;">
      <span style="font-family: var(--font-serif); font-size: 18px; color: {PIG.gser}; letter-spacing: var(--tracking-tight); line-height: 1;">{round.wind} {round.num}</span>
      <span style="color: var(--fg-tertiary); font-size: 11px;">·</span>
      <span style="color: var(--fg-secondary); font-size: 11px;">{round.honba} 本</span>
    </div>

    <div style="text-align: center;">
      <div style="font-family: var(--font-mono); font-size: 28px; font-weight: 600; line-height: 1; color: {tileCountColor};">{remaining}</div>
      <div style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: var(--fg-tertiary); font-size: 9px; margin-top: 2px;">tiles · 山</div>
    </div>

    <div style="display: flex; align-items: center; gap: 6px;">
      <Tile t={dora} size="xs" />
      <span style="color: var(--fg-tertiary); font-size: 10px; letter-spacing: var(--tracking-wide);">dora · 巡 {junme}</span>
    </div>
  </div>
</div>
