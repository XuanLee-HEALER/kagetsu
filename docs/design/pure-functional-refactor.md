# Pure Functional Refactor — 设计 scratchpad

> 这是一份**开放问题清单**, 不是最终设计. 每节列出待决策点 + 当前候选 + tradeoff,
> 一节一节讨论清楚后再开始动代码. 决策定下来就把候选标 ✅ 并补一句 rationale.

## 0. 动机 + 目标

- **动机**: 技术学习实践, 不是为了解某个具体痛点.
- **目标**: 把 `engine` 层 (至少) 改成 pure functional, 让 lib 可以被任何
  wrapper (UI / net / replay / test fixture) 当作纯函数库使用.
- **非目标**:
  - 性能不要求超过现状 (clone-everywhere 可接受, 性能优化下次再说).
  - UI 不要求一并 pure (UI 状态机本身留 mutable, 只是它调用的 engine 是 pure).
  - 不引入 `no_std` 限制.

## 1. 范围 / 边界

哪些模块进 "lib"? 哪些留 "wrapper"?

候选:
- A. **最窄**: 仅 `engine::state` + 它的依赖 (`domain` 已经基本 pure). UI / net / ai / mental_poker 都是 wrapper.
- B. **中**: A + `engine::wall` + `engine::score` + `ai` (AI 决策本来就只读 state).
- C. **宽**: B + `mental_poker` (多者已经 pure 了, 顺手统一接口).
- D. **最宽**: C + `net` 层改成 pure 转换 + tokio driver.

> 决策: TODO. 当前倾向 B (engine 全套 + ai).

tradeoff:
- 范围越窄越快, 但 wrapper 内还是混着 mut 风格, 不太完整.
- 范围越宽改动越大, 风险越高, 但成果越漂亮.

## 2. "Pure" 的具体定义

到底多严? 候选维度:

- **API 层面 mut**: 
  - 所有 `do_*` 方法 `self -> Result<Self, Err>` (consume + return). 内部实现可以借助 `&mut`.
  - 或更严: 内部也不允许 `&mut`, 全部 `&self -> NewSelf`.
- **共享数据结构**:
  - 直接 clone `Vec<Tile>` 等 (简单, 每动作几十次小 alloc).
  - 引入 `im::Vector` / `rpds` persistent structure (结构共享, 内存友好, 但加依赖).
- **副作用**:
  - RNG: 把 seed 显式作为参数, 返回 (NewState, NewRngState) 这种风格.
  - 时间 / IO / 日志: 完全推到 wrapper.
  - `tracing::info!`: 视为副作用 (把它去掉) 还是允许 (作为非语义副作用)?
- **trait object / Box<dyn>**: 用不用?
- **错误**: typed enum vs `&'static str`?

> 决策: TODO.

tradeoff:
- "API 纯, 内部 mut" 够实用, 不绑死 Rust 习惯. **"内外都不 mut"** 工艺洁癖度更高
  但代码更啰嗦 (每个内部 helper 都要 thread state).
- `im` 加依赖换可读性 + 性能; clone 简单粗暴.

## 3. 数据模型

### 3.1 GameState 是否拆?

当前: 一个 `GameState` 含 phase + players + wall + 历史 + 录像缓冲...

#### 候选 A: 保持单一 struct (status quo)

```rust
struct RoundState {
    phase: Phase,                         // enum tag, 运行时 dispatch
    last_drawn: Option<Tile>,             // AwaitDiscard 时 Some
    last_discard: Option<(Seat, Tile)>,   // AwaitCalls 时 Some
    // ...
}
```

#### 候选 B: type-state 模式

把状态机的"当前所在状态"提升到**类型层面**, 每个 phase 一个 struct 携带只有那个 phase
合法的字段:

```rust
enum RoundState {
    AwaitDiscard(AwaitDiscardState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitCalls(AwaitCallsState),
    RoundEnd(RoundEndState),
    // Phase::Draw 不存在: Draw 是瞬时 op 内部完成, 不是 dwell state
}

struct AwaitDiscardState {
    common: CommonRound,         // 共享字段子 struct
    turn: Seat,
    last_drawn: Tile,            // 不是 Option, 编译期保证
}

struct AwaitCallsState {
    common: CommonRound,
    last_discard: (Seat, Tile),  // 不是 Option
}

impl AwaitDiscardState {
    fn apply(self, op: AwaitDiscardOp) -> Result<NextDiscardState, Err>;
}

enum AwaitDiscardOp {
    Discard(Tile),
    RiichiDeclare,
    Tsumo,
    Ankan(TileIndex),
    Shouminkan(TileIndex),
    // 没有 Pon/Chi/Ron — 那是 AwaitCallsOp 的事
}

enum NextDiscardState {
    AwaitCalls(AwaitCallsState),  // Discard 之后
    RoundEnd(RoundEndState),       // Tsumo 之后
    AwaitRiichiDiscard(...),       // RiichiDeclare 之后
}
```

#### 对比

| 维度 | 候选 A (单 struct) | 候选 B (type-state) |
|---|---|---|
| 非法状态 | 运行时 Err / panic | 编译期不可表示 |
| 代码量 | 少 (一个 struct) | 多 (N 个 struct + N 个 op enum) |
| 模式匹配 | 每个 op handler 内 match phase | 调用前 match RoundState 选 handler |
| serde | 一个 struct 序列化 | 多 variant 序列化 (能搞但麻烦) |
| 重构成本 | 改 enum 添 phase | 加 struct + 加 transition |
| 与"统一 AtomicOp"愿景 | 兼容 | 冲突 — op 也得按 phase 拆 |

#### 与 AtomicOp 关系: 数据层 vs 行为层 (canonical pattern)

⚠️ **更正之前的"二选一"误判**: 这不是鱼与熊掌. 通过分清"数据层"和"行为层"两者可以并存.

**思路**: AtomicOp 是 **数据** (wire format / 录像 / 网络包), type-state 是 **行为** (engine 内部不变量). 两者在不同层级, 用一个 bridge function 连接.

```rust
// L1 数据层: 单一 AtomicOp (统一算子代数, 外部全用这个)
enum AtomicOp { Draw, Discard(Tile), RiichiDeclare, Pon{..}, Pass, ... }

// L2 类型化 state
enum RoundState { AwaitDiscard(...), AwaitRiichiDiscard(...), AwaitCalls(...), RoundEnd(...) }

// L3 类型化 op (每个 state 接受的子集, engine 内部用)
enum AwaitDiscardOp { Discard(Tile), RiichiDeclare, Tsumo, Ankan(..), Shouminkan(..) }
enum AwaitCallsOp   { Pon{..}, Chi{..}, Minkan{..}, Ron{..}, Pass }

// L4 bridge: 唯一 runtime 合法性检查处
impl AwaitDiscardState {
    fn try_op(&self, op: AtomicOp) -> Result<AwaitDiscardOp, OpError> {
        match op {
            AtomicOp::Discard(t) => Ok(AwaitDiscardOp::Discard(t)),
            AtomicOp::RiichiDeclare => Ok(AwaitDiscardOp::RiichiDeclare),
            // ...
            _ => Err(OpError::IllegalForPhase),
        }
    }
    fn apply(self, op: AwaitDiscardOp) -> Result<NextDiscardState, ApplyError> {
        // 编译期已保证 op 是这个 state 能接受的, 内部代码没有 phase 检查 / Option unwrap.
    }
}

// 公开 entry: 接 AtomicOp, 内部 dispatch 到 typed
pub fn round_apply(state: RoundState, op: AtomicOp) -> Result<RoundState, OpError> {
    match state {
        RoundState::AwaitDiscard(s) => Ok(s.apply(s.try_op(op)?)?.into()),
        RoundState::AwaitCalls(s)   => Ok(s.apply(s.try_op(op)?)?.into()),
        ...
    }
}
```

**收益**:
- 外部 (录像 / 网络 / 应用层) 看到统一 AtomicOp, 单一算子代数, 序列化简单.
- Engine 内部代码类型安全, 没有 `state.last_drawn.unwrap()`.
- Runtime 合法性 check 只在 L4 的 `try_op` 集中, 主转移代码干净.

**boilerplate 用宏消化** (用户提议, 采纳):

L3 的 typed-op enum + L4 的 try_op 函数都是机械生成. 用 declarative macro:

```rust
typed_op! {
    AwaitDiscardOp from AtomicOp {
        Discard(Tile),
        RiichiDeclare,
        Tsumo,
        Ankan(TileIndex),
        Shouminkan(TileIndex),
    }
}

// 宏展开生成:
//   1. enum AwaitDiscardOp { Discard(Tile), RiichiDeclare, Tsumo, Ankan(TileIndex), Shouminkan(TileIndex) }
//   2. impl TryFrom<AtomicOp> for AwaitDiscardOp { type Error = OpError; ... }
//      (match 列出的 variant, 其它 -> Err::IllegalForPhase)
//   3. impl From<AwaitDiscardOp> for AtomicOp (双向, 录像反向用)
```

每个 state 一行 `typed_op!{}` 就配齐了. 真正手写的只有 `apply` (实际转移逻辑),
那才是业务逻辑.

这是个标准模式 (协议解析器 / CRDT / 游戏引擎都这套). 并不是设计折衷, 而是 layer separation.

#### 对比 (3 选 1)

| 维度 | A: 单 struct | B: type-state 拆 op | **C: type-state + 统一 AtomicOp + bridge** |
|---|---|---|---|
| 外部 op 一致性 | ✅ | ❌ (op 按 phase 拆) | ✅ |
| 内部类型安全 | ❌ | ✅ | ✅ |
| 录像 / 序列化 | 简单 | 多 variant 麻烦 | 简单 (用 AtomicOp) |
| Boilerplate | 最少 | 多 | 中 (多一层 bridge) |
| 学习价值 | 低 | 高 | 最高 |

→ **当前倾向 C** (4 层 canonical pattern). 决策待用户确认.

### 3.2 Wall 重新设计

当前: `Wall { live: Vec<Tile>, dead: Vec<Tile>, rinshan_used: usize, dora_revealed: usize }`,
`draw()` 会 mutate live (pop).

候选:
- **保持 Vec, draw consume self**: `fn draw(self) -> (Self, Option<Tile>)`. 简单直接.
- **抽象成 `Stream<Tile>` + 内部 cursor**: 不动牌, 只移指针. 更省内存但要重写.

### 3.3 历史 / 事件

`events: VecDeque<GameEvent>` 当前是 ring buffer (UI 用最近 32 条).
`recorded_actions: Option<Vec<RecordedAction>>` 是录像 sink.

候选:
- **从 GameState 拿出去**: `apply` 函数返回 `(new_state, Vec<event_emitted_by_this_step>)`,
  state 不带历史. wrapper 自己累积 (UI 维护 ring buffer, recorder 维护完整 log).
- **保留在 state 内**: state 仍带 events 字段, 但每次 transition 只追加这步产生的.

> 决策: TODO. 强烈倾向"events 出 state". 这是 pure FP 标准模式
> (Erlang/Elixir/EventSourcing 都这么干), 而且能消灭 `recorded_actions` 这个杂质字段.

## 4. Transition / Action 模型

### 4.1 入口数量

✅ **决策: 单一入口** `(state, op) -> Result<state, err>`. 标准 fold 形式.

不引入分解多入参 (上一选手 + 影响值 + 新选手 init), 保持简洁可读.

### 4.2 Action / Op enum

✅ **决策: 单一 enum AtomicOp**, 类似关系代数的预定义算子集. 见 abstract-model.md §"操作算子集".

⚠️ 与 §3.1 type-state 互斥, 见 §3.1 末尾"关键 tradeoff".

### 4.3 自动转换 / Draw 阶段处理

✅ **决策: 一步只走一步, driver 循环调**. Draw 也是显式 op (由 driver 见 phase=Draw 时自动喂入), 录像里能看到 Draw 这一步, 完整可重放.

## 5. 副作用 / 边缘

### 5.1 RNG

当前: `Wall::shuffled(seed: u64, ...)` 一次性洗完, 后续不再用 RNG.

如果以后引擎要在中途用 RNG (比如 AI 决策), 怎么传?
- 把 `ChaCha8Rng` 扔进 state? (state 不再 Eq).
- 每次 apply 显式传 `(state, action, &mut rng)` → wrapper 责任?
- 或抽象成 `RngStream` trait, state 持 `RngStream::Cursor`?

> 决策: TODO. 短期内不影响, 但要先想好.

### 5.2 时间

UI 顶栏 hh:mm:ss 时钟、AI_STEP_DELAY_MS 节流、思考时间倒计时—— 全是 wrapper 责任,
不进 lib. ✅

### 5.3 IO + 日志

✅ **决策**:
- 文件存档 / 网络 IO 全在 driver, 不进 engine.
- Engine **不允许** `tracing::info!` / `warn!` / `error!` (运行时副作用).
- Engine 仅允许 `tracing::debug!` 用作测试场景插桩, release build 自动抹掉.
- 正确性靠**测试覆盖**保证, 不靠运行时日志兜底.

## 6. 错误模型

### 6.1 错误的本质 (重要概念)

`OpError` **不是计算错误**, 是**输入合法性裁定**.

类比: `1 + 1` 永远是 2, 没有计算错误. mahjong `apply(state, op)` 的转移逻辑也是
total — 给一个**合法**的 (state, op), 下一 state 是确定的. 但 `(RoundState, AtomicOp)`
的笛卡尔积里有大量"语义无效"对 (AwaitCalls 时给 Discard / 切不在手里的牌 / 立直时未
听牌 ...), 这些不在合法输入域. 公开 API 的 Result 标记 "你的 op 是否在这个域内".

**engine 永远不该有"calculation error"** (那是 bug, 应该 panic). 所有 Err 都是
"输入无效"的细分.

### 6.2 OpError variant 设计

按"无效原因"组织, 每个 variant 反映 mahjong 规则的某条反面陈述:

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpError {
    // ─── 数据级 (op 引用了 state 里不存在的东西) ───
    #[error("手中无 id={0} 的牌")]
    TileNotInHand(u16),
    #[error("当前无 last_discard, 无法响应")]
    NoLastDiscard,
    #[error("当前家手中无 4 张 {0:?} 同 kind 牌, 不能暗杠")]
    InsufficientForAnkan(TileIndex),
    #[error("当前家无 {0:?} 同 kind 的副露刻子, 不能加杠")]
    NoMatchingPonForShouminkan(TileIndex),

    // ─── 规则级 (op 违反 mahjong 规则) ───
    #[error("立直方必须摸切")]
    RiichiMustTsumogiri,
    #[error("有副露不能立直")]
    NotMenzen,
    #[error("切此牌后未听牌, 不能立直")]
    NotTenpaiAfterDiscard,
    #[error("分数 < 1000, 不能立直")]
    InsufficientScore,
    #[error("牌山剩余 < 4, 不能立直")]
    InsufficientWall,
    #[error("不能碰自己的弃牌")]
    PonOwnDiscard,
    #[error("吃只能从上家")]
    ChiNotFromUpper,
    #[error("明杠的三张需与弃牌同 kind")]
    MinkanKindMismatch,
    #[error("自摸 / 荣和 但牌型不和")]
    NotWinning,
    #[error("和了但无役")]
    NoYaku,

    // ─── Phase 错配 (type-state 路径下大部分编译期消, 落地这里只剩兜底) ───
    #[error("op 在当前 phase 不合法")]
    IllegalForPhase {
        op_kind: AtomicOpKind,
        phase_kind: PhaseKind,
    },
}
```

driver 负责展示文案 (用户语言 / i18n / TUI 错误条). engine 只提供结构化分类.

注意: **没有 `Internal(&'static str)` variant**. 如果 engine 走到了"理论不可达",
那是 engine bug, 应该 panic, 不该作为 OpError 漏给 caller. Caller 看到 OpError 应该
能保证是输入问题, 不是 engine 问题.

### 6.3 Result vs total apply (设计选择)

理论上 3 种放置 validity check 的方式:

- A. `apply(s, op) -> Result<s, OpError>` (单一入口, Result 在 apply)
- B. `validate(&s, &op) -> Result<(), OpError>; apply(s, op) -> s` (validate 单独, apply total + debug_assert)
- C. `try_new(&s, op) -> Result<ValidOp<S>, OpError>; apply(s, ValidOp<S>) -> s` (smart constructor 产生 token, apply total)

**总 runtime check 次数都一样** — 区别只是 Err 的位置. ABC 各有理论纯度, 但实际:

- A 最 idiomatic (Rust 标准 Result), 写起来最直接
- B 把 validate 露出来给 caller "想先看看是否合法可以单独问", 但 apply 内还得
  debug_assert, 多一层
- C 最纯 (apply 真 total), 但 ValidOp token 在无 dependent types 的 Rust 里
  绑死特定 state 实例需要 phantom type + 工程量大, 收益边际

✅ **决策: A**. apply 公开 API 返 Result, 内部 (type-state typed-op 已 validated 后)
转移逻辑 total — 真正的 `1+1` 在 type-state 内层.

### 6.4 错误回退语义 (state ownership)

apply 失败时 caller 怎么拿回原 state?

- A. `apply(self, op) -> Result<Self, (Self, OpError)>` — Err 带回原 state. 严谨但啰嗦.
- B. caller 在 apply 前 clone, Err 时丢弃新副本. 最简单.
- C. `apply(&self, op) -> Result<Self, OpError>` — engine 内部 clone, 失败不影响原 state. 折中.

✅ **决策: C**. clone 已被 §7 接受 (clone-everywhere). engine 内部 clone, 失败时
caller state 不动, 心智最干净, 调用站点也无需 clone 包装.

```rust
pub fn round_apply(state: &RoundState, op: AtomicOp) -> Result<RoundState, OpError>;
```

## 7. 性能预算

当前 GameState 大小估算:
- 4 玩家 × `Vec<Tile>` (闭手 13-14 张, river 24 张, melds 几个) ≈ 几 KB
- Wall: 70 张 live + 14 张 dead × Tile ≈ 1 KB
- events: VecDeque<GameEvent> 32 条 ≈ 1 KB

每动作 clone 整个 GameState ≈ 几 KB allocate, 在 100ms 节流的 turn-based 游戏里
完全可接受 (< 1 ms overhead).

候选优化路径 (用上时再做):
- 持久化数据结构 (`im::Vector<Tile>`) 让 player.river clone 变 O(1)
- COW: `Cow<'_, GameState>`, 只在 mut 时 clone

> 决策: 不优化. 默认 clone. 文档化.

## 8. 测试 + 迁移路径

### 8.1 测试不动

408 lib 测试 + 54 集成测试都得继续过. **测试是 ground truth**, 不许修测试以适配 API
变化 — 如果测试要改, 说明引擎语义变了, 要单独 audit.

例外: 测试里直接构造 GameState 的 helper 必须改 API 适配.

### 8.2 迁移顺序 (bottom-up)

1. `domain` 层 — 几乎已经 pure, 复查一遍, 确认没有 mut self 方法.
2. `engine::wall` — Wall 改 consume-and-return.
3. `engine::state` — GameState 内部 do_* 改, 加 `apply` 入口.
4. `dev/recorder.rs` — 把 replay 重写成 fold.
5. `ai` — 适配新 API (大概率只需改函数签名, 决策逻辑不动).
6. `ui::screens::game` 等 wrapper — 适配新调用.
7. `net::room` 等 — 适配 (这层最痛).

每步都要全测试通过才能动下一步.

### 8.3 中间状态

big-bang 一次改完没法 review. 拆 PR 但都在 `pure-fn-refactor` 分支:
- commit-1: domain 复查 + Wall consume.
- commit-2: GameState do_* consume.
- commit-3: 引入 apply + 删 recorded_actions hack.
- commit-4: type-state 拆分 (如果决定做).
- commit-5+: wrapper 适配.

每 commit 自包含, 全测试绿.

## 9. 命名 / 结构

主 crate 切不切?
- A. 保持单 crate, lib 部分 re-export 在 `tui_majo::engine::*`.
- B. workspace + 子 crate `tui-majo-engine`, 可独立发版到 crates.io.
- C. 不切, 但 `[lib]` 标记一些 pub 的 entry point 当作"lib API contract".

> 决策: TODO. 倾向 A. B 的好处是清楚, 但 workspace 改 Cargo.toml + CI 一连串牵动.

## 10. 风险登记

- **net 层难映射 pure**: actor 是 long-running, msg-driven. 改成纯 `(state, msg) -> (state, [out])`
  + tokio 外层 driver 可行, 但 mental_poker actor 已经是这风格, p2p swarm 不是. (但这层不在
  本次 refactor 强制范围内, 见 §1 倾向 B.)
- **type-state 状态数量增加**: 每加一种 phase 就要新加 struct. mahjong phase 数量
  稳定 (Draw 瞬时省掉, 实际 4-5 个), 不太会爆.
- **AtomicOp + state 演化兼容性**: 录像文件依赖 AtomicOp / RoundState 序列化格式.
  以后加 op variant / 改 state 字段, 老录像可能反序列化失败. 加 schema version
  字段, 或接受老录像 invalidation.
- **学习收益 vs 时间成本不对等**: 设计都讨论清楚后, 实施部分只是机械 typing,
  风险点都集中在前面这些决策上.

**已通过设计消除的风险**:
- ~~Recorder pop-and-replace hack~~ → engine 不再持 `recorded_actions` 字段, 录像在外部.
- ~~Type-state vs 统一 AtomicOp 互斥~~ → 4 层架构, 数据层与行为层分离.
- ~~Type-state boilerplate 爆炸~~ → declarative macro `typed_op!{}` 消化.

## 决策状态总览

| 节 | 决策 | 状态 |
|---|---|---|
| 1 | 范围 | ⬜ (倾向 B: engine 全套 + ai) |
| 2 | "Pure" 严格度 | ⬜ |
| 3.1 | GameState 拆不拆 | ⬜ (倾向 C: type-state + 统一 AtomicOp + bridge) |
| 3.2 | Wall consume 模式 | ⬜ |
| 3.3 | events 出 state | ⬜ (倾向 出 state) |
| 4.1 | apply 入口 | ✅ 单一 `(state, op) -> Result` |
| 4.2 | AtomicOp enum | ✅ 单一统一算子集 (数据层) |
| 4.3 | 自动转换 | ✅ 一步一 op, driver 循环 |
| 5.1 | RNG 模型 | ⬜ (短期 wall 一次洗完, 不影响) |
| 5.3 | tracing | ✅ engine 不带 info!, debug! 仅测试用 |
| 6 | Error 性质 | ✅ "输入合法性裁定", 不是 calculation error |
| 6 | Error 类型 | ✅ thiserror enum `OpError` |
| 6 | apply 签名 | ✅ A. 公开返 Result, type-state 内部 total |
| 6 | 错误回退语义 | ✅ C. `&self` 内部 clone |
| 7 | 性能策略 | ✅ clone-everywhere |
| 8.1 | 测试不动 | ✅ |
| 8.2 | bottom-up 迁移 | ✅ |
| 9 | crate 拆分 | ⬜ (倾向 A: 单 crate) |
| 9 | Riichi 是否拆 op | ✅ 拆 2 op (RiichiDeclare + Discard) |
| 9 | 岭上摸是否独立 op | ✅ 独立 (RinshanDraw) |
| 9 | Pass 粒度 | ⬜ (倾向 单一 op) |

---

## 已确立的核心模型

抽象概念 + 三层 fold 结构见 [`abstract-model.md`](abstract-model.md).

要点 (用户已确认):
- 3 层: 庄 (Match) / 局 (Round) / 操作 (AtomicOp).
- "操作" = 原子单位, 摸是一个 op, 切是另一个. 没有"轮次/巡"作为独立层.
- 鸣牌问题: 在原子模型下退化, 每个鸣牌就是一个 op, 不再"跨边界".

---

**讨论顺序**: §1 (范围) + §2 (严格度) → §3-§4 (数据 + 转换模型) → §5+.
当前 abstract-model.md 已锁住宏观结构, 剩下决策对应 scratchpad 各小节.
