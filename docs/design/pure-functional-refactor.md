# Pure Functional Refactor — 实施计划

设计概念见 [`abstract-model.md`](abstract-model.md). 本文档是实施侧的范围 / 步骤 /
风险登记.

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

- `src/domain/*` — 几乎已经 pure, 走查 + 补 derive.
- `src/engine/state.rs` — 重写为 type-state.
- `src/engine/wall.rs` — `draw(&self) -> (Wall, Option<Tile>)`.
- `src/engine/score.rs` — 函数形式 + 已经基本 pure.
- `src/engine/event.rs` — 保留 GameEvent, 但从 GameState 移到 round_apply 返回值.
- 新文件 `src/engine/op.rs` — AtomicOp + OpError + typed_op! 宏.
- 新文件 `src/engine/match_state.rs` — MatchState + match_apply.

### 不进 engine

- `src/ai/*` — **独立模块**. AI 通过 engine 公开 API (读 RoundState, 调 legal_ops, 输出 AtomicOp). 不在 engine 内, engine 也不知道 ai 存在.
- `src/net/*` — 不在本次 refactor 强制范围. 调用 engine 的代码点跟着新签名改, 但 actor 模型保持现状.
- `src/ui/*` — 不变, 继续是 mutable driver.
- `src/dev/recorder.rs` — replay 函数从循环改成 `try_fold(round_apply)`. 录像写盘逻辑不变.
- `src/mental_poker/*` — 已经基本 pure, 不动.

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

每阶段一个 commit, 每 commit 全测试 (408 lib + 54 集成) 必须绿. **测试本身不允许改**
(测试是 ground truth — 测试需要改的 helper 适配 API 变化是允许的, 但 assertion 语义不改).

### 阶段 1: domain 层走查
- `Tile` / `TileIndex` / `Seat` / `Meld` / `Hand`: 确认无 `&mut self` 方法.
- 必要的 derive 补齐 (Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize).

### 阶段 2: Wall pure 化
- `Wall::draw(&self) -> (Wall, Option<Tile>)`.
- `Wall::rinshan_draw(&self) -> (Wall, Option<Tile>)`.
- `Wall::reveal_next_dora(&self) -> Wall`.
- 调用方 (state.rs 旧 do_*) 暂时通过本地 `let (w, t) = self.wall.draw(); self.wall = w;` 适配, 为后面 type-state 重写铺路.

### 阶段 3: AtomicOp + OpError + typed_op! 宏
- 新文件 `src/engine/op.rs`.
- `AtomicOp` enum (含 Draw / RinshanDraw / Discard / RiichiDeclare / Tsumo / Ankan / Shouminkan / Pon / Chi / Minkan / Ron / Pass).
- `AtomicOpKind` (variant kind only, 用作 OpError 字段).
- `OpError` enum (thiserror).
- `typed_op!` declarative macro 生成 typed-op enum + try_from_atomic + From 反向.

### 阶段 4: MatchState
- 新文件 `src/engine/match_state.rs`.
- `MatchState` struct.
- `RoundOutcome` enum.
- `match_apply(&MatchState, RoundOutcome) -> MatchState`.
- `init_match(GameRules) -> MatchState`.
- `check_match_ended(&MatchState) -> bool`.

### 阶段 5: type-state RoundState 重写
最大一步. `src/engine/state.rs` 新结构:
- `CommonRound` 子 struct (共享字段).
- 每个 phase 的 typed state struct.
- 每个 typed state 的 `try_op` (完整 validity gate).
- 每个 typed state 的 typed `apply` (total).
- `NextXxxState` enum + `From<NextXxxState> for RoundState`.
- 公开 `RoundState` enum 包装.
- 公开 `round_apply(&RoundState, AtomicOp) -> Result<RoundState, OpError>`.
- 公开 `legal_ops(&RoundState) -> LegalOps`.
- 公开 `summarize_round(&RoundState) -> Option<RoundOutcome>`.
- 公开 `init_round(&MatchState, seed: u64) -> RoundState`.
- events 从 state 字段拆出: `round_apply` 返回 `Result<(RoundState, Vec<GameEvent>), OpError>`. (TBD: 也可能保留 events 在 state 里方便 driver 读, 实施时定.)

### 阶段 6: 删遗留
- engine 中的 `tracing::info!` / `warn!` 全删.
- `recorded_actions: Option<Vec<RecordedAction>>` 字段删 (engine 不知道录像).
- `do_riichi` pop-and-replace hack 删 (新结构里 RiichiDeclare 是独立 op).
- 旧 `do_*` 方法删.

### 阶段 7: 适配外部
- `src/dev/recorder.rs`:
  - `RecordedAction` 等同 `engine::AtomicOp` (要么改用 engine::AtomicOp, 要么 type alias).
  - `replay` 函数重写成 `ops.iter().try_fold(initial, |s, op| engine::round_apply(&s, op))`.
- `src/ai/dummy.rs`:
  - `ai_choose_discard(&RoundState) -> AtomicOp`.
  - `ai_react_to_discard(&RoundState, who: Seat) -> AtomicOp`.
- `src/ui/screens/game.rs`:
  - 持 `RoundState` (替代 GameState).
  - 用 `round_apply(&self.state, op)?` 替代 `self.game.do_*`.
  - 内部 driver loop: 读 state phase, 决定下一 op 来源 (玩家 / AI / 自动 Draw).
- `src/net/room.rs` / `online_game.rs`:
  - 跟着新签名改. actor 内部仍 `&mut server_state`, 但 `server_state.engine_state = round_apply(&server_state.engine_state, op)?` 走 pure 函数.

### 阶段 8: 测试 + benchmark
- 全 408 lib + 54 集成测试跑过.
- (可选) 加几个 benchmark 看 clone 开销 (一般不超过 1ms / op).

---

## 5. 容忍 (非完美但接受)

- **RNG**: 仅用于 `Wall::shuffled(seed, with_aka)`, 一次性洗完后不再用. 直接 seed 参数喂入就够, 不上升到 `RngStream` trait 抽象. 影响仅限 wall 生成.
- **events 是否完全脱离 state**: 阶段 5 实施时拍. 倾向脱离 (每次 round_apply 返新 events Vec), 但如果 driver 实际方便操作历史, 留在 state 也行. 对核心模型不影响.
- **Pass 粒度**: 单一 op (整个 call window 关闭一次). 不按家拆 4 个.
- **Crate 拆分**: 保持单 crate, lib 部分 re-export 在 `tui_majo::engine::*`. 不切 workspace.

---

## 6. 风险登记

### 真实风险

- **net 层调用点适配**: `room.rs` / `online_game.rs` / `online_zerotrust_game.rs` 都调用 engine. 签名换了之后这些调用点都得动. mental_poker 的 actor 已经接近 pure, mp_swarm 没那么干净, 但都不深.
- **录像 schema 演化**: AtomicOp 加 variant / RoundState 加字段会让老录像反序列化失败. 加 `schema_version` 字段, 或文档化"录像不跨 minor 版本兼容".
- **type-state state 数量增加成本**: 每加一种 phase 加一个 struct + 一组 typed-op + 一组 apply. mahjong phase 数稳定 (5-6 个), 短期不会爆.

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
