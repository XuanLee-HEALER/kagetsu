<!-- Result screen — match end · 終局 -->
<script>
  import { PIG } from '../core/pigments.js';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import StandingRow from './results/StandingRow.svelte';
  import BigStat from './results/BigStat.svelte';

  const results = [
    { rank: 1, seat: '西', name: '对家', base: 39500, uma: '+30', final: 42500 },
    { rank: 2, seat: '東', name: '你',   base: 28500, uma: '+10', final: 29500, you: true },
    { rank: 3, seat: '北', name: '上家', base: 18500, uma: '-10', final: 17500 },
    { rank: 4, seat: '南', name: '下家', base: 13500, uma: '-30', final: 10500 },
  ];
</script>

<div
  data-screen-label="Match end · 終局"
  style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); position: relative; overflow: hidden;"
>
  <header style="padding: 56px 64px 36px;">
    <Eyebrow>Match end · 終局 · 半庄战</Eyebrow>
    <h1 style="font: var(--t-h1); letter-spacing: var(--tracking-tight); margin-top: 12px;">
      <span style="color: {PIG.gser};">对家</span>
      <span style="color: var(--fg-secondary); margin: 0 14px; font-weight: 300;">·</span>
      <span style="font-style: italic; color: var(--fg-secondary); font-family: var(--font-serif);">winner</span>
    </h1>
    <p style="margin-top: 8px; color: var(--fg-tertiary);">
      25,000 起点 · 30,000 终点 · uma 20-10-(-10)-(-20) · 8 局 1 时 14 分
    </p>
  </header>

  <main style="padding: 0 64px; display: grid; grid-template-columns: 1fr 1fr; gap: 56px;">
    <!-- standings -->
    <section>
      <Eyebrow style="margin-bottom: 16px;">Standings · 顺位</Eyebrow>
      <div style="display: flex; flex-direction: column; gap: 10px;">
        {#each results as r (r.rank)}
          <StandingRow {r} />
        {/each}
      </div>
    </section>

    <!-- stats -->
    <section>
      <Eyebrow style="margin-bottom: 16px;">Your match · 你的本场</Eyebrow>
      <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 12px;">
        <BigStat label="和了率" value="33%" sub="3 / 9 局" />
        <BigStat label="放铳率" value="11%" sub="1 / 9 局" />
        <BigStat label="副露率" value="22%" sub="2 / 9 局" />
        <BigStat label="平均打点" value="6,800" sub="跳满 ×1 · 满貫 ×1" mono />
        <BigStat label="立直" value="3 回" sub="2 / 3 命中" />
        <BigStat label="役満" value="0 回" sub="—" muted />
      </div>
    </section>
  </main>

  <footer style="position: absolute; bottom: 0; left: 0; right: 0; padding: 20px 64px; background: var(--bg-deepest); border-top: 1px solid var(--border-subtle); display: flex; align-items: center; justify-content: space-between;">
    <div style="display: flex; gap: 14px;">
      <KeyBadge k="N" tone="primary" label="再来一局" />
      <KeyBadge k="S" label="保存牌谱" size="sm" />
      <KeyBadge k="R" label="重放" size="sm" />
    </div>
    <KeyBadge k="Esc" label="主菜单" size="sm" />
  </footer>
</div>
