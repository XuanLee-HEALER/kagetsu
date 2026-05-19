<script>
  import { PIG } from '../../core/pigments.js';
  import Tile from '../../core/Tile.svelte';
  import TileRow from '../../core/TileRow.svelte';
  import KeyBadge from '../../core/KeyBadge.svelte';

  export let me = { hand: [], draw: undefined };
  /** @type {'NORMAL'|'COMMAND'} */
  export let mode = 'NORMAL';
  export let commandText = 'discard p4';

  $: modeColor = mode === 'COMMAND' ? PIG.gser : PIG.ljangkhu;
  $: modeBg = mode === 'COMMAND' ? 'var(--accent-soft)' : 'var(--status-success-bg)';
</script>

<!-- hand row -->
<div style="display: flex; align-items: flex-end; gap: 20px; justify-content: center;">
  <TileRow tiles={me.hand} size="md" gap={3} selected={6} />
  <div style="width: 1px; height: 52px; background: var(--border-default);"></div>
  <Tile t={me.draw} size="md" state="draw" />
</div>

<!-- action band -->
<div style="display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-top: 6px;">
  <div style="display: flex; align-items: center; gap: 16px; flex-wrap: wrap;">
    {#if mode === 'COMMAND'}
      <div style="display: flex; align-items: center; gap: 4px; padding: 6px 12px; background: var(--bg-surface0); border: 1px solid var(--border-focus); border-radius: var(--radius-md); font-family: var(--font-mono); font-size: 14px; min-width: 280px;">
        <span style="color: {PIG.gser}; font-weight: 600;">:</span>
        <span style="color: var(--fg-primary);">{commandText}</span>
        <span style="display: inline-block; width: 8px; height: 16px; background: {PIG.gser}; margin-left: 2px; animation: blink 1s steps(2) infinite;"></span>
      </div>
    {:else}
      <KeyBadge k="1-9" tone="primary" label="选牌" />
      <KeyBadge k="D" label="切" />
      <KeyBadge k="T" label="摸切" />
      <KeyBadge k="R" tone="danger" label="立直" />
      <KeyBadge k="W" tone="ok" label="自摸" />
      <KeyBadge k="K" label="暗杠" />
      <span style="width: 1px; height: 18px; background: var(--border-default);"></span>
      <KeyBadge k=":" label="命令" />
      <KeyBadge k="M" label="菜单" />
    {/if}
  </div>

  <div style="display: flex; align-items: center; gap: 10px;">
    <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: {modeColor}; background: {modeBg}; padding: 4px 12px; border-radius: var(--radius-sm); font-weight: 600; font-size: 11px;">
      {mode}
    </span>
    <span style="color: var(--fg-tertiary); font-size: 12px; font-family: var(--font-mono);">
      h j k l · ← →
    </span>
  </div>
</div>

<style>
  /* blink animation for the COMMAND-mode cursor caret.
     Declared with the -global- prefix so the rule isn't scoped, since the
     animation is referenced via inline-style elsewhere in this component. */
  @keyframes -global-blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0; }
  }
</style>
