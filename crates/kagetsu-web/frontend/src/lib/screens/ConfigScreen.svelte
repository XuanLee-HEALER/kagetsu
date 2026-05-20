<script>
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import DorjeMark from '../core/DorjeMark.svelte';
  import Button from '../core/Button.svelte';
  import NavItem from './pregame/NavItem.svelte';
  import ConfigGroup from './pregame/ConfigGroup.svelte';
  import Radio from './pregame/Radio.svelte';
  import Toggle from './pregame/Toggle.svelte';
</script>

<div data-screen-label="Config · 設定" style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); display: grid; grid-template-columns: 260px 1fr; grid-template-rows: 72px 1fr 72px; grid-template-areas: 'head head' 'nav body' 'foot foot';">

  <header style="grid-area: head; background: var(--bg-deepest); border-bottom: 1px solid var(--border-subtle); display: flex; align-items: center; padding: 0 32px; gap: 16px;">
    <DorjeMark size={20} />
    <div>
      <Eyebrow>Pre-game · 设定</Eyebrow>
      <div style="font-family: var(--font-serif); font-size: var(--text-lg); letter-spacing: var(--tracking-tight); margin-top: 2px;">对局配置</div>
    </div>
    <div style="margin-left: auto; display: flex; gap: 10px; align-items: center;">
      <KeyBadge k="Esc" label="返回" size="sm" />
    </div>
  </header>

  <nav style="grid-area: nav; background: var(--bg-deep); border-right: 1px solid var(--border-subtle); padding: 20px 0;">
    <NavItem n="01" label="赛制 · Format" active />
    <NavItem n="02" label="役种 · Yaku" />
    <NavItem n="03" label="点棒 · Points" />
    <NavItem n="04" label="Uma" />
    <NavItem n="05" label="计时 · Timer" />
    <NavItem n="06" label="主题 · Theme" />
    <NavItem n="07" label="种子 · Seed" />
  </nav>

  <main style="grid-area: body; overflow-y: auto; padding: 32px 64px;">
    <div style="max-width: 720px;">
      <Eyebrow>Section 01</Eyebrow>
      <h2 style="font: var(--t-h3); letter-spacing: var(--tracking-tight); margin-top: 8px; margin-bottom: 6px;">赛制 · Format</h2>
      <p style="color: var(--fg-secondary); max-width: 540px; margin-bottom: 28px;">
        选择庄数与终局规则。半庄战为标准 8 局 (东南各 4)，东风战仅 4 局。
      </p>

      <ConfigGroup label="Game length · 庄数">
        <Radio name="length" label="半庄战 · Hanchan" sub="东 + 南 · 8 局" active />
        <Radio name="length" label="东风战 · Tonpuusen" sub="仅东风 · 4 局" />
      </ConfigGroup>

      <ConfigGroup label="Multi-ron · 多家荣和">
        <Radio name="ron" label="头跳 · Atamahane" sub="按上家顺序仅头家获胜" active />
        <Radio name="ron" label="双家荣 · Double ron" sub="允许两家同时荣和" />
        <Radio name="ron" label="三家荣 · Triple ron" sub="三家荣和 = 流局 → 三家和" />
      </ConfigGroup>

      <ConfigGroup label="Special rules · 特殊規定">
        <Toggle label="食断 · Kuitan" sub="开放副露后断幺九成立" on />
        <Toggle label="赤宝牌 · Red five" sub="三色五各一张赤" on />
        <Toggle label="一发 · Ippatsu" sub="立直后一巡内和牌" on />
        <Toggle label="里宝牌 · Uradora" sub="立直和牌时翻里宝指示" on />
        <Toggle label="数役満 · Counted yakuman" sub="13+ 番按役満计算" />
        <Toggle label="W役満 · Double yakuman" sub="允许双重役満" />
      </ConfigGroup>

      <ConfigGroup label="End condition · 終局">
        <Toggle label="西入 · Westward extension" sub="到南四亲未达终局点 → 继续" />
        <Toggle label="击飞 · Tobi" sub="任一家点数 < 0 立即終局" on />
      </ConfigGroup>
    </div>
  </main>

  <footer style="grid-area: foot; border-top: 1px solid var(--border-subtle); background: var(--bg-deepest); display: flex; align-items: center; justify-content: space-between; padding: 0 32px;">
    <div style="display: flex; gap: 14px;">
      <KeyBadge k="Tab" label="下一节" size="sm" />
      <KeyBadge k="←→" label="切换选项" size="sm" />
    </div>
    <div style="display: flex; gap: 12px;">
      <Button label="重置默认" />
      <Button label="开始对局" primary kb="Enter" />
    </div>
  </footer>
</div>
