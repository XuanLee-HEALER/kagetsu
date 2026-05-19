<script>
  import { GAME } from '../core/game-fixture.js';
  import DorjeMark from '../core/DorjeMark.svelte';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import Tile from '../core/Tile.svelte';
  import Stat from './game/Stat.svelte';
  import Divider from './game/Divider.svelte';
  import Table from './game/Table.svelte';
  import HandStrip from './game/HandStrip.svelte';
  import SidePanel from './game/SidePanel.svelte';
  import ActionModal from './ActionModal.svelte';

  export let showActionModal = false;
  /** @type {'NORMAL'|'COMMAND'} */
  export let mode = 'NORMAL';
  export let commandText = 'discard p4';

  const g = GAME;
  $: me = g.players[0];

  $: kyotakuTone = g.round.riichi > 0 ? 'warning' : null;
  $: wallTone = g.wall < 16 ? 'warning' : null;
</script>

<!--
  Main game screen — single player / Standard mode.
  Layout: top status bar · 4-player table · bottom hand · right info rail.
-->
<div data-screen-label="Game · main"
  style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); position: relative; display: grid; grid-template-columns: 1fr 320px; grid-template-rows: 56px 1fr 156px; grid-template-areas: 'top top' 'table side' 'hand side';">

  <!-- ── top status bar ────────────────────────────────────── -->
  <header style="grid-area: top; background: var(--bg-deepest); border-bottom: 1px solid var(--border-subtle); display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; padding: 0 32px; backdrop-filter: blur(20px) saturate(140%);">
    <div style="display: flex; gap: 24px; align-items: center;">
      <div style="display: flex; align-items: center; gap: 10px;">
        <DorjeMark size={20} />
        <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: var(--fg-tertiary);">tui-majo</span>
      </div>
      <Divider />
      <Stat label="Round · 局" value={`${g.round.wind} ${g.round.num}`} />
      <Stat label="Honba · 本場" value={String(g.round.honba)} />
      <Stat label="Kyotaku · 供託" value={String(g.round.riichi)} tone={kyotakuTone} />
    </div>

    <div style="display: flex; align-items: center; gap: 14px; justify-content: center;">
      <Eyebrow>Dora · 宝牌</Eyebrow>
      <Tile t={g.dora[0]} size="xs" />
      <Tile t="?" size="xs" />
      <Tile t="?" size="xs" />
    </div>

    <div style="display: flex; gap: 20px; align-items: center; justify-content: flex-end;">
      <Stat label="Junme · 巡" value={String(g.junme)} />
      <Stat label="Wall · 山" value={String(g.wall)} tone={wallTone} />
      <Divider />
      <Stat label="" value="20:14" mono />
      <KeyBadge k="?" label="Help" size="sm" />
      <KeyBadge k="Esc" label="Menu" size="sm" />
    </div>
  </header>

  <!-- ── table ─────────────────────────────────────────────── -->
  <section style="grid-area: table; position: relative; overflow: hidden;">
    <Table players={g.players} dora={g.dora[0]} wall={g.wall} junme={g.junme} round={g.round} />
  </section>

  <!-- ── own hand strip ─────────────────────────────────────── -->
  <section style="grid-area: hand; background: var(--bg-deep); border-top: 1px solid var(--border-subtle); padding: 20px 32px 16px; display: flex; flex-direction: column; gap: 12px;">
    <HandStrip {me} {mode} {commandText} />
  </section>

  <!-- ── right rail ─────────────────────────────────────────── -->
  <aside style="grid-area: side; background: var(--bg-deep); border-left: 1px solid var(--border-subtle); padding: 20px 20px 16px; overflow-y: auto; display: flex; flex-direction: column; gap: 20px;">
    <SidePanel game={g} {me} />
  </aside>

  {#if showActionModal}
    <ActionModal />
  {/if}
</div>
