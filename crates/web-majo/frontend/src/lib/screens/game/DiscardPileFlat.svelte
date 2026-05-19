<script>
  import Tile from '../../core/Tile.svelte';

  export let tiles = [];
  export let riichiAt = -1;
</script>

<!-- 6 cols × 3 rows discard pile with no internal rotation. -->
<div style="display: grid; grid-template-columns: repeat(6, max-content); grid-auto-rows: max-content; gap: 3px;">
  {#each tiles as t, i}
    {@const state = i === riichiAt
      ? 'riichi'
      : i === tiles.length - 1
      ? 'discarded-recent'
      : 'normal'}
    {@const rotate = i === riichiAt ? 90 : 0}
    <Tile {t} size="xs" {state} {rotate} />
  {/each}
</div>
