<script>
  import DiscardPileFlat from './DiscardPileFlat.svelte';
  import Wall from './Wall.svelte';

  export let selfTiles = [];
  export let oppTiles = [];
  export let leftTiles = [];
  export let rightTiles = [];
  export let oppRiichiAt = -1;
  export let wall = 0;
  export let dora = undefined;
  export let junme = 0;
  export let round = { wind: '東', num: 1, honba: 0 };

  // xs tile = 24×32. Discard pile: 6 cols × 3 rows = 6*24+5*3 = 159 wide × 3*32+2*3 = 102 tall.
  const DH = 102;
  const wallSize = 168;
  const gap = 16;
</script>

<!-- Wall in the middle + 4 discard piles radiating out. -->
<div style="position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); display: grid; grid-template-columns: {DH}px {gap}px {wallSize}px {gap}px {DH}px; grid-template-rows: {DH}px {gap}px {wallSize}px {gap}px {DH}px; align-items: center; justify-items: center;">
  <!-- opp (top, rotated 180°) -->
  <div style="grid-row: 1; grid-column: 3; transform: rotate(180deg);">
    <DiscardPileFlat tiles={oppTiles.slice(0, 18)} riichiAt={oppRiichiAt ?? -1} />
  </div>

  <!-- 上家 (left, rotated 90° CW) -->
  <div style="grid-row: 3; grid-column: 1; transform: rotate(90deg);">
    <DiscardPileFlat tiles={leftTiles.slice(0, 18)} />
  </div>

  <!-- central wall + plate -->
  <div style="grid-row: 3; grid-column: 3;">
    <Wall remaining={wall} {dora} {junme} {round} size={wallSize} />
  </div>

  <!-- 下家 (right, rotated -90° CCW) -->
  <div style="grid-row: 3; grid-column: 5; transform: rotate(-90deg);">
    <DiscardPileFlat tiles={rightTiles.slice(0, 18)} />
  </div>

  <!-- self (bottom, normal orientation) -->
  <div style="grid-row: 5; grid-column: 3;">
    <DiscardPileFlat tiles={selfTiles.slice(0, 18)} />
  </div>
</div>
