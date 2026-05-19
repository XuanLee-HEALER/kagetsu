<script>
  // ZeroTrust game screen — variant of GameScreen showing mental poker protocol state.
  // Different keys, protocol indicator, joint shuffle status.
  import DorjeMark from '../core/DorjeMark.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import { GAME } from '../core/game-fixture.js';
  import Table from './game/Table.svelte';
  import ZTBadge from './zerotrust/ZTBadge.svelte';
  import ProtocolPill from './zerotrust/ProtocolPill.svelte';
  import ZTHandStrip from './zerotrust/ZTHandStrip.svelte';
  import ZTSidePanel from './zerotrust/ZTSidePanel.svelte';

  const g = GAME;
  const me = g.players[0];
</script>

<div data-screen-label="ZeroTrust · game" style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); display: grid; grid-template-columns: 1fr 340px; grid-template-rows: 56px 1fr 156px; grid-template-areas: 'top top' 'table side' 'hand side'; position: relative;">

  <!-- top status bar — adds protocol pill -->
  <header style="grid-area: top; background: var(--bg-deepest); border-bottom: 1px solid var(--border-subtle); display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; padding: 0 32px;">
    <div style="display: flex; gap: 16px; align-items: center;">
      <div style="display: flex; align-items: center; gap: 10px;">
        <DorjeMark size={20} />
        <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: var(--fg-tertiary);">tui-majo</span>
      </div>
      <ZTBadge />
      <ProtocolPill step={2} label="Draw · 摸牌" />
    </div>

    <div style="display: flex; align-items: center; gap: 14px; justify-content: center;">
      <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: var(--fg-tertiary);">東 2 局 · 1 本場 · 巡 8</span>
    </div>

    <div style="display: flex; gap: 16px; align-items: center; justify-content: flex-end;">
      <span style="color: var(--fg-tertiary); font-size: 12px; font-family: var(--font-mono);">Sako-Killian · K=80</span>
      <span style="width: 1px; height: 24px; background: var(--border-default);"></span>
      <KeyBadge k="P" label="Protocol" size="sm" />
      <KeyBadge k="Esc" label="Leave" size="sm" />
    </div>
  </header>

  <section style="grid-area: table; position: relative; overflow: hidden;">
    <Table players={g.players} dora={g.dora[0]} wall={g.wall} junme={g.junme} round={g.round} />

    <!-- protocol overlay tint — barely visible blue, signals ZT -->
    <div style="position: absolute; inset: 0; background: radial-gradient(circle at center, rgba(91,138,184,0.04) 0%, transparent 60%); pointer-events: none;"></div>
  </section>

  <section style="grid-area: hand; background: var(--bg-deep); border-top: 1px solid var(--border-subtle); padding: 20px 32px 16px; display: flex; flex-direction: column; gap: 12px;">
    <ZTHandStrip {me} />
  </section>

  <aside style="grid-area: side; background: var(--bg-deep); border-left: 1px solid var(--border-subtle); padding: 20px 20px 16px; overflow-y: auto; display: flex; flex-direction: column; gap: 20px;">
    <ZTSidePanel game={g} />
  </aside>
</div>
