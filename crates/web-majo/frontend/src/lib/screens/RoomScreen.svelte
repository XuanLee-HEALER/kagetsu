<script>
  import { PIG } from '../core/pigments.js';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import DorjeMark from '../core/DorjeMark.svelte';
  import Button from '../core/Button.svelte';
  import Hr from '../core/Hr.svelte';
  import Row from '../core/Row.svelte';
  import DiscoveryPill from '../core/DiscoveryPill.svelte';
  import SeatCard from './multiplayer/SeatCard.svelte';
</script>

<div data-screen-label="Room · 房間" style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); display: flex; flex-direction: column;">
  <header style="height: 72px; background: var(--bg-deepest); border-bottom: 1px solid var(--border-subtle); display: flex; align-items: center; padding: 0 32px; gap: 16px;">
    <DorjeMark size={20} />
    <div>
      <Eyebrow>Room · 房间</Eyebrow>
      <div style="font-family: var(--font-serif); font-size: var(--text-lg); margin-top: 2px;">
        murakami@thinkpad
      </div>
    </div>
    <div style="margin-left: auto; display: flex; gap: 16px; align-items: center;">
      <DiscoveryPill />
      <KeyBadge k="L" label="离开" size="sm" />
    </div>
  </header>

  <main style="flex: 1; display: grid; grid-template-columns: 1fr 360px; gap: 0; overflow: hidden;">
    <section style="padding: 48px 64px; display: flex; flex-direction: column; gap: 32px;">
      <div>
        <Eyebrow>Seats · 座位</Eyebrow>
        <h2 style="font: var(--t-h3); letter-spacing: var(--tracking-tight); margin-top: 6px;">等待玩家就坐</h2>
        <p style="color: var(--fg-secondary); max-width: 520px; margin-top: 8px;">
          当前 3/4 玩家。房主可点击空座位填入 AI，或继续等待。
        </p>
      </div>

      <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px;">
        <SeatCard seat="東" name="murakami" host you />
        <SeatCard seat="南" name="tanaka" />
        <SeatCard seat="西" name="okuda" />
        <SeatCard seat="北" empty />
      </div>
    </section>

    <aside style="background: var(--bg-deep); border-left: 1px solid var(--border-subtle); padding: 32px 24px; overflow-y: auto; display: flex; flex-direction: column; gap: 22px;">
      <section>
        <Eyebrow style="margin-bottom: 10px;">Rules · 规则</Eyebrow>
        <div style="display: flex; flex-direction: column; gap: 8px;">
          <Row label="Format" value="半庄战" />
          <Row label="Multi-ron" value="头跳" />
          <Row label="食断 · 赤宝 · 一发">
            <span style="color: {PIG.ljangkhu};">开</span>
          </Row>
          <Row label="里宝牌">
            <span style="color: {PIG.ljangkhu};">开</span>
          </Row>
          <Row label="Uma" value="20-10-(-10)-(-20)" mono />
          <Row label="Start · Target" value="25,000 · 30,000" mono />
          <Row label="Timer" value="30 秒" mono />
        </div>
      </section>

      <Hr />

      <section>
        <Eyebrow style="margin-bottom: 10px;">Chat · 聊天</Eyebrow>
        <div style="font-size: 12px; color: var(--fg-tertiary); display: flex; flex-direction: column; gap: 6px; font-family: var(--font-mono);">
          <div><span style="color: {PIG.gser};">murakami</span> joined</div>
          <div><span style="color: var(--fg-secondary);">tanaka</span> joined</div>
          <div><span style="color: var(--fg-secondary);">okuda</span> joined</div>
          <div><span style="color: {PIG.ljangkhu};">system</span> waiting 1 more...</div>
        </div>
      </section>

      <div style="margin-top: auto; display: flex; gap: 10px; flex-direction: column;">
        <Button label="Fill empty with AI · 补 AI" />
        <Button label="Start · 開始" primary kb="Enter" />
      </div>
    </aside>
  </main>
</div>
