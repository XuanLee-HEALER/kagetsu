<script>
  export let filled = false;
  /** @type {'horizontal'|'vertical'} */
  export let variant = 'horizontal';
  export let cursor = false;

  $: isV = variant === 'vertical';
  $: w = isV ? 8 : 12;
  $: h = isV ? 12 : 8;
  $: bg = filled
    ? 'linear-gradient(180deg, #2A3A60 0%, #1F2A48 60%, #142038 100%)'
    : 'transparent';
  $: border = filled
    ? '1px solid #0F1830'
    : `1px dashed ${cursor ? 'rgba(210,180,80,0.6)' : 'rgba(232,224,212,0.12)'}`;
  $: shadow = filled
    ? '0 1px 0 rgba(0,0,0,0.4), inset 0 0.5px 0 rgba(255,255,255,0.10)'
    : cursor ? '0 0 4px rgba(210,180,80,0.45)' : 'none';
</script>

<!-- Single slot of the central wall (filled tile-back or empty draw target). -->
<div style="width: {w}px; height: {h}px; background: {bg}; border: {border}; border-radius: 1.5px; box-sizing: border-box; box-shadow: {shadow};"></div>
