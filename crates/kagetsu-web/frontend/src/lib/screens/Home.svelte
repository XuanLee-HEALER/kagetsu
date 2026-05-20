<!--
  Index page listing all 10 artboards. Not part of the design prototype —
  it replaces the React DesignCanvas (zoom/drag tooling not relevant to
  the product).
-->
<script>
  import DorjeMark from '../core/DorjeMark.svelte';
  import Eyebrow from '../core/Eyebrow.svelte';

  const SECTIONS = [
    {
      title: '主对局 · Single player + Standard P2P',
      items: [
        { id: 'g-normal', label: '01 ・ Normal' },
        { id: 'g-command', label: '02 ・ Command 命令输入' },
        { id: 'g-modal', label: '03 ・ Action modal 唤起' },
      ],
    },
    {
      title: 'ZeroTrust · 零信任 mental poker',
      items: [{ id: 'zt-game', label: '04 ・ ZeroTrust 对局' }],
    },
    {
      title: '入口 · 主菜单 + 配置',
      items: [
        { id: 'menu', label: '05 ・ Main menu' },
        { id: 'config', label: '06 ・ Pre-game config' },
      ],
    },
    {
      title: '多人 · 大厅 + 房间',
      items: [
        { id: 'lobby', label: '07 ・ LAN lobby' },
        { id: 'room', label: '08 ・ Room waiting' },
      ],
    },
    {
      title: '结算 · 和了 + 终局',
      items: [
        { id: 'hand-result', label: '09 ・ Hand result' },
        { id: 'match-end', label: '10 ・ Match end' },
      ],
    },
  ];
</script>

<div class="page">
  <header style="max-width: 1000px; margin-bottom: 56px;">
    <div style="display: flex; align-items: center; gap: 12px;">
      <DorjeMark size={24} />
      <Eyebrow>kagetsu · Web · v2.1.0</Eyebrow>
    </div>
    <h1 style="font: var(--t-h2); font-family: var(--font-serif); font-weight: 300; letter-spacing: var(--tracking-tight); margin-top: 14px;">
      主对局 · 配置 · 大厅 · 结算
    </h1>
    <p style="font: var(--t-lede); color: var(--fg-secondary); margin-top: 12px; font-style: italic;">
      The full web interface for kagetsu — Standard P2P, ZeroTrust mental poker,
      and the surfaces that surround a single hand of riichi.
    </p>
    <div class="meta">
      <span>SakyaHuman Design System</span>
      <span>Dorje · single dark theme</span>
      <span>IBM Plex Sans · Serif · Mono</span>
      <span>Svelte 5 · 1:1 translation</span>
    </div>
  </header>

  <main>
    {#each SECTIONS as section}
      <section>
        <h2>{section.title}</h2>
        <div class="cards">
          {#each section.items as item}
            <a class="card" href={'#' + item.id}>
              <div class="id">#{item.id}</div>
              <div class="label">{item.label}</div>
            </a>
          {/each}
        </div>
      </section>
    {/each}
  </main>
</div>

<style>
  .page {
    min-height: 100vh;
    background: var(--bg-deepest);
    color: var(--fg-primary);
    font-family: var(--font-sans);
    padding: 56px 64px;
  }
  .meta {
    margin-top: 20px;
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--fg-tertiary);
  }
  .meta span {
    padding: 3px 10px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
  }
  main {
    display: flex;
    flex-direction: column;
    gap: 56px;
    max-width: 1200px;
  }
  h2 {
    font-size: 28px;
    font-weight: 600;
    color: var(--fg-primary);
    letter-spacing: -0.4px;
    margin: 0 0 24px;
  }
  .cards {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
  }
  .card {
    display: block;
    padding: 18px 22px;
    background: var(--bg-surface0);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--fg-primary);
    text-decoration: none;
    min-width: 240px;
    transition: border-color 200ms, background 200ms;
  }
  .card:hover {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .card .id {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--fg-tertiary);
    letter-spacing: var(--tracking-wide);
    text-transform: uppercase;
  }
  .card .label {
    font-family: var(--font-serif);
    font-size: 17px;
    margin-top: 6px;
    letter-spacing: -0.2px;
  }
</style>
