<script>
  import { PIG } from '../core/pigments.js';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import DorjeMark from '../core/DorjeMark.svelte';
  import Button from '../core/Button.svelte';
  import Hr from '../core/Hr.svelte';
  import Row from '../core/Row.svelte';
  import DiscoveryPill from '../core/DiscoveryPill.svelte';
  import RoomRow from './multiplayer/RoomRow.svelte';
  import PlayerBadge from './multiplayer/PlayerBadge.svelte';
</script>

<div data-screen-label="Lobby · 大廳" style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); display: grid; grid-template-rows: 72px 1fr 64px;">

  <header style="background: var(--bg-deepest); border-bottom: 1px solid var(--border-subtle); display: flex; align-items: center; padding: 0 32px; gap: 16px;">
    <DorjeMark size={20} />
    <div>
      <Eyebrow>Local network · 局域网</Eyebrow>
      <div style="font-family: var(--font-serif); font-size: var(--text-lg); letter-spacing: var(--tracking-tight); margin-top: 2px;">大厅</div>
    </div>
    <div style="margin-left: auto; display: flex; gap: 18px; align-items: center;">
      <DiscoveryPill />
      <KeyBadge k="R" label="刷新" size="sm" />
      <KeyBadge k="C" tone="primary" label="创建房间" size="sm" />
    </div>
  </header>

  <main style="display: grid; grid-template-columns: 1.5fr 1fr; gap: 32px; padding: 32px 48px; overflow: hidden;">
    <!-- room list -->
    <section style="display: flex; flex-direction: column; gap: 16px; overflow: hidden;">
      <div style="display: flex; justify-content: space-between; align-items: baseline;">
        <Eyebrow>Rooms · 房间 (3)</Eyebrow>
        <span style="color: var(--fg-tertiary); font-size: 12px; font-family: var(--font-mono);">
          mDNS · gossipsub · 5 秒刷新
        </span>
      </div>

      <div style="display: flex; flex-direction: column; gap: 10px; overflow-y: auto; padding-right: 6px;">
        <RoomRow active host="murakami@thinkpad" mode="Standard" players={2} max={4}
          addr="10.0.0.42:4321" rules="半庄 · 食断 · 赤宝" />
        <RoomRow host="tanaka@air" mode="ZeroTrust" players={3} max={4}
          addr="10.0.0.18:5142" rules="半庄 · 头跳" highlight="zt" />
        <RoomRow host="okuda@imac" mode="Standard" players={1} max={4}
          addr="10.0.0.91:4099" rules="东风 · 一发 · 里宝" />
      </div>

      <!-- manual fallback -->
      <div style="margin-top: 4px; padding: 14px 18px; background: var(--bg-surface0); border: 1px solid var(--border-default); border-radius: var(--radius-lg);">
        <Eyebrow style="margin-bottom: 10px;">Manual · multiaddr</Eyebrow>
        <div style="display: flex; gap: 10px; align-items: center;">
          <input readonly value="/ip4/10.0.0.42/udp/4321/quic-v1/p2p/12D3KooWHJ..."
            style="flex: 1; height: 36px; padding: 0 12px; background: var(--bg-base); border: 1px solid var(--border-default); border-radius: var(--radius-md); color: var(--fg-primary); font-family: var(--font-mono); font-size: 12px;" />
          <Button label="Join" />
        </div>
        <div style="color: var(--fg-tertiary); font-size: 12px; margin-top: 8px;">
          mDNS 失效或跨子网时使用 · QUIC over TCP fallback
        </div>
      </div>
    </section>

    <!-- side: details / status -->
    <aside style="background: var(--bg-deep); border: 1px solid var(--border-subtle); border-radius: var(--radius-lg); padding: 20px 22px; display: flex; flex-direction: column; gap: 18px;">
      <section>
        <Eyebrow style="margin-bottom: 10px;">Selected · 选中房间</Eyebrow>
        <div style="font-family: var(--font-serif); font-size: var(--text-2xl); letter-spacing: var(--tracking-tight); color: {PIG.gser};">murakami</div>
        <div style="color: var(--fg-tertiary); font-size: 13px; margin-top: 4px;">
          @thinkpad · 10.0.0.42
        </div>
      </section>

      <Hr />

      <section style="display: flex; flex-direction: column; gap: 8px;">
        <Row label="Mode · 模式">
          <span style="color: {PIG.gser};">Standard</span>
        </Row>
        <Row label="Rules · 规则" value="半庄 · 食断 · 赤宝" />
        <Row label="Players · 玩家" value="2 / 4" mono />
        <Row label="AI fill · 补满" value="空座位补 AI" />
        <Row label="Timer · 计时" value="30 秒 / 步" mono />
        <Row label="Seed · 种子" value="random" mono />
      </section>

      <Hr />

      <section>
        <Eyebrow style="margin-bottom: 10px;">Players · 在房</Eyebrow>
        <div style="display: flex; flex-direction: column; gap: 8px;">
          <PlayerBadge seat="東" name="murakami" host />
          <PlayerBadge seat="南" name="tanaka" />
          <PlayerBadge seat="西" empty />
          <PlayerBadge seat="北" empty />
        </div>
      </section>

      <div style="margin-top: auto; display: flex; gap: 10px;">
        <Button label="Spectate · 観戦" />
        <Button label="Join · 入る" primary kb="Enter" />
      </div>
    </aside>
  </main>

  <footer style="border-top: 1px solid var(--border-subtle); background: var(--bg-deepest); display: flex; align-items: center; justify-content: space-between; padding: 0 32px;">
    <div style="display: flex; gap: 14px;">
      <KeyBadge k="↑↓" label="选" size="sm" />
      <KeyBadge k="Enter" label="加入" size="sm" />
      <KeyBadge k="C" label="创建" size="sm" />
      <KeyBadge k="S" label="観戦" size="sm" />
    </div>
    <div style="display: flex; gap: 14px;">
      <KeyBadge k=":" label="命令" size="sm" />
      <KeyBadge k="Esc" label="返回" size="sm" />
    </div>
  </footer>
</div>
