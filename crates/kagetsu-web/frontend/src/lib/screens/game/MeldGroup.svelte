<script>
  import Tile from '../../core/Tile.svelte';

  export let meld = { type: 'chi', tiles: [] };
</script>

<!-- Single meld group (chi / pon / kan). First tile rotates -90° for chi/pon. -->
<div style="display: flex; gap: 1px; align-items: flex-end;">
  {#each meld.tiles as t, i}
    {@const rotate = (meld.type === 'chi' && i === 0) || (meld.type === 'pon' && i === 0) ? -90 : 0}
    <Tile {t} size="xs" {rotate} />
  {/each}
</div>
