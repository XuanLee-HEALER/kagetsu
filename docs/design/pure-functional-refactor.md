# Pure Functional Refactor — 实施计划

设计概念见 [`abstract-model.md`](abstract-model.md). 本文档是实施侧的范围 / 步骤 /
风险登记.

## 当前进度 (commits 在 `pure-fn-refactor` 分支)

✅ **阶段 1-7 完成**:
- 阶段 1: domain 下沉为 engine::domain 子模块
- 阶段 2: Wall pure 化 (新方法共存)
- 阶段 3: AtomicOp + OpError + typed_op! 宏
- 阶段 4: MatchState + match_apply
- 阶段 5a-d: type-state RoundState (AwaitDraw / AwaitDiscard / AwaitRiichiDiscard /
  AwaitRinshanDraw / AwaitCalls / RoundEnd) + try_op (validity gate) + typed apply
  (total + emit events) + 公开 entry (round_apply / legal_ops / summarize_round /
  init_round)
- 阶段 6: UI 层 GameEngine wrapper 接管 game.rs (commit 446a603) → 提到顶层
  crate::game_engine 给 net/dev 共用
- 阶段 7: 调用面全切到 GameEngine —
  * AI 删 *_legacy 桥接, 统一吃 &RoundState
  * net::room::RoomActor.game 字段 GameState → GameEngine, 全部走 method 调用
  * dev::recorder 重写: RecordedAction = AtomicOp, RoundRecording.initial_state
    持 GameEngine, replay 顺序 round_apply
  * GameEngine 内部 apply() helper 自动 push 真实 AtomicOp 到 recorded_actions
    (do_riichi push RiichiDeclare+Discard, do_ankan push Ankan+RinshanDraw 等)
  * **删除 src/legacy_state.rs (1012 LOC)** — crate 层面再无 GameState

测试: cargo test --lib --features dev-tools 415 passed (engine + domain + score
+ yaku + ai + dev::recorder replay roundtrip + dev::savestate + ui state machine
+ mental_poker 算法层全绿).

⏸️ **阶段 8 (后续工作)**:
- net::room::tests 现在用 #[cfg(all(test, feature = "net-tests"))] 屏蔽, 测试
  原本大量直接戳 GameState 内部字段, 重写为 round_apply 驱动留 follow-up.
- net::p2p::mp_{bridge,swarm} 两个 libp2p socket 集成测试 pre-existing flake,
  跟本次重构无关.
- 单机游戏手动验证 (`just play`).

---

## 0. 动机 + 目标

- **动机**: 技术学习实践, 不是为了解某个具体痛点.
- **目标**: 把 `engine` 模块改成 pure functional, 分清 lib (engine) + 外部 wrapper.
- **非目标**:
  - 性能不超过现状 (clone-everywhere 接受).
  - UI 不要求一并 pure (UI 状态机本身留 mutable, 它调用的 engine 是 pure).
  - 不引入 `no_std` 限制.

---

## 1. 范围

### 进 engine (refactor 范围)

- `src/engine/domain/*` — 原 `src/domain/` **整体下沉为 engine 子模块**. 走查 + 补 derive.
  - engine 顶层 re-export: `pub use domain::{Tile, TileIndex, Seat, Meld, MeldKind, Hand};`
  - 外部仍写 `tui_majo::engine::Tile`, 不暴露 `engine::domain::` 路径.
- `src/engine/state.rs` — 重写为 type-state.
- `src/engine/wall.rs` — `draw(&self) -> (Wall, Option<Tile>)`.
- `src/engine/score.rs` — 函数形式 + 已经基本 pure.
- `src/engine/event.rs` — 保留 GameEvent 类型, 但**从 RoundState 字段移除**, 由 round_apply 返回值带出.
- 新文件 `src/engine/op.rs` — AtomicOp + OpError + typed_op! 宏.
- 新文件 `src/engine/match_state.rs` — MatchState + match_apply.

### 不进 engine

- `src/ai/*` — **独立模块**. AI 通过 engine 公开 API (读 RoundState, 调 legal_ops, 输出 AtomicOp). engine 不知道 ai 存在.
- `src/ui/*` — driver, 适配新 API 但不 pure 化.
- `src/net/*` — **refactor 期间允许临时性破坏 (在线模式不可用)**. 阶段 7 仅保证签名层适配 + cargo build --bin tui-majo 通过. 运行时正确性 / net 测试不在本次 refactor 验证范围, 后续单独修.
- `src/mental_poker/*` — 已经基本 pure, 不动. 但调用它的 net::room 等可能临时坏.
- `src/dev/recorder.rs` — **暂不动**. dev-tools feature 下可能编不过, 接受. refactor 结束后再处理 RecordedAction ↔ AtomicOp 关系.

---

## 2. "Pure" 严格度

- **API 层面**: 公开函数全是 `(&state, op) -> Result<state, err>`, 不 consume self.
- **内部实现**: typed apply 内可以 `let mut s = self.common; ...; NextState{ common: s, ... }` 这种 ownership 流转, 编译器允许的范围内.
- **共享数据结构**: 直接 clone Vec / HashMap. 不引入 `im` / `rpds`.
- **副作用**: RNG 显式 seed 参数, 没有 IO / tracing / 全局变量.
- **错误**: typed enum + thiserror.

---

## 3. 性能策略

clone-everywhere. 每次 `round_apply` 内部 `state.clone()` 一次再走转移. 估算:
- RoundState ≈ 几 KB (4 玩家 × Vec<Tile> + Wall 70+14 张 + events queue).
- Turn-based 节奏 ~100ms/op, clone 几 KB << 1ms 完全可接受.

未来如果发现瓶颈 (一般不会), 可以引入 `im::Vector` 让 player.river clone 变 O(1).
本次 refactor 不优化.

---

## 4. 实施步骤 (bottom-up)

### 通用规则

- 每阶段 (含子阶段) 一个 commit, 自包含.
- 每 commit 必须 `cargo build --bin tui-majo` 通过.
- 单机相关测试 (engine / domain / ai / dev::savestate / ui 相关) 必须绿.
- net / mental_poker 测试在 refactor 期间允许失败 / 编不过 — **不阻塞**.
- `cargo build --features dev-tools` 在 refactor 中段可能编不过 (dev::recorder 没适配), 接受.
- **测试 assertion 语义不允许改** — 测试 helper 签名跟着 API 适配是允许的.

### 新旧共存策略

阶段 2-5 期间, **保留所有旧 `do_*` 方法**让外部代码继续编. 新 API (`round_apply`,
type-state state) 同时存在. 阶段 6 一次性切外部到新 API, 阶段 7 才删旧 do_*.
否则中段 commit 会大面积编不过.

### 阶段 1: domain 下沉 + 走查
- 物理移动 `src/domain/` → `src/engine/domain/`.
- `src/lib.rs`: 删 `pub mod domain;`.
- `src/engine/mod.rs`: 加 `pub mod domain;` 和 re-export `pub use domain::{Tile, TileIndex, Seat, Meld, MeldKind, Hand, ...};`
- 全工程 `use crate::domain::*` 改 `use crate::engine::*` (re-export 路径).
- 走查无 `&mut self` 方法, 必要 derive 补齐 (Debug / Clone / PartialEq / Eq / Hash / Serialize / Deserialize).

### 阶段 2: Wall pure 化
- `Wall::draw(&self) -> (Wall, Option<Tile>)` 等. 老 `&mut self` 方法暂保留 (state.rs 旧 do_* 还在用), 加新方法不动旧.
- 旧 do_* 调用站点暂不切, 等阶段 6.

### 阶段 3: AtomicOp + OpError + typed_op! 宏
新文件 `src/engine/op.rs`:
- `AtomicOp` enum (Draw / RinshanDraw / Discard / RiichiDeclare / Tsumo / Ankan / Shouminkan / Pon / Chi / Minkan / Ron / Pass).
- `AtomicOpKind` (variant kind only, 用作 OpError 字段).
- `OpError` enum (thiserror, 完整 variant 列表见 abstract-model.md §OpError).
- `typed_op!` declarative macro: 生成 typed-op enum + `try_from_atomic` + `From<TypedOp> for AtomicOp`.

### 阶段 4: MatchState
新文件 `src/engine/match_state.rs`:
- `MatchState` struct.
- `RoundOutcome` enum.
- `match_apply(&MatchState, RoundOutcome) -> MatchState`.
- `init_match(GameRules) -> MatchState`.
- `check_match_ended(&MatchState) -> bool`.

### 阶段 5: type-state RoundState (拆 4 子阶段)

最大一步, 拆细:

#### 5a — 类型骨架
- `src/engine/state.rs` 新增: `CommonRound` 子 struct + 各 typed state struct (字段, 无方法).
- `RoundState` enum 包装.
- `From<NextXxxState> for RoundState` 模板 (空实现, 占位).
- 此阶段不引入任何转移逻辑. 旧 `GameState` 仍存在并继续工作.

#### 5b — 各 typed state 的 try_op (validity gate)
- 每个 state impl `fn try_op(&self, op: AtomicOp) -> Result<TypedOp, OpError>`.
- 完整检查: phase 错配 (typed_op! 自动) + 数据级 + 规则级.
- 此阶段加单元测试覆盖每个 OpError variant 的触发路径.

#### 5c — 各 typed state 的 typed apply
- 每个 state impl `fn apply(self, op: TypedOp) -> (NextState, Vec<GameEvent>)`.
- **total 函数, 无 Result**. 输入已 validated.
- 转移逻辑直接从旧 `do_*` 抄过来重组, 算法不变.
- emit 的 events 跟旧逻辑一致 (Discard / Pon / Riichi 等).

#### 5d — 公开 entry 函数
- `round_apply(&RoundState, AtomicOp) -> Result<(RoundState, Vec<GameEvent>), OpError>`.
- `legal_ops(&RoundState) -> LegalOps`.
- `summarize_round(&RoundState) -> Option<RoundOutcome>`.
- `init_round(&MatchState, seed: u64) -> RoundState`.

阶段 5 完成后: 新旧 API 并存. 旧 `GameState` + `do_*` 仍可用 (老调用方继续编),
新 `RoundState` + `round_apply` 已就绪 (但还没人调用).

### 阶段 6: 切外部到新 API
分成两步, 都在本阶段:

#### 6a — UI 单机驱动切换 (最重要)
`src/ui/screens/game.rs`:
- `GameScreenState` 持 `RoundState` + `MatchState` 替代 `GameState`.
- `advance()` 内部循环改成 `state = round_apply(&state, op)?`.
- driver loop 读 state phase 决定下一 op 来源 (玩家 / AI / 自动 Draw / RinshanDraw).
- 单机游戏正常运行 = refactor 主要验证目标.

#### 6b — 其它调用方签名层适配
- `src/ai/dummy.rs`: 改成 `ai_choose_discard(&RoundState) -> AtomicOp` / `ai_react_to_discard(&RoundState, who) -> AtomicOp`.
- `src/net/room.rs` / `online_game.rs` / `online_zerotrust_game.rs`: 仅签名层适配让 cargo build 过, 运行时正确性不验.
- `src/dev/recorder.rs`: 暂不动, dev-tools build 接受坏.

### 阶段 7: 删遗留
- 旧 `GameState` 删.
- 旧 `do_*` 方法全删.
- engine 中的 `tracing::info!` / `warn!` 全删.
- `recorded_actions` 字段相关代码全删.
- `do_riichi` pop-and-replace hack 自然消失.

### 阶段 8: 收尾
- `cargo build --bin tui-majo` + 单机测试全绿.
- 单机游戏手动验证 (`just play`) 一切正常.
- 文档 `abstract-model.md` 跟实际代码 cross-check 一遍.
- net / dev-tools 修复留作 follow-up issue, 不阻塞本 PR.

---

## 5. 容忍 (非完美但接受)

- **RNG**: 仅用于 `Wall::shuffled(seed, with_aka)`, 一次性洗完后不再用. 直接 seed 参数喂入就够, 不上升到 `RngStream` trait 抽象. 影响仅限 wall 生成. **以后做 P2P 在线游戏可能需要重新设计这块** (mental_poker 协议涉及共同 RNG), 但不在本次范围.
- **events 完全脱离 state** ✅: round_apply 返 `Vec<GameEvent>`. UI/recorder/网络 各自累积.
- **Pass 粒度**: 单一 op (整个 call window 关闭一次). 不按家拆 4 个.
- **Crate 拆分**: 保持单 crate, lib 部分 re-export 在 `tui_majo::engine::*`. 不切 workspace.
- **在线游戏临时不可用**: refactor 期间 net / mental_poker 测试可能黑色, dev::recorder 在 dev-tools build 下可能编不过. 接受.

---

## 6. 风险登记

### 真实风险

- **net 层 follow-up**: refactor 末尾 net 层只签名层适配 + cargo build 通过, 运行时正确性留 follow-up. 包括 `room.rs::reduce_to_view` 那种 GameState → GameStateView 的转换 (现在依赖 GameState 内部字段, 重构后 RoundState 是 enum, 转换逻辑要重写). 单独一轮工作.
- **dev::recorder follow-up**: 同上. 旧 `RecordedAction` 跟新 `AtomicOp` 不一致, 需要决定是 alias / 替换 / 还是双轨并存. 旧 quick.json / recordings/ 可能失效.
- **录像 schema 演化**: AtomicOp 加 variant / RoundState 加字段会让老录像反序列化失败. 加 `schema_version` 字段, 或文档化"录像不跨 minor 版本兼容".
- **type-state state 数量增加成本**: 每加一种 phase 加一个 struct + 一组 typed-op + 一组 apply. mahjong phase 数稳定 (5-6 个), 短期不会爆.
- **mental_poker / P2P 重设计**: 本次完全不动, 但后期做 P2P 在线游戏跟 pure engine 配合时可能要重新设计 actor model + RNG 协调. 提前知道有这件事, 不在本次解决.

### 已通过设计消除的风险

- ~~Recorder pop-and-replace hack~~ → engine 不持 `recorded_actions`, 录像在外部.
- ~~Type-state vs 统一 AtomicOp 互斥~~ → 4 层架构, 数据层与行为层分离.
- ~~Type-state boilerplate 爆炸~~ → declarative macro `typed_op!{}` 消化.
- ~~Engine 内日志副作用~~ → 设计原则禁止, 测试覆盖代替.
- ~~错误回退时 state 丢失~~ → `&self` + 内部 clone, 失败不动 caller state.

---

## 7. 决策状态总览

| 议题 | 决策 |
|---|---|
| 范围 | engine 模块 + domain 走查; ai/net/ui/mental_poker 不进, 仅适配调用点 |
| Pure 严格度 | API 全 `(&state, op) -> Result`; 内部 ownership 流转随便; clone-everywhere |
| RoundState 结构 | type-state (4 层架构: AtomicOp / try_op / typed-op / typed state) |
| Wall | `&self -> (Wall, Tile)` consume-and-return 风格 |
| events 字段位置 | 倾向脱离 state (round_apply 返 Vec<Event>), 实施时定细节 |
| apply 入口 | 单一 `round_apply`, 内部 dispatch |
| AtomicOp 形态 | 单一统一 enum, 数据层. type-state 内部用 typed-op (宏生成) |
| 自动转换 | 一步一 op, driver 循环 (Draw / RinshanDraw 也是显式 op) |
| Riichi | 拆 2 op (RiichiDeclare + Discard), 中间 phase=AwaitRiichiDiscard |
| 岭上摸 | 独立 RinshanDraw op, 中间 phase=AwaitRinshanDraw |
| Pass | 单一 op (call window 整体关闭) |
| RNG | 仅 wall shuffle, seed 显式参数, 容忍, 不抽象 RngStream |
| tracing | engine 不允许 info!/warn!/error!; debug! 仅测试场景 |
| Error 性质 | 输入合法性裁定, 不是 calculation error; calculation bug 应 panic |
| Error 类型 | thiserror enum `OpError`, variants 反映规则反面陈述 |
| 错误回退 | `&self` + 内部 clone, 失败时 caller state 不动 |
| 性能 | clone-everywhere, 不优化 (turn-based < 1ms/op) |
| 测试不动 | 是 (assertion 语义不变, 仅 helper 签名适配) |
| 迁移顺序 | bottom-up (domain → wall → op → match → state → 删旧 → 适配外部) |
| Crate 拆分 | 不切, 单 crate re-export `tui_majo::engine::*` |
