<script>
  import TileRow from '../../core/TileRow.svelte';
  import SeatLabelInline from './SeatLabelInline.svelte';
  import CenterCluster from './CenterCluster.svelte';
  import MeldGroup from './MeldGroup.svelte';

  export let players = [];
  export let dora = undefined;
  export let wall = 0;
  export let junme = 0;
  export let round = { wind: '東', num: 1, honba: 0 };

  $: me = players[0] ?? {};
  $: shimo = players[1] ?? {};
  $: toi = players[2] ?? {};
  $: kami = players[3] ?? {};

  const backs = Array(13).fill('?');
</script>

<!--
  Real-mahjong-table layout. From outside in:
    each player's HAND (at edge) → their DISCARDS → the WALL (center) → plate.
  Discards sit between each player's hand and the wall — "above" their hand
  from their POV. Wall is in the very center, showing remaining tile count + dora.
-->
<div style="position: absolute; inset: 0; display: flex; justify-content: center; align-items: center;">
  <div style="width: 920px; height: 600px; position: relative;">
    <!-- opponent hand backs at edges -->
    <div style="position: absolute; top: 14px; left: 50%; transform: translateX(-50%) rotate(180deg);">
      <TileRow tiles={backs} size="xs" gap={3} />
    </div>

    <div style="position: absolute; left: 14px; top: 50%; transform: translateY(-50%) rotate(90deg); transform-origin: center center;">
      <TileRow tiles={backs} size="xs" gap={3} />
    </div>

    <div style="position: absolute; right: 14px; top: 50%; transform: translateY(-50%) rotate(-90deg); transform-origin: center center;">
      <TileRow tiles={backs} size="xs" gap={3} />
    </div>

    <!-- seat labels at table corners — each near their player's hand -->
    <SeatLabelInline
      seat={toi.seat}
      name={toi.name}
      score={toi.score}
      riichi={toi.riichi}
      dealer={toi.dealer}
      style="position: absolute; top: 60px; right: 36px;"
    />
    <SeatLabelInline
      seat={kami.seat}
      name={kami.name}
      score={kami.score}
      riichi={kami.riichi}
      dealer={kami.dealer}
      style="position: absolute; top: 60px; left: 36px;"
    />
    <SeatLabelInline
      seat={shimo.seat}
      name={shimo.name}
      score={shimo.score}
      riichi={shimo.riichi}
      dealer={shimo.dealer}
      style="position: absolute; bottom: 60px; right: 36px;"
    />
    <SeatLabelInline
      seat={me.seat}
      name={me.name}
      score={me.score}
      riichi={me.riichi}
      dealer={me.dealer}
      you
      style="position: absolute; bottom: 60px; left: 36px;"
    />

    <!-- center cluster — wall in middle + discards radiating out -->
    <CenterCluster
      selfTiles={me.discards ?? []}
      oppTiles={toi.discards ?? []}
      leftTiles={kami.discards ?? []}
      rightTiles={shimo.discards ?? []}
      oppRiichiAt={toi.riichiAt}
      {wall}
      {dora}
      {junme}
      {round}
    />

    <!-- melds for kami/shimo near their hands -->
    <div style="position: absolute; bottom: 96px; left: 14px; display: flex; gap: 4px; transform: rotate(90deg); transform-origin: left bottom;">
      {#each kami.melds ?? [] as m}
        <MeldGroup meld={m} />
      {/each}
    </div>
    <div style="position: absolute; top: 96px; right: 14px; display: flex; gap: 4px; transform: rotate(-90deg); transform-origin: right top;">
      {#each shimo.melds ?? [] as m}
        <MeldGroup meld={m} />
      {/each}
    </div>
  </div>
</div>
