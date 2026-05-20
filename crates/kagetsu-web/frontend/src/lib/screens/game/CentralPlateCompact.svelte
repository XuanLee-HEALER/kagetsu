<script>
  import { PIG } from '../../core/pigments.js';
  import Tile from '../../core/Tile.svelte';
  import KV from './KV.svelte';

  export let dora = undefined;
  export let wall = 0;
  export let junme = 0;
  export let round = { wind: '東', num: 1, honba: 0 };
  export let w = 0;
  export let h = 0;
</script>

<!-- Compact central plate (no absolute positioning — usable inside a grid cell). -->
<div style="width: {w}px; height: {h}px; background: var(--bg-deepest); border: 1px solid var(--border-default); border-radius: var(--radius-lg); padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; justify-content: space-between; box-sizing: border-box; box-shadow: var(--shadow-2);">
  <div style="display: flex; align-items: center; gap: 8px; justify-content: center;">
    <span style="font-family: var(--font-serif); font-size: var(--text-xl); color: {PIG.gser}; letter-spacing: var(--tracking-tight);">{round.wind} {round.num}</span>
    <span style="color: var(--fg-tertiary); font-size: 12px;">·</span>
    <span style="color: var(--fg-secondary); font-size: 12px;">{round.honba} 本場</span>
  </div>
  <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; text-align: center;">
    <KV label="Wall · 山" value={String(wall)} />
    <KV label="Junme · 巡" value={String(junme)} />
  </div>
  <div style="display: flex; justify-content: center; gap: 6px; align-items: center;">
    <Tile t={dora} size="xs" />
    <span style="color: var(--fg-tertiary); font-size: 11px; letter-spacing: var(--tracking-wide);">dora</span>
  </div>
</div>
