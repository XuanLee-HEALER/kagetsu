<script>
  import { PIG } from '../../core/pigments.js';
  import Eyebrow from '../../core/Eyebrow.svelte';
  import Hr from '../../core/Hr.svelte';
  import Row from '../../core/Row.svelte';
  import Tile from '../../core/Tile.svelte';
  import SidePlayer from './SidePlayer.svelte';

  export let game;
  export let me;
</script>

<!-- 得点 -->
<section>
  <Eyebrow style="margin-bottom: 10px;">Scores · 得点</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 6px;">
    {#each game.players as p, i}
      <SidePlayer {p} you={i === 0} />
    {/each}
  </div>
</section>

<Hr />

<!-- tenpai / hand analysis -->
<section>
  <Eyebrow style="margin-bottom: 10px;">Hand · 自家</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 10px;">
    <Row label="Shanten · 向聴">
      <span style="color: {PIG.ljangkhu}; font-weight: 600;">0 · 聴牌</span>
    </Row>
    <Row label="Tenpai · 待ち">
      <span style="display: inline-flex; gap: 3px;">
        {#each me.tenpai as t}
          <Tile {t} size="xs" state="tenpai" />
        {/each}
      </span>
    </Row>
    <Row label="Tiles · 残" value="6 枚" mono />
    <Row label="Yaku · 役">
      <span style="color: {PIG.ngonpo};">立直 · 平和 · ドラ</span>
    </Row>
    <Row label="Score · 打点">
      <span style="font-family: var(--font-mono);">
        5,200 <span style="color: var(--fg-tertiary);">/</span> 7,700
      </span>
    </Row>
  </div>
</section>

<Hr />

<!-- danger -->
<section>
  <Eyebrow style="margin-bottom: 10px; color: {PIG.mtshal};">Danger · 危険</Eyebrow>
  <div style="display: flex; gap: 4px;">
    {#each game.danger as t}
      <Tile {t} size="xs" state="danger" />
    {/each}
  </div>
  <div style="margin-top: 8px; font-size: 12px; color: var(--fg-tertiary);">
    对家立直巡 · 至 8 巡前过的牌
  </div>
</section>

<Hr />

<!-- log -->
<section style="flex: 1;">
  <Eyebrow style="margin-bottom: 10px;">Log · 対局</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 6px; font-size: 12px;">
    {#each game.log as e}
      <div style="display: flex; align-items: center; gap: 8px; color: {e.emphasize ? 'var(--fg-primary)' : 'var(--fg-tertiary)'};">
        <span style="font-family: var(--font-mono); color: var(--fg-disabled);">
          {String(e.junme).padStart(2, '0')}
        </span>
        <span style="color: {e.emphasize ? PIG.gser : 'var(--fg-secondary)'};">{e.who}</span>
        <span>{e.action}</span>
        <Tile t={e.tile} size="xs" />
      </div>
    {/each}
  </div>
</section>
