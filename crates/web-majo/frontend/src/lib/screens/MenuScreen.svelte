<script>
  import { PIG } from '../core/pigments.js';
  import Eyebrow from '../core/Eyebrow.svelte';
  import KeyBadge from '../core/KeyBadge.svelte';
  import ThreeStripes from '../core/ThreeStripes.svelte';

  const ITEMS = [
    { k: '1', title: 'Single player', subtitle: '单人对局 · AI 补满空座', hover: true, muted: false },
    { k: '2', title: 'LAN game', subtitle: '局域网 · libp2p mDNS 发现', hover: false, muted: false },
    { k: '3', title: 'ZeroTrust', subtitle: 'P2P mental poker · 无需信任房主', hover: false, muted: false },
    { k: '4', title: 'Replay', subtitle: '重放天凤 mjlog', hover: false, muted: false },
    { k: '5', title: 'Settings', subtitle: '规则 · 主题 · 计时', hover: false, muted: false },
  ];
</script>

<div data-screen-label="Menu · launcher" style="width: 1440px; height: 900px; background: var(--bg-base); color: var(--fg-primary); font-family: var(--font-sans); display: grid; grid-template-columns: 1fr 480px 1fr; position: relative; overflow: hidden;">

  <!-- left column — display title + lineage -->
  <div style="display: flex; flex-direction: column; justify-content: center; padding: 80px 64px;">
    <Eyebrow>Welcome · 帰宅</Eyebrow>
    <h1 style="font: var(--t-h1); letter-spacing: var(--tracking-tight); margin-top: 14px; color: var(--fg-primary);">tui-majo</h1>
    <div style="font-family: var(--font-serif); font-style: italic; font-size: var(--text-lg); color: var(--fg-secondary); margin-top: 8px; max-width: 400px;">
      A single dark surface for one quiet game of riichi.
    </div>

    <div style="margin-top: 64px; font-family: var(--font-mono); font-size: 12px; color: var(--fg-tertiary); line-height: 1.8;">
      <div>v2.1.0 · WRC 2022 rules</div>
      <div>local · libp2p · zerotrust</div>
      <div>403 unit tests · 99 replay verifications</div>
    </div>

    <div style="margin-top: 48px; display: flex; gap: 12px; align-items: center;">
      <ThreeStripes />
      <span style="font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: var(--fg-tertiary);">
        Sakya · 萨迦
      </span>
    </div>
  </div>

  <!-- center column — menu -->
  <div style="display: flex; flex-direction: column; justify-content: center; padding: 80px 0;">
    {#each ITEMS as item}
      <div style="padding: 18px 32px; border-left: 3px solid {item.hover ? PIG.gser : 'transparent'}; background: {item.hover ? 'var(--accent-soft)' : 'transparent'}; display: grid; grid-template-columns: auto 1fr; align-items: center; gap: 20px; transition: background 200ms var(--ease-out); cursor: default; opacity: {item.muted ? 0.5 : 1};">
        <span style="font-family: var(--font-mono); font-size: var(--text-lg); color: {item.hover ? PIG.gser : 'var(--fg-tertiary)'}; font-weight: 500; width: 28px;">{item.k}</span>
        <div>
          <div style="font-family: var(--font-serif); font-weight: 400; font-size: var(--text-xl); color: var(--fg-primary); letter-spacing: var(--tracking-tight); line-height: 1.2;">{item.title}</div>
          <div style="font-size: var(--text-sm); color: var(--fg-tertiary); margin-top: 4px;">{item.subtitle}</div>
        </div>
      </div>
    {/each}

    <div style="height: 24px;"></div>

    <div style="padding: 18px 32px; border-left: 3px solid transparent; background: transparent; display: grid; grid-template-columns: auto 1fr; align-items: center; gap: 20px; cursor: default; opacity: 0.5;">
      <span style="font-family: var(--font-mono); font-size: var(--text-lg); color: var(--fg-tertiary); font-weight: 500; width: 28px;">Q</span>
      <div>
        <div style="font-family: var(--font-serif); font-weight: 400; font-size: var(--text-xl); color: var(--fg-primary); letter-spacing: var(--tracking-tight); line-height: 1.2;">Quit</div>
        <div style="font-size: var(--text-sm); color: var(--fg-tertiary); margin-top: 4px;">退出 · ご機嫌よう</div>
      </div>
    </div>
  </div>

  <!-- right column — quiet quote -->
  <div style="position: relative; display: flex; flex-direction: column; justify-content: flex-end; padding: 0 64px 64px;">
    <div style="font-family: var(--font-serif); font-size: var(--text-base); font-style: italic; color: var(--fg-tertiary); max-width: 280px; text-align: right; line-height: 1.7;">
      "Like a black thangka — gold lines of enlightened form emerging from the void."
    </div>
  </div>

  <!-- bottom hint -->
  <div style="position: absolute; bottom: 24px; left: 50%; transform: translateX(-50%); display: flex; gap: 18px; align-items: center;">
    <KeyBadge k="↑↓" size="sm" label="navigate" />
    <KeyBadge k="Enter" tone="primary" size="sm" label="select" />
    <KeyBadge k="1-5" size="sm" label="direct" />
  </div>
</div>
