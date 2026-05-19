# Svelte 翻译执行计划

把 `crates/web-majo/static/*.jsx` 的 10 个 React 设计原型 1:1 翻译到
`crates/web-majo/frontend/src/lib/screens/*.svelte`,Svelte 5。

**分支**: `workspace-refactor`
**目标**: 视觉 pixel-perfect 与原型一致;`npm run build` 通过;`cargo run -p web-majo` 后浏览器看效果与原型 React 版一致。

---

## 0. 已就绪 (不要重复)

工程骨架 (commit `4841441`) 已完成:
- vite + svelte 5 工程在 `crates/web-majo/frontend/`
- `npm install` 已跑过 (node_modules/ gitignored 但 package-lock 在)
- core primitives 在 `frontend/src/lib/core/`:
  - `pigments.js` — `PIG.gser` / `PIG.ngonpo` 等矿物色常量 (object export)
  - `tile-utils.js` — `parseTile` / `tileFace` / `tileTextColor` / `tileSvgPath` / `TILE_SIZES` / `NUMERALS` / `HONORS`
  - `game-fixture.js` — `GAME` 样本对象
  - `Tile.svelte` — props: `t, size, state, rotate, style`
  - `BackInner.svelte` — 无 props
  - `TileRow.svelte` — props: `tiles, size, gap, selected, drawIdx, riichiAt, style, getState`
  - `DiscardPile.svelte` — props: `tiles, riichiAt, rotation, style` (注: prototype 内 game-screen 用的 `DiscardPileFlat` 是它的无内旋转版本,见下面 § 3)
  - `KeyBadge.svelte` — props: `k, label, tone, disabled, size`
  - `Eyebrow.svelte` — slot, prop: `style`
  - `Card.svelte` — slot, props: `style, raised, accent, padding`
  - `Trilingual.svelte` — props: `en, zh, ja, style`
  - `Hr.svelte` — prop: `style`
  - `Row.svelte` — props: `label, value, mono` (value 可以是字符串,也可省略改用 `<slot />`)
  - `DorjeMark.svelte` — prop: `size`
  - `Button.svelte` — props: `label, primary, kb`
  - `ThreeStripes.svelte` — 无 props
  - `DiscoveryPill.svelte` — 无 props (硬编码 "mDNS active · 3 peers")
- 入口 + 路由就绪:
  - `src/main.js`, `src/App.svelte` (hash router 派发)
  - `src/lib/router.js` (writable store)
  - `src/lib/screens/Home.svelte`, `src/lib/screens/Frame.svelte`
  - `src/lib/screens/MenuScreen.svelte` (**已完成,作为参考模板**)

---

## 1. 你 (agent) 要翻译的 9 个 screen

| # | 源 jsx (路径都在 `crates/web-majo/static/`) | 目标 svelte 文件 (路径都在 `crates/web-majo/frontend/src/lib/screens/`) | route id | 备注 |
|---|---|---|---|---|
| 01 | `game-screen.jsx` | `GameScreen.svelte` | `g-normal` | mode="NORMAL", showActionModal=false |
| 02 | 同上 | (复用 `GameScreen.svelte`,接受 mode prop) | `g-command` | mode="COMMAND", commandText="discard p4" |
| 03 | 同上 | (复用 `GameScreen.svelte`,showActionModal=true) | `g-modal` | mode="NORMAL", showActionModal=true |
| 04 | `zerotrust.jsx` | `ZeroTrustGameScreen.svelte` | `zt-game` | |
| 05 | `pregame.jsx` (MenuScreen 函数) | `MenuScreen.svelte` | `menu` | ✅ 已完成 |
| 06 | `pregame.jsx` (ConfigScreen 函数) | `ConfigScreen.svelte` | `config` | |
| 07 | `multiplayer.jsx` (LobbyScreen 函数) | `LobbyScreen.svelte` | `lobby` | |
| 08 | `multiplayer.jsx` (RoomScreen 函数) | `RoomScreen.svelte` | `room` | |
| 09 | `results.jsx` (HandResultScreen 函数) | `HandResultScreen.svelte` | `hand-result` | |
| 10 | `results.jsx` (MatchEndScreen 函数) | `MatchEndScreen.svelte` | `match-end` | |
| - | `action-modal.jsx` | `ActionModal.svelte` | (无独立 route) | 被 GameScreen 在 showActionModal=true 时 import |

合计 9 个新 svelte 文件 + ActionModal。

## 2. 翻译规则 (JSX → Svelte 5)

### 总原则
- **视觉 pixel-perfect**:每个 px / color / border-radius / font-size / gap / padding / margin 等数值与原型完全一致
- **结构允许差异**:HTML/CSS 等价即可,不必和 React 一一对应
- **可读性优先**:不要为了"原汁原味"保留无意义的 React-isms

### Syntax 映射

| React JSX | Svelte 5 |
|---|---|
| `function Foo({ a, b = 1 })` | `<script>\n  export let a;\n  export let b = 1;\n</script>` |
| `<div style={{ color: 'red', fontSize: 14 }}>` | `<div style="color: red; font-size: 14px;">` (kebab-case, 数字加 px) |
| `<div style={{ background: bg }}>` | `<div style="background: {bg};">` 或 `<div style:background={bg}>` |
| `{cond ? a : b}` | `{cond ? a : b}` 或 `{#if cond}{a}{:else}{b}{/if}` |
| `{cond ? <X /> : null}` | `{#if cond}<X />{/if}` |
| `{arr.map((x, i) => <Item />)}` | `{#each arr as x, i}<Item />{/each}` |
| `{children}` | `<slot />` |
| `<>...</>` (Fragment) | 直接展开,无需 wrapper |
| `onClick={fn}` | `on:click={fn}` (svelte 5 也支持 `onclick={fn}`) |
| 组件内部小 helper 函数 | svelte file 一开始 `<script>` 里 `function helper()` |
| `useState`, `useEffect` | 这次基本用不到 (原型多是 stateless) |

### inline-style 转换技巧

React 用 object: `style={{ marginTop: 12, color: 'red' }}`
Svelte 用 string: `style="margin-top: 12px; color: red;"`

**复杂值用模板字符串**:
```svelte
<script>
  export let active;
  $: bg = active ? 'var(--accent-soft)' : 'transparent';
</script>
<div style="background: {bg}; padding: 10px;">
```

**或者用** `style:` directive (更干净):
```svelte
<div style:background={bg} style:padding="10px">
```

**多 inline 拼接**: 写一个 reactive variable 拼好再用:
```svelte
<script>
  $: outerStyle = `
    width: 1440px;
    height: 900px;
    background: var(--bg-base);
    ...
  `;
</script>
<div style={outerStyle}>
```

### 内部 sub-component 处理

每个源 jsx 都有 internal helper component (如 `Stat`, `Divider`, `Wall`,
`SeatCard`, `Radio`, `Toggle`, `ProtoStep`...)。两种处理方式:

**方式 A: 同文件内** (svelte 5 不支持 multiple top-level components per file,所以**这条不行**)

**方式 B: 拆成独立小文件 (推荐)**

例如 game-screen 的 `Wall` / `WallSlot` / `SeatLabelInline` / `CenterCluster` /
`Stat` / `Divider` 等,放到 `frontend/src/lib/screens/game/` 子目录:

```
src/lib/screens/
├── GameScreen.svelte         (主文件)
├── game/
│   ├── Stat.svelte
│   ├── Divider.svelte
│   ├── Table.svelte
│   ├── CenterCluster.svelte
│   ├── DiscardPileFlat.svelte
│   ├── Wall.svelte
│   ├── WallSlot.svelte
│   ├── SeatLabelInline.svelte
│   ├── MeldGroup.svelte
│   ├── HandStrip.svelte
│   ├── SidePanel.svelte
│   ├── SidePlayer.svelte
│   ├── Mini.svelte
│   └── (etc)
├── ZeroTrustGameScreen.svelte
├── zerotrust/
│   ├── ZTBadge.svelte
│   ├── ProtocolPill.svelte
│   ├── ZTHandStrip.svelte
│   ├── ZTSidePanel.svelte
│   ├── PeerRow.svelte
│   ├── ProtoStep.svelte
│   └── Evt.svelte
├── ConfigScreen.svelte
├── pregame/
│   ├── NavItem.svelte
│   ├── ConfigGroup.svelte
│   ├── Radio.svelte
│   └── Toggle.svelte
├── LobbyScreen.svelte
├── RoomScreen.svelte
├── multiplayer/
│   ├── RoomRow.svelte
│   ├── PlayerBadge.svelte
│   └── SeatCard.svelte
├── HandResultScreen.svelte
├── MatchEndScreen.svelte
├── results/
│   ├── YakuRow.svelte
│   ├── DeltaRow.svelte
│   ├── StandingRow.svelte
│   └── BigStat.svelte
├── ActionModal.svelte
└── (existing: MenuScreen, Home, Frame)
```

`MenuItem` 已经在 MenuScreen.svelte 里 inline 写了 (因为只用一次,不值得独立文件)。

如果某个 sub-component 只用 1-2 次且很小 (< 30 行),你可以选择在父 svelte 文件里 inline 写 (写一个 helper render snippet 或者直接在 mustache 表达式里展开)。**但不要在 svelte 文件里 export 多个组件 — svelte 5 不支持**。

## 3. 重要细节 / 陷阱

### a. game-screen 的 DiscardPileFlat
prototype 用了一个 6-col × 3-row no-rotation 版本叫 `DiscardPileFlat` (不同于 core 的 `DiscardPile`,后者整个旋转)。需要在 `screens/game/DiscardPileFlat.svelte` 自己写,代码见原 jsx L266-281。

### b. SVG 大段 inline
- `DorjeMark` 已在 core. `BackInner` 已在 core.
- `ZTBadge` 的 shield/lock 小 svg、grip icon 等,直接 inline 在对应 svelte 里。
- React 写 `strokeWidth` → SVG 写 `stroke-width`. 同理 `viewBox`, `strokeLinecap`, `strokeLinejoin`, `clipPath`, `xmlns:xlink` 等都用 kebab-case (但 viewBox 是个例外, viewBox 保持驼峰)。

### c. 在 inline event 里别用字符串
svelte 5 不再支持 `onclick="this.style.bg=..."` 这种 string handler。要用 `on:click={fn}` 或 `onclick={fn}` 表达式。hover 效果改用 scoped `<style>` 的 `:hover`。

### d. `class="dc-..."` 这种 prototype 的 design-canvas 工具相关 class **不要**翻译过来 (设计画布是 dev 工具,产品不需要)。

### e. `data-screen-label="..."` 属性可以保留 (作 metadata,不影响渲染)。

### f. `ResizeObserver` / 复杂 useEffect — design-canvas 才有,屏幕组件几乎没有。如果某个屏幕真的用了 React state hook, 翻译成 svelte 的 reactive declaration 即可。

### g. `tileSvgPath` 返回**绝对路径** `/tiles/Xxx.svg` (svelte 版),不是原型的相对路径 `tiles/Xxx.svg`。tile-utils.js 已经改过。

### h. 数字 props
React `Tile size="lg"` → Svelte `<Tile size="lg" />` 一致。
React `rotate={90}` → Svelte `<Tile rotate={90} />` 一致。
React `size={20}` (DorjeMark) → Svelte `<DorjeMark size={20} />` 一致。

### i. 注释保留
原型每个组件都有顶部 `// ...` 注释 (说明用途)。翻译过去时,这些注释**保留**,作为 svelte `<!-- ... -->` 或 `<script>` 内的 `//`。1:1 复刻 intent。

### j. inline `style={{ ...prop_style }}` (rest spread)
React 用 `...style` spread 用户传入的 style。Svelte 翻译: 把 `style` 作为字符串 prop,拼到 outer style 末尾。已经在 core 的 Tile / TileRow 等里这么做了。

## 4. 完成后 (你不要做的部分,我会做)

写完 9 个 screen 后:
1. 在 `src/App.svelte` 的 `ROUTES` 对象里挂上新组件 (我会做,你不需要改 App.svelte)
2. `npm run build` (我会跑,翻译时**你不要 build**)
3. cargo restart + chrome 验证
4. 删 static/*.jsx 设计稿 (vite emptyOutDir 自动清掉)

## 5. 提交规范

每个 agent 完成自己负责的 screen 后:
- 用 `git add` 添加新文件 (按 path 添加,不要 `-A`)
- commit message: `feat(web-majo/frontend): 翻译 <ScreenName> + 内部 sub-components 到 svelte`
- 一个 agent 一个 commit (或一组紧密相关的 screen 一个 commit)

如果某个 svelte 有 syntax 错误你不确定,**先写完不要 build**,完成后给 team-lead (我) 发消息描述疑虑,我会一起 review + fix。

## 6. 验收标准

- 9 个新 svelte 文件按上面表格落地
- 每个文件按本文档的翻译规则 (inline style 转 string, 内部小组件拆文件)
- 视觉跟原型 jsx 1:1 一致 (每个 px / color / radius 数值)
- 不引入新 npm dependency (只用 svelte + 现有 core primitives)
- 不动 src/App.svelte / src/main.js / 任何 core primitives / 原型 jsx
