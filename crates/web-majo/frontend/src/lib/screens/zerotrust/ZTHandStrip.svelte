<script>
  // Hand strip for ZeroTrust — same row of 13 + draw, ZT-specific key cluster + P2P pill.
  import Tile from '../../core/Tile.svelte';
  import TileRow from '../../core/TileRow.svelte';
  import KeyBadge from '../../core/KeyBadge.svelte';
  import { PIG } from '../../core/pigments.js';

  export let me;
</script>

<div style="display: flex; align-items: flex-end; gap: 20px; justify-content: center;">
  <TileRow tiles={me.hand} size="md" gap={3} selected={6} />
  <div style="width: 1px; height: 52px; background: var(--border-default);"></div>
  <Tile t={me.draw} size="md" state="draw" />
</div>

<div style="display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-top: 6px;">
  <div style="display: flex; align-items: center; gap: 16px; flex-wrap: wrap;">
    <KeyBadge k="D" tone="primary" label="摸下一张" />
    <KeyBadge k="Enter" label="弃 cursor 牌" />
    <KeyBadge k="←→" label="移动 cursor" />
    <span style="width: 1px; height: 18px; background: var(--border-default);"></span>
    <KeyBadge k="R" label="揭示宝牌" />
    <KeyBadge k="C" label="吃" /><KeyBadge k="P" label="碰" />
    <KeyBadge k="K" label="明杠" /><KeyBadge k="A" label="暗杠" />
    <KeyBadge k="X" label="加杠" />
    <span style="width: 1px; height: 18px; background: var(--border-default);"></span>
    <KeyBadge k="I" tone="danger" label="立直" />
    <KeyBadge k="T" tone="ok" label="自摸" />
    <KeyBadge k="N" tone="ok" label="荣和" />
  </div>

  <div style="display: flex; align-items: center; gap: 10px;">
    <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: {PIG.ngonpo}; background: rgba(91,138,184,0.16); padding: 4px 12px; border-radius: var(--radius-sm); font-weight: 600; font-size: 11px;">P2P</span>
    <KeyBadge k="L" label="Leave" size="sm" />
  </div>
</div>
