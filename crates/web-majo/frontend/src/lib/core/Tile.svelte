<script>
  import { PIG } from './pigments.js';
  import { TILE_SIZES, tileSvgPath } from './tile-utils.js';
  import BackInner from './BackInner.svelte';

  export let t = undefined;
  /** @type {'xs'|'sm'|'md'|'lg'} */
  export let size = 'md';
  /** @type {'normal'|'selected'|'draw'|'riichi'|'danger'|'discarded-recent'|'tenpai'} */
  export let state = 'normal';
  export let rotate = 0;
  export let style = '';

  $: d = TILE_SIZES[size];
  $: isBack = !t || t === '?';
  $: src = tileSvgPath(t);

  // Outline + lift + glow per state (mirrors prototype Tile()).
  $: lift = state === 'selected' ? -7 : 0;
  $: outline = state === 'selected' || state === 'draw'
    ? `2px solid ${PIG.gser}`
    : state === 'riichi' || state === 'danger'
    ? `2px solid ${PIG.mtshal}`
    : state === 'discarded-recent'
    ? `1.5px dashed ${PIG.gser}`
    : state === 'tenpai'
    ? `1.5px solid ${PIG.ljangkhu}`
    : '1px solid rgba(27,27,42,0.40)';
  $: glow = state === 'selected'
    ? `, 0 0 0 1px rgba(210,180,80,0.20)`
    : state === 'draw'
    ? `, 0 0 8px rgba(210,180,80,0.45)`
    : '';

  // 3D edge — inset highlight on top, dark depth on bottom/right.
  $: e = d.edge;
  $: shadowParts = [
    isBack ? `inset 0 1px 0 rgba(255,255,255,0.10)` : `inset 0 1px 0 rgba(255,255,255,0.55)`,
    isBack ? `inset -1px 0 0 rgba(0,0,0,0.25)` : `inset -1px 0 0 rgba(0,0,0,0.10)`,
    isBack ? `inset 0 -1px 0 rgba(0,0,0,0.35)` : `inset 0 -1px 0 rgba(0,0,0,0.18)`,
    `0 ${e}px 0 -0.5px #9F7045`,
    `0 ${e + 1}px 0 0 rgba(0,0,0,0.20)`,
    `0 ${e + 3}px ${e * 1.5}px -1px rgba(0,0,0,0.40)`,
  ];
  $: shadow = shadowParts.join(', ') + glow;

  $: outerStyle = [
    `width: ${d.w}px`,
    `height: ${d.h}px`,
    `border-radius: ${d.radius}px`,
    `background: ${isBack ? '#2F3B5B' : '#F7F0DD'}`,
    `border: ${outline}`,
    `box-shadow: ${shadow}`,
    `box-sizing: border-box`,
    `flex-shrink: 0`,
    `transform: translateY(${lift}px) rotate(${rotate}deg)`,
    `transform-origin: center center`,
    `transition: transform 220ms cubic-bezier(0.16,1,0.30,1)`,
    `overflow: hidden`,
    `position: relative`,
    style,
  ].filter(Boolean).join('; ');
</script>

<div style={outerStyle}>
  {#if isBack}
    <BackInner />
  {:else}
    <img
      src={src}
      alt=""
      draggable="false"
      style="width: 100%; height: 100%; display: block; object-fit: contain; padding: 1px; box-sizing: border-box; pointer-events: none; user-select: none;"
    />
  {/if}
</div>
