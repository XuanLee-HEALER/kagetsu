<script>
  import Tile from './Tile.svelte';

  export let tiles = [];
  export let riichiAt = -1;
  export let rotation = 0;
  export let style = '';

  const cols = 6;
  $: outerStyle = `display: grid; grid-template-columns: repeat(${cols}, max-content); gap: 3px; transform: rotate(${rotation}deg); transform-origin: center center; ${style}`;
</script>

<div style={outerStyle}>
  {#each tiles as t, i}
    <Tile
      {t}
      size="xs"
      state={i === riichiAt ? 'riichi' : 'normal'}
      rotate={i === riichiAt ? 90 : 0}
    />
  {/each}
</div>
