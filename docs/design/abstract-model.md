# Abstract Model — 三层 fold 结构

mahjong engine pure functional 重构的概念模型. 用 fold 范式形式化"庄/局/操作"
三层时间结构, engine 暴露纯计算 API, 所有外部副作用 (UI/AI/网络/录像) 不进 engine.

---

## 术语

| 中文 | 英文 | 含义 | 时间跨度 |
|---|---|---|---|
| 庄 | **Match** | 整场比赛 (东风 / 半庄 / 一庄). 跨局累积状态. | 数十分钟~1 小时 |
| 局 | **Round** | 一手牌, 配牌→和/流→结算. 局间隔离. | 数分钟 |
| 操作 | **AtomicOp** | 局内不可分动作. 摸是一 op, 切是另一 op. | 一瞬 |

注: 「巡」(junme) 不作一级抽象, 原子模型下退化为"两个连续 op" (Draw + Discard) 加可能的 Pass / 鸣牌.

---

## 设计原则

### 0. Engine 零外部感知 (最高原则)

**Engine 不知道任何外部实体存在.** engine 代码里没有 driver / recorder / network / UI / AI 这些概念 — engine 只见数据和计算.

- engine 代码 **零 import** 来自 `dev::recorder` / `net::*` / `ui::*` / `ai::*`
- engine 没有为外部模块设计的字段 / hook / callback / observer
- engine 不暴露 subscribe / on_event / dispatch 这种主动通知接口

类比 SQLite 与应用程序: SQLite 不知道是谁在调用它, 只管 SQL 进、结果出.

### 1. 数据直接体现业务

`AtomicOp` (合法算子集) + `RoundState`/`MatchState` (累积值) + `OpError` (拒绝原因) 三组类型 **就是** mahjong 业务契约本身. 读类型 = 读规则书. 任何外部模块都是"这套数据契约的消费者", engine 不区分谁消费.

### 2. 计算正确性靠测试, 不靠运行时日志

Engine 的唯一职责是给定 `(state, op)` 永远返回同 `(state', err)`. 由测试验证.

- 没有 `tracing::info!` / `warn!` / `error!` (运行时副作用)
- `tracing::debug!` 仅测试场景插桩, release build 自动抹除
- 没有"防御性兜底" / 静默修复 / 容错 fallback

### 3. 标准 fold 签名

```rust
pub fn round_apply(state: &RoundState, op: AtomicOp) -> Result<RoundState, OpError>;
```

2 入参. `&self` + 内部 clone, 失败时 caller state 不动. clone-everywhere 性能策略接受.

### 4. 错误是"输入合法性裁定", 不是"计算错误"

⚠️ `OpError` 不代表 engine 计算出错 — 那是 bug, 应该 panic. 它代表 caller 喂的 op 在当前 state 下没意义.

类比: `1 + 1 = 2` 永远成功, 因为 i32 在加法运算下封闭. mahjong 的 `(RoundState, AtomicOp)` 笛卡尔积里有大量"语义无效"对 (AwaitCalls 时给 Discard / 切不在手里的牌 / 立直时未听牌), 这些不在合法输入域. Result 标记 caller 喂的 op 是否在域内.

三类 validity 错误:

| 类别 | 例子 | 检测层 |
|---|---|---|
| Phase 错配 | AwaitCalls 时给 Discard | type-state 编译期消大部分; 兜底 runtime |
| 数据级 | Discard 不在手里的 tile | 必须 runtime (类型不能表达) |
| 规则级 | Riichi 时未听牌 / 无役而和 | 必须 runtime |

后两类 Rust 无 dependent types 表达不了, 必然 runtime check. type-state 内层 typed-op 转移逻辑是 **total** (没 Result), 真正的 "1+1" 在那层. 公开 API 包一层 Result 是因为 caller 喂的 AtomicOp 是 untrusted.

---

## fold 是什么 — 概念基础

整个模型架在 **fold** 上. 先定义清楚.

### 定义

fold (在 FP 世界也叫 reduce / accumulate / catamorphism) 是把一个**事件序列**塌缩成一个**累积值**的操作:

| 角色 | 含义 |
|---|---|
| `initial: Acc` | 初始累积值 |
| `events: [E]` | 待处理的事件序列 |
| `step: (Acc, E) -> Acc` | 一步如何把事件应用到累积值上 |

```rust
fn fold<Acc, E>(
    initial: Acc,
    events: impl Iterator<Item = E>,
    step: impl Fn(Acc, E) -> Acc,
) -> Acc {
    let mut acc = initial;
    for e in events { acc = step(acc, e); }
    acc
}
```

求和例子:
```rust
let sum = [1, 2, 3, 4, 5].iter().fold(0, |acc, x| acc + x);
// acc 一路: 0 → 1 → 3 → 6 → 10 → 15
```

### 为什么 fold 不只是 for 循环糖

**1. 确定性 + 可重放**: (initial, events, step) 三件事不变, 输出永远一样. 可精确重建任何中间状态:
```rust
let state_at_step_k = events.iter().take(k).fold(initial, step);
```
event sourcing 的全部基石, replay 的本质.

**2. 强制把"状态"和"事件"概念分开**: 写 fold 强迫你回答三件事:
- 累积值 (state) 是什么? 类型?
- 事件是什么? 几种 variant? 各带什么数据?
- step 怎么把事件应用到 state 上?

这三个问题答清楚, 领域模型 90% 设计完成.

**3. step 是 `(input) -> output`, 单元测试天然干净**.

### try_fold: 可失败版本

step 失败时短路返回 Err:
```rust
fn try_fold<Acc, E, Err>(
    initial: Acc,
    events: impl Iterator<Item = E>,
    step: impl Fn(Acc, E) -> Result<Acc, Err>,
) -> Result<Acc, Err>
```

mahjong 的 `round_apply` 是 try_fold 风格 — caller 喂的 op 可能在域外, Err 是 validity 拒绝.

### 与 reducer / 状态机的关系

熟悉 Redux / Elm / NgRx 的人很眼熟 — fold step 在前端框架里叫 **reducer**: `(state, action) -> state`. 一回事. 也跟有限状态机的转移函数 `δ: (Q, Σ) → Q` 等价.

### 为什么 mahjong 是天然 fold

| mahjong 自然语义 | fold 中的角色 |
|---|---|
| 全局状态 (谁的牌 / 谁的河 / 牌山剩多少) | Acc (`RoundState`) |
| 玩家决策 (摸切碰立直) | Event (`AtomicOp`) |
| 应用决策的规则 | step (`round_apply`) |
| 一局完整过程 | fold |

### 嵌套 fold = 多层级状态

mahjong 不止一层. 一庄 = 多局, 一局 = 多个 op:

```rust
let match_final = ROUNDS.try_fold(init_match, |m, _round_idx| {
    let round_init  = init_round(&m, seed);
    let round_final = OPS_of_this_round.try_fold(round_init, round_apply)?;
    let outcome     = summarize_round(&round_final).expect("ended");
    Ok(match_apply(&m, outcome))
});
```

外层 fold 的 step 内嵌一次内层 fold. 每局跑完 summarize 喂给外层. 这是后面"三层 fold 结构"展开的核心.

---

## 三层 fold 结构

```
┌──────────────────────────────────────────────────────────────────────┐
│ Layer 1 — 庄 (Match)                                                │
│   match_apply(MatchState, RoundOutcome) -> MatchState                │
│      ↑ event = 内层 summary                                          │
├──────────────────────────────────────────────────────────────────────┤
│ Layer 2 — 局 (Round)                                                │
│   round_apply(RoundState, AtomicOp) -> Result<RoundState, OpError>   │
│      ↑ event = 玩家/AI/网络/录像 任意 driver 喂入                   │
├──────────────────────────────────────────────────────────────────────┤
│ Layer 3 — 操作 (AtomicOp)                                           │
│   纯枚举, 自身无状态, 是 Layer 2 的 event 参数                       │
└──────────────────────────────────────────────────────────────────────┘
```

完整一庄:
```rust
match_state = ROUNDS.fold(match_apply, init_match_state)
其中 ROUNDS 每项 = summarize_round(round_final),
     round_final = OPS_in_round.try_fold(round_apply, init_round_state)
```

两层 fold 互相嵌套.

---

## Layer 1: 庄 (Match)

### State

```rust
pub struct MatchState {
    pub scores: [i32; 4],
    pub dealer: Seat,
    pub round_wind: RoundWind,
    pub kyoku: u8,
    pub honba: u8,
    pub riichi_sticks_pool: u32,
    pub rules: GameRules,         // 整庄不变
    pub ended: bool,
}
```

### Outcome (内层产出, 外层消费)

```rust
pub enum RoundOutcome {
    Win {
        winner: Seat,
        is_tsumo: bool,
        loser: Option<Seat>,
        payments: Vec<PaymentDistribution>,
        riichi_sticks_won: u32,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
        tenpai: [bool; 4],
        riichi_sticks_added: u32,
    },
}
```

### Transition

```rust
pub fn match_apply(state: &MatchState, outcome: RoundOutcome) -> MatchState {
    let mut s = state.clone();
    match outcome {
        RoundOutcome::Win { winner, payments, riichi_sticks_won, .. } => {
            apply_payments(&mut s.scores, &payments);
            s.scores[winner.idx()] += riichi_sticks_won as i32 * 1000;
            s.riichi_sticks_pool = 0;
            if winner == s.dealer {
                s.honba += 1;
            } else {
                s.dealer = s.dealer.next();
                s.honba = 0;
                advance_kyoku(&mut s);
            }
        }
        RoundOutcome::Ryuukyoku { tenpai, riichi_sticks_added, .. } => {
            apply_tenpai_payments(&mut s.scores, &tenpai);
            s.riichi_sticks_pool += riichi_sticks_added;
            s.honba += 1;
            if !tenpai[s.dealer.idx()] {
                s.dealer = s.dealer.next();
                advance_kyoku(&mut s);
            }
        }
    }
    s.ended = check_match_ended(&s);
    s
}
```

(伪代码用 `&mut s.clone()` 简洁表达, 真实实现遵循 §设计原则 3 的 `&self` + 内部 clone.)

---

## Layer 2: 局 (Round) — 4 层架构

外部 API 是简单 fold, engine 内部用 type-state 保 invariant. 4 层架构 + macro 自动生成 boilerplate 兼得二者:

```
┌─────────────────────────────────────────────────────────────────┐
│ L1 数据层  AtomicOp                       (单一统一算子集)        │ ← 序列化/录像/网络
├─────────────────────────────────────────────────────────────────┤
│ L4 桥接   try_op(AtomicOp) -> Result<TypedOp, OpError>           │ ← 唯一 runtime 裁定
├─────────────────────────────────────────────────────────────────┤
│ L3 类型化op AwaitDiscardOp / AwaitCallsOp / ...                  │ ← engine 内部 (typed_op! 宏生成)
├─────────────────────────────────────────────────────────────────┤
│ L2 类型化state AwaitDiscardState / AwaitCallsState / ...         │ ← type-state
└─────────────────────────────────────────────────────────────────┘
                                ↓
                      RoundState (enum 包装) ← 公开 state 类型
```

### 公开 state

```rust
pub enum RoundState {
    AwaitDiscard(AwaitDiscardState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitRinshanDraw(AwaitRinshanDrawState),
    AwaitCalls(AwaitCallsState),
    RoundEnd(RoundEndState),
}
```

### 类型化 state (L2)

每个 state 只携带它**确定有效**的字段, 共享字段进 `CommonRound` 子 struct:

```rust
pub struct CommonRound {
    pub rules: GameRules,
    pub round_wind: RoundWind,
    pub kyoku: u8,
    pub honba: u8,
    pub riichi_sticks_pool: u32,
    pub dealer: Seat,
    pub players: [PlayerState; 4],
    pub wall: Wall,
    pub first_go_around: bool,
}

pub struct AwaitDiscardState {
    common: CommonRound,
    turn: Seat,
    last_drawn: Tile,         // 不是 Option, 类型保证
}

pub struct AwaitRiichiDiscardState {
    common: CommonRound,
    turn: Seat,
    last_drawn: Tile,
    // 进入此 state 表明 riichi 已宣告, 唯一合法 op = Discard
}

pub struct AwaitRinshanDrawState {
    common: CommonRound,
    turn: Seat,
    // 杠刚执行完, 唯一合法 op = RinshanDraw
}

pub struct AwaitCallsState {
    common: CommonRound,
    last_discard: (Seat, Tile),    // 不是 Option
}

pub struct RoundEndState {
    common: CommonRound,
    result: RoundResult,
}
```

### 公开 AtomicOp (L1)

```rust
pub enum AtomicOp {
    // ─── 引擎自动 (driver 据 state 自然推断) ───
    Draw,
    RinshanDraw,

    // ─── AwaitDiscard 算子 ───
    Discard(Tile),
    RiichiDeclare,
    Tsumo,
    Ankan(TileIndex),
    Shouminkan(TileIndex),

    // ─── AwaitRiichiDiscard 算子 (复用 Discard) ───

    // ─── AwaitCalls 算子 ───
    Pon { who: Seat, hand_tile_ids: [u16; 2] },
    Chi { who: Seat, hand_tile_ids: [u16; 2] },
    Minkan { who: Seat, hand_tile_ids: [u16; 3] },
    Ron { who: Seat },
    Pass,
}
```

### 类型化 op (L3) — 宏自动生成

每个 state 接受的 AtomicOp 子集. 用 declarative macro 一行声明:

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

typed_op! {
    AwaitCallsOp from AtomicOp {
        Pon { who: Seat, hand_tile_ids: [u16; 2] },
        Chi { who: Seat, hand_tile_ids: [u16; 2] },
        Minkan { who: Seat, hand_tile_ids: [u16; 3] },
        Ron { who: Seat },
        Pass,
    }
}
```

宏展开生成:
1. typed-op enum 本身
2. `try_from_atomic(op: AtomicOp) -> Result<Self, OpError>` (列出的 variant 翻译, 其它 → `OpError::IllegalForPhase`)
3. `From<TypedOp> for AtomicOp` (反向, 录像复用)

### 桥接 (L4) — 调用 typed_op 自动生成的 try

```rust
impl AwaitDiscardState {
    fn try_op(&self, op: AtomicOp) -> Result<AwaitDiscardOp, OpError> {
        AwaitDiscardOp::try_from_atomic(op)
    }
}
// 同理 AwaitCallsState::try_op 等
```

### 转移逻辑 (typed apply) — 手写, **total**, 无 Result

输入已 validated, 内部代码不需要 phase check / Option unwrap:

```rust
impl AwaitDiscardState {
    fn apply(self, op: AwaitDiscardOp) -> NextDiscardState {
        match op {
            AwaitDiscardOp::Discard(t) => {
                let mut common = self.common;
                let p = &mut common.players[self.turn.idx()];
                remove_from_hand(p, t.id);
                p.river.push(t);
                p.last_drawn = None;
                NextDiscardState::AwaitCalls(AwaitCallsState {
                    common,
                    last_discard: (self.turn, t),
                })
            }
            AwaitDiscardOp::RiichiDeclare => {
                let mut common = self.common;
                apply_riichi_flag(&mut common.players[self.turn.idx()]);
                common.riichi_sticks_pool += 1;
                NextDiscardState::AwaitRiichiDiscard(AwaitRiichiDiscardState {
                    common,
                    turn: self.turn,
                    last_drawn: self.last_drawn,
                })
            }
            AwaitDiscardOp::Tsumo => {
                let score = compute_tsumo_score(&self);  // total — 此时一定能算
                let common = self.common;
                NextDiscardState::RoundEnd(RoundEndState {
                    common,
                    result: RoundResult::Win { winner: self.turn, is_tsumo: true, score, ... },
                })
            }
            AwaitDiscardOp::Ankan(kind) => {
                let mut common = self.common;
                apply_ankan(&mut common.players[self.turn.idx()], kind);
                common.wall = reveal_next_dora(common.wall);
                NextDiscardState::AwaitRinshanDraw(AwaitRinshanDrawState {
                    common,
                    turn: self.turn,
                })
            }
            AwaitDiscardOp::Shouminkan(_) => { ... }
        }
    }
}

pub enum NextDiscardState {
    AwaitCalls(AwaitCallsState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitRinshanDraw(AwaitRinshanDrawState),
    RoundEnd(RoundEndState),
}

impl From<NextDiscardState> for RoundState { ... }
```

注意: typed apply 内 `compute_tsumo_score` 等也是 **total** — 进入这条分支已经经过了 `try_op` 裁定 (Tsumo 在 op 列表里) 加上构造 state 时的不变量. 但更细的规则级 validity (有役? 不振听?) 还是要在 `try_op` 里查, 不能放到 typed apply.

→ 实际上 try_op 不只是"列出 variant", 而是**完整的 validity 检查**:

```rust
impl AwaitDiscardState {
    fn try_op(&self, op: AtomicOp) -> Result<AwaitDiscardOp, OpError> {
        // 1. 先用 macro 生成的 try_from_atomic 检查 phase 错配
        let typed = AwaitDiscardOp::try_from_atomic(op)?;
        
        // 2. 再做数据级 + 规则级检查
        match &typed {
            AwaitDiscardOp::Discard(t) => {
                let p = &self.common.players[self.turn.idx()];
                if !hand_contains(p, t.id) {
                    return Err(OpError::TileNotInHand(t.id));
                }
                if p.riichi && self.last_drawn.id != t.id {
                    return Err(OpError::RiichiMustTsumogiri);
                }
            }
            AwaitDiscardOp::RiichiDeclare => {
                let p = &self.common.players[self.turn.idx()];
                if !p.hand.is_menzen() { return Err(OpError::NotMenzen); }
                if p.score < 1000 { return Err(OpError::InsufficientScore); }
                if self.common.wall.remaining() < 4 { return Err(OpError::InsufficientWall); }
                if !is_tenpai_after_discard_any(p, self.last_drawn) {
                    return Err(OpError::NotTenpaiForRiichi);
                }
            }
            AwaitDiscardOp::Tsumo => {
                if !is_winning_tsumo(self) { return Err(OpError::NotWinning); }
                if compute_yaku_count(self) == 0 { return Err(OpError::NoYaku); }
            }
            AwaitDiscardOp::Ankan(kind) => {
                let counts = count_by_kind(&self.common.players[self.turn.idx()].hand.closed);
                if counts[kind.0 as usize] < 4 {
                    return Err(OpError::InsufficientForAnkan(*kind));
                }
                if self.common.players[self.turn.idx()].riichi {
                    return Err(OpError::AnkanWhileRiichi);  // 立直后简化禁
                }
            }
            // ...
        }
        
        Ok(typed)
    }
}
```

`try_op` 是完整 validity gate: phase + 数据 + 规则 都查完, 通过后 `apply` 是干净的 total 函数.

### 公开 round_apply 实现

```rust
pub fn round_apply(state: &RoundState, op: AtomicOp) -> Result<RoundState, OpError> {
    let s = state.clone();
    match s {
        RoundState::AwaitDiscard(st) => {
            let typed = st.try_op(op)?;
            Ok(st.apply(typed).into())
        }
        RoundState::AwaitRiichiDiscard(st) => {
            let typed = st.try_op(op)?;
            Ok(st.apply(typed).into())
        }
        RoundState::AwaitRinshanDraw(st) => {
            let typed = st.try_op(op)?;
            Ok(st.apply(typed).into())
        }
        RoundState::AwaitCalls(st) => {
            let typed = st.try_op(op)?;
            Ok(st.apply(typed).into())
        }
        RoundState::RoundEnd(_) => Err(OpError::AlreadyEnded),
    }
}
```

---

## Engine 暴露的全部 API

```rust
// ─── 数据类型 ───
pub enum AtomicOp { ... }
pub enum RoundState { ... }
pub struct MatchState { ... }
pub enum OpError { ... }
pub enum RoundOutcome { ... }
pub struct LegalOps { ... }   // legal_ops 返回值

// ─── 纯函数 ───
pub fn round_apply(state: &RoundState, op: AtomicOp) -> Result<RoundState, OpError>;
pub fn match_apply(state: &MatchState, outcome: RoundOutcome) -> MatchState;
pub fn legal_ops(state: &RoundState) -> LegalOps;
pub fn summarize_round(state: &RoundState) -> Option<RoundOutcome>;  // RoundEnd 时 Some
pub fn init_round(m: &MatchState, seed: u64) -> RoundState;
pub fn init_match(rules: GameRules) -> MatchState;
```

就这些. 没有别的. 没有 hooks / observers / callbacks / drivers / loggers.

### Engine 内部代码 *绝对不做* 的事

- 任何 IO (文件 / 网络 / stdout / 任何 syscall)
- `tracing::info!` / `warn!` / `error!`
- 用户提示文案 / 错误展示 / i18n
- 业务序列调度 (engine 没有 `play_round` 函数 — 那是外部的事)
- import 任何来自 `dev` / `net` / `ui` / `ai` 的类型

### Engine 完全无感知的外部使用模式

外部基于 engine 数据契约组装业务. 几个示例 (这些代码**不在 engine 里**):

```rust
// 用法 1: 整局对战 (UI 应用层)
fn run_game_loop(state: RoundState) -> RoundState {
    let mut s = state;
    while !matches!(s, RoundState::RoundEnd(_)) {
        let op = pick_op_somehow(&s);                  // UI / AI / 网络
        match engine::round_apply(&s, op) {
            Ok(next) => { s = next; }
            Err(e)   => { show_error(&e); /* 重试 / 中断 */ }
        }
    }
    s
}

// 用法 2: 录像 + 重放
fn replay(initial: RoundState, ops: &[AtomicOp]) -> Result<RoundState, OpError> {
    ops.iter().cloned().try_fold(initial, |s, op| engine::round_apply(&s, op))
}

// 用法 3: 网络同步
fn on_network_op(local: &RoundState, op_bytes: &[u8]) -> Result<RoundState, AppError> {
    let op: AtomicOp = serde_json::from_slice(op_bytes)?;
    engine::round_apply(local, op).map_err(AppError::from)
}

// 用法 4: 测试 fixture
#[test]
fn pon_after_discard() {
    let s = setup_state_with_two_5p_in_south_hand();
    let s = engine::round_apply(&s, AtomicOp::Discard(tile_5p())).unwrap();
    let s = engine::round_apply(&s, AtomicOp::Pon { who: South, hand_tile_ids: [...] }).unwrap();
    assert!(matches!(s, RoundState::AwaitDiscard(_)));
}
```

---

## OpError 设计

按"无效原因"组织, 每个 variant 反映 mahjong 规则的反面陈述:

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpError {
    // ─── 数据级 ───
    #[error("手中无 id={0} 的牌")]
    TileNotInHand(u16),
    #[error("当前无 last_discard, 无法响应")]
    NoLastDiscard,
    #[error("当前家手中无 4 张 {0:?} 同 kind 牌, 不能暗杠")]
    InsufficientForAnkan(TileIndex),
    #[error("当前家无 {0:?} 同 kind 的副露刻子, 不能加杠")]
    NoMatchingPonForShouminkan(TileIndex),

    // ─── 规则级 ───
    #[error("立直方必须摸切")]
    RiichiMustTsumogiri,
    #[error("有副露不能立直")]
    NotMenzen,
    #[error("切此牌后未听牌, 不能立直")]
    NotTenpaiForRiichi,
    #[error("分数 < 1000, 不能立直")]
    InsufficientScore,
    #[error("牌山剩余 < 4, 不能立直")]
    InsufficientWall,
    #[error("立直方不能暗杠/加杠")]
    KanWhileRiichi,
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

    // ─── Phase 错配 (type-state 路径下大部分编译期消, 兜底用) ───
    #[error("op 在当前 phase 不合法")]
    IllegalForPhase {
        op_kind: AtomicOpKind,
        phase_kind: PhaseKind,
    },

    // ─── 边界态 ───
    #[error("局已结束, 不接受任何 op")]
    AlreadyEnded,
}
```

注意: **没有 `Internal(&'static str)` variant**. engine 内部走到"理论不可达"是 engine bug, 应 panic, 不该作为 OpError 漏给 caller. Caller 看到 OpError 总能保证是输入问题.

---

## 一局完整 trace 示例

initial state: East 庄, dealer 配 13 张, wall 70 张, RoundState::AwaitDiscard (start_round 内部已注入 Draw, 跳过).

实际更精确: `init_round` 返回的状态是 **配牌完毕但未摸牌** 的 phase, 第一个 op 必然是 Draw. 取决于实现选择 — 这里假设 init_round 返回 `AwaitDiscard` (东家已摸第一张):

```
init_round → AwaitDiscard{turn=East, last_drawn=4m}

ops:
  1.   Discard(9p)                          → AwaitCalls{last_discard=(East,9p)}
  2.   Pass                                 → AwaitDiscard{turn=South, last_drawn=2s}
                                              (engine 内 Pass 自动驱动 Draw 进入)
  3.   Discard(2s)                          → AwaitCalls{last_discard=(South,2s)}
  4.   Pon{who=North, hand_tile_ids=[...]}  → AwaitDiscard{turn=North, ...} (副露 [2s 2s 2s])
  5.   Discard(W风)                         → AwaitCalls{last_discard=(North,W)}
  6.   Pass                                 → AwaitDiscard{turn=East, last_drawn=...}
  ...  (省略 N 步)
 K.    RiichiDeclare                        → AwaitRiichiDiscard{turn=East, last_drawn=8m}
                                              (riichi=true, score-=1000, pool+=1)
 K+1.  Discard(8m)                          → AwaitCalls{...}
                                              (riichi_river_idx 自动写入)
 K+2.  Pass                                 → AwaitDiscard{turn=South, ...}
  ...
 M.    Ankan(发)                            → AwaitRinshanDraw{turn=East, ...}
                                              (新 dora 翻开)
 M+1.  RinshanDraw                          → AwaitDiscard{turn=East, last_drawn=岭上牌}
 M+2.  Discard(...)
  ...
 N.    Tsumo                                → RoundEnd{result=Win{...}}
```

`summarize_round(&final_state)` 返 `Some(RoundOutcome::Win{..})` → 喂 `match_apply` 更新 MatchState.

注: 上面 trace 把 Pass 之后的 Draw 隐含了. 如果要严格录像 Draw 也是显式 op, 序列变成:
```
Pass → Draw → Discard → AwaitCalls → ...
```
具体取决于实施时是否把 Draw 内嵌 Pass (录像更紧凑) 还是保持显式 op (录像更完整). 这是实施细节, 待落地时拍.

---

## 与现有 codebase 的对应关系

| 抽象层 | 当前代码 | refactor 后 |
|---|---|---|
| MatchState | `GameState` 顶层字段 | 独立 struct |
| match_apply | 散在 `next_round` + `apply_payments` + `declare_*` 末尾 | 单一函数 |
| RoundState | `GameState` 全部 (单 struct + phase 字段) | enum + N 个 typed state |
| round_apply | `do_*` 方法各一 | 单一入口, 内部 dispatch + try_op + typed apply |
| AtomicOp | `dev/recorder.rs::RecordedAction` (近似) | 升级为 engine 公开类型, 加 Draw / RinshanDraw |
| OpError | `Result<(), &'static str>` (字符串) | typed enum, thiserror |
| Wall | `Wall` (内部 `&mut self.live` pop) | `Wall::draw(&self) -> (Wall, Option<Tile>)` |
| events | `events: VecDeque<GameEvent>` 字段 | round_apply 返回值额外的 Vec<Event>, 不在 state 里 |
| recorded_actions | `recorded_actions: Option<Vec<...>>` 字段 | 删, 录像在 engine 外 |

→ 实际改动是**提取 + 重命名 + 拆分边界**, 算法不动.
