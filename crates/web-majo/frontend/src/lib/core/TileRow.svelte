<script>
  import Tile from './Tile.svelte';

  export let tiles = [];
  export let size = 'md';
  export let gap = 4;
  export let selected = -1;
  export let drawIdx = -1;
  export let riichiAt = -1;
  export let style = '';
  /** Optional callback (tile, idx) → state string. Overrides selected/drawIdx/riichiAt. */
  export let getState = null;

  $: outerStyle = `display: flex; gap: ${gap}px; align-items: flex-end; ${style}`;
</script>

<div style={outerStyle}>
  {#each tiles as t, i}
    {@const s = getState
      ? (getState(t, i) || 'normal')
      : i === selected
      ? 'selected'
      : i === drawIdx
      ? 'draw'
      : i === riichiAt
      ? 'riichi'
      : 'normal'}
    <Tile {t} {size} state={s} />
  {/each}
</div>
