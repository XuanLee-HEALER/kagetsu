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

候选:
- **保持单一 struct**, 用 `Phase` 枚举字段表达阶段.
- **Type-state 模式**: 把 GameState 拆成 `DealState` / `AwaitDiscardState` / `AwaitCallsState` / `RoundEndState`. 每个 state 只允许特定 transition. 编译期保证非法转移不可表示.
  ```rust
  enum GameState {
      Dealing(DealState),
      AwaitDiscard(AwaitDiscardState),  // turn 一定有效, last_drawn 一定 Some
      AwaitCalls(AwaitCallsState),      // last_discard 一定 Some
      RoundEnd(RoundEndState),
      GameEnd(GameEndState),
  }
  ```

> 决策: TODO. 倾向 type-state, 但 ai/UI 都得跟着重写 match arms.

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

### 4.1 单一入口还是多入口?

候选:
- **单一**: `pub fn apply(state: GameState, action: Action) -> Result<(GameState, Vec<Event>), Error>`
- **多入口**: 保留 `do_discard / do_pon / ...`, 各自 consume+return. 更直接但 ai/UI 还是要写 dispatch.
- **type-state 限定**: `AwaitDiscardState::apply(self, AwaitDiscardAction) -> ...`. 编译期 dispatch.

> 决策: TODO.

### 4.2 Action enum

应该统一所有可能动作还是按 phase 拆?

```rust
// 选 1: 单一 Action, 各 variant
enum Action {
    Discard(Tile),
    Pon { who: Seat, two: [Tile; 2] },
    Chi { ... },
    // ...
    AdvanceTurn,  // 从 AwaitCalls 推到 Draw
    Pass,         // 鸣牌窗口跳过
}

// 选 2: 按 phase 拆
enum AwaitDiscardAction { Discard(Tile), Riichi(Tile), Tsumo, Ankan(TileIndex), Shouminkan(TileIndex) }
enum AwaitCallsAction { Pon { ... }, Chi { ... }, Minkan { ... }, Ron { who }, Pass }
```

### 4.3 自动转换

`Phase::Draw` 不是用户决策, 引擎自己 do_draw. 应该:
- `apply` 自动消耗 Draw 阶段, 一直推进到 AwaitDiscard / RoundEnd 才返回?
- 还是 `apply` 一步只走一步, wrapper 循环调?

> 决策: TODO. 当前 dev/recorder.rs replay 的循环就是 wrapper 模式, 留作参考.

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

文件存档 / 网络 IO / `tracing::info!` 全在 wrapper. ✅

但 `tracing::info!` 在引擎里也有几处 (advance_turn 等). 要清理吗?
> 决策: TODO. 倾向把它们也搬到 wrapper.

## 6. 错误模型

当前: `Result<(), &'static str>` — 简洁但不结构化.

候选:
- 保留 `&'static str`, 调用方自己拼 message.
- 引入 `EngineError` enum, `thiserror` 派生.
- pattern matchable error code + display string.

> 决策: TODO.

错误回退语义:
- consume-self-return-self: 失败时 state 已被 move 走, **怎么把原 state 还给 caller**?
  - A. 让 `apply(self, ...) -> Result<Self, (Self, Error)>` (Err 带回 state).
  - B. caller `clone` 后再 apply.
  - C. `apply(&self, ...) -> Result<NewState, Error>` (永不消费, 内部 clone).

> 决策: TODO. C 最简单, A 最严, B 居中.

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
  + tokio driver 可行, 但 mental_poker actor 已经是这风格, p2p swarm 不是.
- **Type-state 拆分会爆炸 match**: ai/UI 每个地方都要 dispatch 4-5 个 phase variant. 体验未必好.
- **Recorder pop-and-replace 重构**: 那个 hack 消失后 recorder 实现要重写. 测试得重跑.
- **学习收益 vs 时间成本不对等**: 设计都讨论清楚后, 实施部分只是机械 typing,
  风险点都在前面这些决策上.

## 决策状态总览

| 节 | 决策 | 状态 |
|---|---|---|
| 1 | 范围 | ⬜ |
| 2 | "Pure" 严格度 | ⬜ |
| 3.1 | GameState 拆不拆 | ⬜ |
| 3.2 | Wall consume 模式 | ⬜ |
| 3.3 | events 出 state | ⬜ |
| 4.1 | apply 单/多入口 | ⬜ |
| 4.2 | Action 单/多 enum | ⬜ |
| 4.3 | 自动转换 | ⬜ |
| 5.1 | RNG 模型 | ⬜ |
| 5.3 | tracing 是否清掉 | ⬜ |
| 6 | Error 类型 + 回退语义 | ⬜ |
| 7 | 性能策略 | 暂定 clone-everywhere ✅ |
| 8.1 | 测试不动 | ✅ |
| 8.2 | bottom-up 迁移 | ✅ |
| 9 | crate 拆分 | ⬜ |

---

**建议讨论起点**: 先攻 §1 (范围) + §2 (严格度), 这两节定了之后下面都收敛.
确认后我们继续讨论 §3-§4 (数据 + 转换模型), 那是最核心的设计选择.
