<!-- Result screen — single hand result · 和了 -->
<script>
  import { PIG } from '../core/pigments.js';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import Tile from '../core/Tile.svelte';
  import Row from '../core/Row.svelte';
  import YakuRow from './results/YakuRow.svelte';
  import DeltaRow from './results/DeltaRow.svelte';
</script>

<div
  data-screen-label="Hand result · 和了"
  style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); position: relative; overflow: hidden;"
>
  <!-- protective gradient at bottom — the prayer-flag fade -->
  <div style="position: absolute; left: 0; right: 0; bottom: 0; height: 200px; background: linear-gradient(180deg, transparent 0%, var(--bg-deepest) 100%); pointer-events: none;"></div>

  <header style="padding: 40px 64px 28px; display: flex; align-items: baseline; justify-content: space-between;">
    <div>
      <Eyebrow>Hand · 和了 · 東 2 局</Eyebrow>
      <h1 style="font: var(--t-h1); letter-spacing: var(--tracking-tight); margin-top: 10px; color: {PIG.gser};">立直 · 平和 · 一発</h1>
      <p style="font: var(--t-lede); color: var(--fg-secondary); margin-top: 8px; font-style: italic;">
        7 翻 30 符 · 跳満 · 你自摸了七筒
      </p>
    </div>
    <div style="display: flex; gap: 16px; align-items: center;">
      <KeyBadge k="N" tone="primary" label="下一局" />
      <KeyBadge k="L" label="日志" size="sm" />
    </div>
  </header>

  <main style="padding: 0 64px; display: flex; flex-direction: column; gap: 32px;">
    <!-- winning hand + dora -->
    <section>
      <Eyebrow style="margin-bottom: 14px;">Winning hand · 和了形</Eyebrow>
      <div style="display: flex; gap: 18px; align-items: flex-end;">
        <div style="display: flex; gap: 4px;">
          <Tile t="m1" size="lg" /><Tile t="m2" size="lg" /><Tile t="m3" size="lg" />
        </div>
        <div style="display: flex; gap: 4px;">
          <Tile t="p3" size="lg" /><Tile t="p4" size="lg" /><Tile t="p5r" size="lg" /><Tile t="p6" size="lg" /><Tile t="p7" size="lg" state="draw" />
        </div>
        <div style="display: flex; gap: 4px;">
          <Tile t="s2" size="lg" /><Tile t="s3" size="lg" /><Tile t="s4" size="lg" />
        </div>
        <div style="display: flex; gap: 4px;">
          <Tile t="z1" size="lg" /><Tile t="z1" size="lg" />
        </div>
      </div>
      <div style="margin-top: 14px; display: flex; gap: 28px; color: var(--fg-tertiary); font-size: 13px;">
        <span><span style="color: {PIG.gser};">★</span> 自摸 — Tsumo</span>
        <span>14 张 · 标准型</span>
        <span style="color: {PIG.gser};">七筒 · 待ち牌</span>
      </div>
    </section>

    <!-- yaku breakdown -->
    <section style="display: grid; grid-template-columns: 1fr 360px; gap: 48px;">
      <div>
        <Eyebrow style="margin-bottom: 14px;">Yaku · 役</Eyebrow>
        <div style="display: flex; flex-direction: column; gap: 1px;">
          <YakuRow name="立直 · Riichi" han={1} />
          <YakuRow name="平和 · Pinfu" han={1} />
          <YakuRow name="一発 · Ippatsu" han={1} accent />
          <YakuRow name="門前清自摸和 · Menzen tsumo" han={1} />
          <YakuRow name="一盃口 · Iipeiko" han={1} />
          <YakuRow name="ドラ · Dora" han={1} />
          <YakuRow name="赤ドラ · Akadora" han={1} />
        </div>
        <div style="display: flex; justify-content: space-between; padding: 16px 0; margin-top: 8px; border-top: 1px solid var(--border-default); font-size: var(--text-lg);">
          <span style="color: var(--fg-secondary);">30 符 · 7 翻</span>
          <span style="font-family: var(--font-serif); font-size: var(--text-3xl); color: {PIG.gser}; letter-spacing: var(--tracking-tight);">跳満 · Haneman</span>
        </div>
      </div>

      <div>
        <Eyebrow style="margin-bottom: 14px;">Score · 点数移动</Eyebrow>
        <div style="background: var(--bg-deep); border: 1px solid var(--border-default); border-radius: var(--radius-lg); padding: 20px;">
          <div style="font-size: var(--text-3xl); font-family: var(--font-mono); color: {PIG.gser}; font-weight: 600;">
            +12,000
          </div>
          <div style="color: var(--fg-tertiary); font-size: 13px; margin-top: 4px;">
            自摸 · 跳満 · 親 = 4,000 all
          </div>

          <div style="margin-top: 18px; display: flex; flex-direction: column; gap: 8px;">
            <DeltaRow seat="東 · 自家" delta="+12,000" before={26500} after={38500} you />
            <DeltaRow seat="南 · 下家" delta="-4,000" before={24000} after={20000} />
            <DeltaRow seat="西 · 对家" delta="-4,000" before={24500} after={20500} />
            <DeltaRow seat="北 · 上家" delta="-4,000" before={25000} after={21000} />
          </div>

          <div style="margin-top: 16px; padding: 10px 12px; background: var(--bg-surface0); border-radius: var(--radius-md);">
            <Row label="Honba · 本場" value="1 → 2 (連庄)" />
            <Row label="Kyotaku · 供託" value="1 → 0" />
          </div>
        </div>
      </div>
    </section>

    <!-- ura dora reveal -->
    <section style="display: flex; gap: 28px; align-items: center;">
      <Eyebrow>Ura dora · 裏宝</Eyebrow>
      <div style="display: flex; gap: 4px;">
        <Tile t="m6" size="sm" />
      </div>
      <span style="color: var(--fg-tertiary); font-size: 13px;">
        指示 m6 → ドラ 七萬 · 命中 0 张
      </span>
    </section>
  </main>
</div>
