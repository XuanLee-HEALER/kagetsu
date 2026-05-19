<script>
  import { PIG } from '../core/pigments.js';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import ActionOption from './game/ActionOption.svelte';
</script>

<!-- Action Modal — floating overlay listing available actions
     (riichi / tsumo / ankan / discard / tsumogiri). -->

<!-- scrim -->
<div style="position: absolute; inset: 0; background: rgba(12,12,20,0.65); backdrop-filter: blur(20px) saturate(140%); z-index: 50;"></div>

<!-- modal -->
<div style="position: absolute; left: 50%; top: 44%; transform: translate(-50%, -50%); width: 520px; background: var(--bg-deep); border: 1px solid var(--border-strong); border-radius: var(--radius-xl); box-shadow: var(--shadow-4); padding: var(--space-6); z-index: 51;">
  <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 16px;">
    <div>
      <Eyebrow>Action · 行动</Eyebrow>
      <div style="font-family: var(--font-serif); font-size: var(--text-xl); margin-top: 4px; letter-spacing: var(--tracking-tight);">
        你摸到 <span style="color: {PIG.gser};">七筒</span> · 已聴牌
      </div>
    </div>
    <KeyBadge k="Esc" label="关闭" size="sm" />
  </div>

  <div style="display: flex; flex-direction: column; gap: 2px;">
    <ActionOption k="R" label="立直 · Riichi" tone="primary" selected
      detail="切 四筒  → 聴 三筒 六筒 ・ 見込打点 5,200 / 7,700" />
    <ActionOption k="W" label="自摸 · Tsumo" disabled
      detail="无満和牌役（缺自摸役）" />
    <ActionOption k="K" label="暗杠 · Ankan" disabled
      detail="无可暗杠组" />
    <ActionOption k="D" label="切牌 · Discard"
      detail="选择手牌中一张打出（默认聴牌切）" />
    <ActionOption k="T" label="摸切 · Tsumogiri"
      detail="切出刚摸到的 七筒（不变手牌）" />
    <ActionOption k="9" label="九种九牌 · Kyuushuu" disabled
      detail="非第一巡 · 不可" />
  </div>

  <div style="margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--border-subtle); display: flex; justify-content: space-between; align-items: center; color: var(--fg-tertiary); font-size: 12px;">
    <span>
      <KeyBadge k="↑↓" size="sm" />
      <span style="margin-left: 8px;">选择 ·</span>
      <span style="margin-left: 8px;">
        <KeyBadge k="Enter" size="sm" label="确认" />
      </span>
    </span>
    <span style="font-family: var(--font-mono);">
      或直接按字母键跳过此菜单
    </span>
  </div>
</div>
