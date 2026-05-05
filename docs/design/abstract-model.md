# Abstract Model — 三层 fold 结构

> 基于用户提出的 3 层抽象: 庄 / 局 / 操作 (atomic op).
> 概念 + 伪代码, 不涉及实施细节. 配套 scratchpad 见 `pure-functional-refactor.md`.

## 设计原则 (已锁定)

这些是后续所有设计选择的硬约束:

1. **Engine = 计算库**, 算子集 + 转移函数. 类似 SQL relational algebra 那种"预定义算子, 别处实现都基于此".
2. **Engine 的唯一职责是计算正确性**. 由测试用例验证, **不依靠运行时日志 / 防御性兜底**.
3. **Driver (调用方) 负责所有副作用**: 业务操作序列调度 / 用户交互 / 网络同步 / 持久化 / 日志 / 错误展示.
4. **Engine 内 `tracing::info!` 等运行时日志 = 副作用, 不出现**. 仅在测试场景用 `tracing::debug!` 辅助 (或干脆不用, 全靠 assertion).
5. **签名形式**: 标准 fold 风格 `(state, op) -> Result<state, error>`. 2 个入参, 不引入"前一选手 / 影响值 / 新选手初始" 这种结构性分解.
6. **错误是结构化的**: engine 返回 typed `OpError` enum, driver 决定怎么展示 / 重试 / 中断.

## 术语

| 中文 | 英文 | 含义 | 时间跨度 |
|---|---|---|---|
| 庄 | **Match** | 整场比赛 (东风 / 半庄 / 一庄等). 有跨局累积的状态. | 数十分钟 ~ 1 小时 |
| 局 | **Round** | 一手牌, 配牌 → 和/流 → 结算. 局间隔离. | 数分钟 |
| 操作 | **AtomicOp** | 局内一次不可分的动作. 摸是一个 op, 切是另一个. | 一瞬 |

注: 「巡」(junme, 一摸一切的循环) 不是这里的"操作", 它在原子模型下退化成"两个连续 op"
(Draw + Discard) 加可能的 Pass / 鸣牌. 我们不把"巡"作为一级抽象.

## fold 是什么 — 概念前置

后面的整个模型架在 **fold** 这个抽象上, 先把它讲清楚.

### 定义

fold (在 FP 世界也叫 **reduce / accumulate / catamorphism**) 是把一个**事件序列**
塌缩成一个**累积值**的操作. 三个角色:

| 角色 | 含义 |
|---|---|
| `initial: Acc` | 初始累积值 |
| `events: [E]` | 待处理的事件序列 |
| `step: (Acc, E) -> Acc` | 一步如何把事件应用到累积值上 |

签名:

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

最朴素的例子, 求和:
```rust
let sum = [1, 2, 3, 4, 5].iter().fold(0, |acc, x| acc + x);
//                                ↑           ↑
//                              initial      step
//   acc 一路: 0 → 1 → 3 → 6 → 10 → 15
```

step 没有副作用, 只是 `(acc, event) -> acc`. 走完整个序列, 拿到最终累积值.

### 为什么 fold 不只是 "for 循环的语法糖"

实现上是个 for 循环没错, 但**形式化它**有几个本质好处:

**1. 确定性 + 可重放**

只要 (initial, events, step) 三个东西不变, 输出**永远一样**——没有隐藏内部状态,
没有副作用. 所以**只要给我事件序列, 我能精确重建任何中间状态**:

```rust
let state_at_step_k = events.iter().take(k).fold(initial, step);
```

这就是 event sourcing 的全部基石, 也是 `dev/recorder.rs::replay` 已经在做的事
(它就是个 fold, 只是没用 Iterator API 写).

**2. 强制把"状态"和"事件"概念分开**

写 fold 强迫你回答三个问题:
- 累积值 (state) 是什么? 类型是?
- 事件是什么? 有几种 variant? 各带什么数据?
- step 怎么把事件应用到 state 上?

这三个问题答清楚, 领域模型就 90% 设计完了. 对 mahjong 来说: 明确
`RoundState` / `AtomicOp` / `round_apply` 三件事, 一局的完整语义就被严格刻画了.

**3. 状态/事件分离, 单元测试天然干净**

每个 step 是 `(input) -> output`, 没有 setup/teardown, 没有 mock. 给我两个 input,
我直接断言 output. 这也是 pure 范式整体的好处, fold 是它最显式的体现.

### try_fold: 可失败版本

step 可能失败时, 用 `try_fold`, Err 短路:

```rust
fn try_fold<Acc, E, Err>(
    initial: Acc,
    events: impl Iterator<Item = E>,
    step: impl Fn(Acc, E) -> Result<Acc, Err>,
) -> Result<Acc, Err>
```

mahjong 的 `round_apply` 必然是 try_fold 风格——非法 op 应该 Err 而不是 panic.

### 与 "reducer" / Redux / 状态机的关系

熟悉前端 Redux / Elm / NgRx 的会很眼熟——fold step 在那些框架里叫 **reducer**:
`(state, action) -> state`. 一回事.

mahjong 的 `round_apply` 就是个 reducer. 接 AtomicOp action, 返回新 RoundState.
区别只在于 mahjong 是单线程回合制, 不需要响应式 store / dispatch 那套.

也跟 **状态机 (state machine)** 的语义一致——状态机的转移函数 `δ: (Q, Σ) → Q`
本质就是一次 step. 一连串输入沿着状态机走完得到的最终状态, 就是这串输入对
δ 的 fold.

### 为什么 mahjong 是"天然的 fold"

mahjong 本身就是一个 **离散事件驱动 + 全局状态** 的回合制博弈:

| mahjong 自然语义 | fold 中的角色 |
|---|---|
| 全局状态 (谁的牌 / 谁的河 / 牌山剩多少) | Acc (RoundState) |
| 玩家决策 (摸切碰立直) | Event (AtomicOp) |
| 应用决策的规则 (副露后转 turn 等) | step (round_apply) |
| 一局完整过程 | fold |

形式化跟它的天然性对齐. 不像有些领域 (带连续物理量的实时游戏) 强行 fold 反而别扭.

### 嵌套 fold = 多层级状态

mahjong 不止一层. 一庄 = 多局, 一局 = 多个 op. 嵌套两层 fold 就把多层级
表达出来:

```rust
let match_final = ROUNDS.fold(init_match, |m, _round_idx| {
    let round_init  = init_round_from_match(&m);
    let round_final = OPS_of_this_round.fold(round_init, round_apply);
    let outcome     = summarize_round(&round_final);
    match_apply(m, outcome)
});
```

外层 fold 的 step 内嵌一次内层 fold. 每局跑完后 summarize 喂给外层. 这是后面
"三层 fold 结构" 一节要展开的核心.

## 三层 fold 结构

每层都是一个**纯 fold**, 外层 fold 的 event 参数 = 内层 fold 的最终 state 摘要:

```
                ┌─────────────────────────────────────┐
                │  match_apply(MatchState, RoundOutcome) -> MatchState
庄 (Match)      │      ↑ event           ↑ accumulator
                │      |                                
                │   summarize_round(RoundState) -> RoundOutcome
                └─────────────────────────────────────┘
                                  ▲
                                  │
                ┌─────────────────────────────────────┐
                │  round_apply(RoundState, AtomicOp) -> RoundState
局 (Round)      │      ↑ event           ↑ accumulator
                │      |
                │   AtomicOp 由外部 driver 喂入 (玩家决策 + 引擎自动)
                └─────────────────────────────────────┘
                                  ▲
                                  │
                ┌─────────────────────────────────────┐
操作 (AtomicOp) │  纯枚举 + 携带数据. 自身无状态.
                └─────────────────────────────────────┘
```

完整一庄:
```
match_state = ROUNDS.fold(match_apply, init_match_state)
其中 ROUNDS 的每一项 = summarize_round(round_final),
     而 round_final = OPS_in_round.fold(round_apply, init_round_state)
```

两层 fold 互相嵌套, 数学上极简洁.

## Layer 1: 庄 (Match)

### State

```rust
struct MatchState {
    scores: [i32; 4],
    dealer: Seat,
    round_wind: RoundWind,    // 东 / 南 / 西 / 北
    kyoku: u8,                // 1..=4
    honba: u8,                // 本场数
    riichi_sticks_pool: u32,  // 桌面累积立直棒
    rules: GameRules,         // 不变, 整庄沿用
    ended: bool,
}
```

### Event 输入 (RoundOutcome)

```rust
enum RoundOutcome {
    Win {
        winner: Seat,
        is_tsumo: bool,
        loser: Option<Seat>,
        payments: Vec<PaymentDistribution>,
        riichi_sticks_won: u32,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
        tenpai: [bool; 4],         // 流局时谁听牌, 用于罚符
        riichi_sticks_added: u32,  // 该局立直棒贡献到桌面
    },
}
```

### Transition

```rust
fn match_apply(s: MatchState, o: RoundOutcome) -> MatchState {
    let mut s = s;
    match o {
        Win { winner, is_tsumo, payments, riichi_sticks_won, .. } => {
            apply_payments(&mut s.scores, &payments);
            s.riichi_sticks_pool = 0;
            s.scores[winner.idx()] += riichi_sticks_won as i32 * 1000;

            if winner == s.dealer {
                s.honba += 1;     // 庄家和: 连庄 + 本场 +1
            } else {
                s.dealer = s.dealer.next();
                s.honba = 0;
                advance_kyoku(&mut s);  // 可能涉及换 round_wind
            }
        }
        Ryuukyoku { tenpai, riichi_sticks_added, .. } => {
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

(实施时还有西入 / 飛び / 王座決定戦等终局判定细节, 都集中在 `check_match_ended` 内.)

## Layer 2: 局 (Round)

### State

```rust
struct RoundState {
    // 静态参数 (从 MatchState 注入, 局内不变)
    rules: GameRules,
    round_wind: RoundWind,
    kyoku: u8,
    honba: u8,
    riichi_sticks_pool: u32,
    dealer: Seat,

    // 动态状态
    players: [PlayerState; 4],   // hands / river / melds / riichi flags / last_drawn
    wall: Wall,
    turn: Seat,
    phase: Phase,                // Draw / AwaitDiscard / AwaitCalls / RoundEnd
    last_discard: Option<(Seat, Tile)>,
    first_go_around: bool,
    last_result: Option<RoundResult>,   // RoundEnd 时填入, summarize_round 据此抽 RoundOutcome
}

enum Phase {
    Draw,                  // 等 turn 玩家摸
    AwaitDiscard,          // 等 turn 玩家切 (或自摸/立直宣告/暗杠/加杠)
    AwaitRiichiDiscard,    // 立直已宣告, 必须切立直牌 (无其它选项)
    AwaitCalls,            // 等其他玩家是否鸣 (Pon/Chi/Kan/Ron) 或 Pass
    RoundEnd,              // 局已结束, 不再接受 op
}
```

### Event 输入 (AtomicOp) — 算子集

AtomicOp 是 engine 暴露给 driver 的**唯一**计算原语集合 — 类似关系代数的 σ/π/⋈,
所有上层逻辑都基于这些算子组合. 不增不减.

```rust
enum AtomicOp {
    // ─── 引擎自动注入 (无玩家决策, driver 调度时识别 phase 自行喂入) ───
    Draw,                                          // 从 wall pop 一张到 turn 玩家
    RinshanDraw,                                   // 杠后从岭上摸

    // ─── AwaitDiscard 阶段算子 ───
    Discard(Tile),                                 // 普通切牌
    RiichiDeclare,                                 // 仅"宣告立直", 不含切. 之后 phase=AwaitRiichiDiscard
    Tsumo,                                         // 自摸
    Ankan(TileIndex),                              // 暗杠
    Shouminkan(TileIndex),                         // 加杠

    // ─── AwaitRiichiDiscard 阶段算子 (唯一合法 op 是 Discard) ───
    //     (复用上面的 Discard, 不需要单独 variant. 内部据 phase 区分语义)

    // ─── AwaitCalls 阶段算子 ───
    Pon  { who: Seat, hand_tile_ids: [u16; 2] },
    Chi  { who: Seat, hand_tile_ids: [u16; 2] },
    Minkan { who: Seat, hand_tile_ids: [u16; 3] },
    Ron  { who: Seat },
    Pass,                                          // 整个鸣牌窗口关闭, 没人响应
}
```

**Riichi 拆 2 op 的设计意图** (已锁定):
- `RiichiDeclare`: 设立 player.riichi=true, 扣 1000 点, 标记 ippatsu_active. **不涉及切牌**.
- 其后 phase = `AwaitRiichiDiscard`, 唯一合法 op 是 `Discard(t)`.
- `Discard(t)` 在 `AwaitRiichiDiscard` 下执行时, 顺便把 t 在 river 中的索引写入 `riichi_river_idx` (UI 横置用).
- 这样 record/replay 看到的就是 `[..., RiichiDeclare, Discard(8m), ...]` 两步, 没有 pop-and-replace 那种 hack.

### Transition

```rust
fn round_apply(s: RoundState, op: AtomicOp) -> Result<RoundState, OpError> {
    if !is_legal(&s, &op) { return Err(IllegalOp); }
    let mut s = s;
    match op {
        Draw => {
            let (wall, t) = s.wall.draw();
            s.wall = wall;
            match t {
                Some(t) => {
                    s.players[s.turn.idx()].last_drawn = Some(t);
                    s.players[s.turn.idx()].hand.closed.push(t);
                    s.phase = Phase::AwaitDiscard;
                }
                None => {
                    // wall 摸尽 → 流局
                    s.phase = Phase::RoundEnd;
                    s.last_result = Some(RoundResult::Ryuukyoku { kind: Howaipai });
                }
            }
        }
        Discard(t) => {
            let p = &mut s.players[s.turn.idx()];
            // 立直后强制摸切
            ensure_riichi_tsumogiri(p, &t)?;
            remove_from_hand(p, t.id);
            p.river.push(t);
            p.last_drawn = None;
            // 立直宣告后的第一切: 写入 river 索引 (UI 横置用)
            if s.phase == Phase::AwaitRiichiDiscard {
                p.riichi_river_idx = Some(p.river.len() - 1);
            }
            s.last_discard = Some((s.turn, t));
            s.phase = Phase::AwaitCalls;
        }

        RiichiDeclare => {
            apply_riichi_flag(&mut s.players[s.turn.idx()]);  // riichi=true, ippatsu_active=true, score-=1000
            s.riichi_sticks_pool += 1;
            s.phase = Phase::AwaitRiichiDiscard;              // 唯一合法下一 op = Discard
        }

        Pon { who, hand_tile_ids } => {
            let (from, called) = s.last_discard.ok_or(NoDiscard)?;
            apply_meld_pon(&mut s.players[who.idx()], hand_tile_ids, called, from);
            s.consume_discard(from);
            s.turn = who;
            s.phase = Phase::AwaitDiscard;       // 鸣方接着切
            s.last_discard = None;
        }
        // Chi / Minkan 类似 Pon

        Ankan(kind) => {
            apply_ankan(&mut s.players[s.turn.idx()], kind);
            s.wall = reveal_next_dora(s.wall);
            // 杠后必须摸岭上 → 由 driver 见 phase=Draw 并已杠的状态时, 喂入 RinshanDraw op
            // (而不是普通 Draw). engine 据 last_meld_was_kan flag 区分.
            s.phase = Phase::Draw;
        }

        Tsumo => {
            let score = compute_tsumo_score(&s)?;
            s.last_result = Some(Win { winner: s.turn, is_tsumo: true, score, ... });
            s.phase = Phase::RoundEnd;
        }
        Ron { who } => {
            let score = compute_ron_score(&s, who)?;
            s.last_result = Some(Win { winner: who, is_tsumo: false, score, ... });
            s.phase = Phase::RoundEnd;
        }

        Pass => {
            // 没人鸣牌, 推到下家
            s.turn = s.turn.next();
            s.last_discard = None;
            s.phase = Phase::Draw;
            // 注: wall 摸尽 → RoundEnd 由下一个 Draw op 内处理
        }
    }
    Ok(s)
}
```

注: 上面伪代码用 `&mut s` 是为了易读. 真实实现按 §1 决策 (consume self / clone-and-return /
type-state) 改写——**外观签名仍是 `(state, op) -> Result<state, err>`**.

### Summary

```rust
fn summarize_round(r: &RoundState) -> RoundOutcome {
    debug_assert!(r.phase == Phase::RoundEnd);
    match &r.last_result {
        Some(RoundResult::Win { .. })       => RoundOutcome::Win { ... },
        Some(RoundResult::Ryuukyoku { .. }) => RoundOutcome::Ryuukyoku { ... },
        None => unreachable!("RoundEnd must have last_result"),
    }
}
```

## Layer 3: 操作 (AtomicOp)

操作本身**没有状态**, 是 transition 的 event 参数. 来源:

- **引擎自动**: `Draw` / `RinshanDraw` 在适当 phase 由 driver 自动注入, 不需玩家决策.
- **玩家决策**: AwaitDiscard / AwaitCalls 阶段, 玩家从 `legal_options(state)` 中选一个 op.
- **AI / 网络**: 同上, 由其他 actor 注入.
- **超时默认**: thinking_time 到期后由 driver 注入 fallback op (通常 Discard(last_drawn) 或 Pass).

录像 (`RecordedAction`) **就是 AtomicOp 的序列化形式**. replay = 把 AtomicOp 流重新喂给 round_apply.

## Engine 责任 vs Driver 责任

这是核心分工原则, 涉及任何具体实现都要回到这里检验.

### Engine 只做一件事: 计算正确性

| 项 | Engine ✅ | Driver ❌ |
|---|---|---|
| 状态转移 (`round_apply` / `match_apply`) | ✓ | |
| 算子合法性判定 (`legal_ops(state)`) | ✓ | |
| 算分 / 役判定 / 副露解析 | ✓ | |
| 立直 / 振听 / 头跳 等规则 | ✓ | |
| 牌山随机化 (洗牌算法) | ✓ | |
| Pure & deterministic, 给定 (state, op) 永远同 (state', err) | ✓ | |

**Engine 不做**:
- 任何 IO (文件 / 网络 / stdout)
- `tracing::info!` 等运行时日志 (副作用)
- 用户提示文案 / 错误展示 / i18n
- 业务序列调度 (谁先谁后 / 超时怎么办 / 网络延迟怎么办)

测试用例覆盖正确性, 不靠日志兜底. `tracing::debug!` 只允许在测试场景临时插桩,
release build 等同删除 (`cfg(debug_assertions)` 或 `RUST_LOG=off` 默认).

### Driver 负责一切副作用

| 项 | Driver ✅ |
|---|---|
| 决策来源调度: 谁的回合 → 找谁要 op (本地用户 / AI / 网络对手 / 录像) | ✓ |
| 思考超时 / fallback op | ✓ |
| 把 engine 返回的 typed Err 翻译成用户可读消息 | ✓ |
| 状态持久化 / 录像存档 / 网络同步 | ✓ |
| UI 渲染节流 / 动画时序 | ✓ |
| 日志 (`tracing::info!` / `warn!` / `error!`) | ✓ |
| Recovery: engine 返回 Err 后是重试 / 跳过 / 中断 | ✓ |

### 实现模板

```rust
// engine 暴露
pub fn round_apply(state: RoundState, op: AtomicOp) -> Result<RoundState, OpError>;
pub fn match_apply(state: MatchState, outcome: RoundOutcome) -> MatchState;
pub fn legal_ops(state: &RoundState) -> LegalOps;
pub fn summarize_round(state: &RoundState) -> RoundOutcome;

// engine 错误是结构化的
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    #[error("op {op:?} 在 phase {phase:?} 下不合法")]
    IllegalForPhase { op: AtomicOpKind, phase: Phase },
    #[error("立直方必须切立直牌")]
    RiichiMustTsumogiri,
    #[error("手中无 id={0} 的牌")]
    TileNotInHand(u16),
    // ...
}

// driver 是普通(可有副作用) Rust 代码
fn play_round(init: RoundState, mut driver: impl Driver) -> Result<RoundState, DriverError> {
    let mut s = init;
    while s.phase != Phase::RoundEnd {
        let op = driver.next_op(&s);          // 副作用: 等用户 / 查 AI / 收网络
        s = match round_apply(s, op) {
            Ok(s) => s,
            Err(e) => {
                driver.on_engine_error(&e);   // 副作用: 提示, 决定是否重试 / 中断
                return Err(DriverError::EngineRejected(e));
            }
        };
    }
    Ok(s)
}
```

driver 多实例:
- `UiDriver`: 本地交互, 用 channel 等用户键盘 op.
- `AiDriver`: 本地计算 op, 立刻返回.
- `NetDriver`: 等远程对手 op 包.
- `ReplayDriver`: 从 `Vec<AtomicOp>` 顺序 pop.
- `TestDriver`: 测试 fixture 喂指定 op 序列.

每种 driver 共用同一个 engine, 互相不知道对方存在.

## 一局完整 trace 示例

initial_round (East 庄, dealer 配 13 张, 其它三家 13 张, wall 70 张):

```
phase=Draw, turn=East
ops:
  1.   Draw                                 → East last_drawn=4m, phase=AwaitDiscard
  2.   Discard(9p)                          → 河 [9p], phase=AwaitCalls, last_discard=(East,9p)
  3.   Pass                                 → turn=South, phase=Draw
  4.   Draw                                 → South last_drawn=2s, phase=AwaitDiscard
  5.   Discard(2s)                          → 河 [2s], phase=AwaitCalls
  6.   Pon{who=North,hand=[2s,2s]}          → North 副露 [2s 2s 2s], turn=North, phase=AwaitDiscard
  7.   Discard(W风)                         → 河 [W], phase=AwaitCalls
  8.   Pass                                 → turn=East, phase=Draw
  9.   Draw                                 → East last_drawn=...
  ...  (省略 N 步)
 K.    RiichiDeclare                        → East riichi=true, score-=1000,
                                              phase=AwaitRiichiDiscard
 K+1.  Discard(8m)                          → 河末尾 8m, riichi_river_idx 自动写入,
                                              phase=AwaitCalls
 K+2.  Pass
 K+3.  Draw                                 → South 摸
  ...
 M.    Ankan(发)                            → East 暗杠发, 翻新 dora, phase=Draw
                                              (last_meld_was_kan=true)
 M+1.  RinshanDraw                          → East 摸岭上, phase=AwaitDiscard
 M+2.  Discard(...)
  ...
 N.    Tsumo                                → 当前 turn 自摸,
                                              last_result=Win{...}, phase=RoundEnd
```

`summarize_round` 抽出 `RoundOutcome::Win{winner, score, payments, ...}`,
喂给 `match_apply` 更新庄状态: dealer 是否换 / honba / kyoku / 整庄是否结束.

下一 round_init 由 `init_round_from_match(&match_state)` 给出 (新的 dealer / honba /
立直棒池 / 重新洗 wall). 进入下一轮 fold.

## 已确认决策

- ✅ **输入模型**: 标准 fold 2 入参, `(state, op) -> Result<state, err>`. 多入参形式不引入.
- ✅ **AtomicOp = 数据层** (单一统一算子集), **type-state = 行为层** (内部不变量), 通过 bridge 函数 `try_op` 连接. 见下面 "数据层 vs 行为层" 一节.
- ✅ **Riichi 拆 2 op**: `RiichiDeclare` + `Discard(t)`. 中间用 phase=`AwaitRiichiDiscard` 锁住, 唯一合法下一 op 是 Discard. `riichi_river_idx` 在 Discard 时自动写入.
- ✅ **岭上摸独立 op**: `Ankan` / `Minkan` 后 phase=Draw, driver 据 last_meld_was_kan flag 喂入 `RinshanDraw` 而非 `Draw`. 录像粒度更明确, 翻新 dora 时机也清晰.
- ✅ **Engine 不带运行时日志**: 没 `tracing::info!`. 仅 `tracing::debug!` 用作测试插桩, release build 自动抹掉.
- ✅ **错误是结构化 typed enum** (`OpError`), driver 负责展示文案.

## 数据层 vs 行为层 — type-state 与统一 AtomicOp 的共存方式

(详细伪代码见 scratchpad §3.1.)

外部 (录像 / 网络 / driver) 用单一 `AtomicOp` enum, 这是数据 — 序列化、传输、记录都用它.
Engine 内部用 type-state — 每个 phase 一个 struct, 类型保证 transition 合法.
两者通过每个 state 上的 `try_op(AtomicOp) -> Result<TypedOp>` bridge 函数连接.

**4 层结构**:
```
L1  AtomicOp                                   ← 数据 (wire format)
       ↓ try_op
L4  AwaitDiscardOp / AwaitCallsOp / ...        ← 类型化算子子集 (engine 内部)
       ↓ apply
L2  AwaitDiscardState / AwaitCallsState / ...  ← typed state
       ↓ .into()
L3  RoundState (enum)                          ← 公开 state 类型
```

公开 API:
```rust
pub fn round_apply(state: RoundState, op: AtomicOp) -> Result<RoundState, OpError>;
```

外部只看到 `(RoundState, AtomicOp) -> Result<RoundState, OpError>` 这套统一签名,
不关心内部 type-state 拆分. 录像/网络/replay 全都基于 AtomicOp, 单一算子代数.

是不是要这样做仍是开放决策点 (`scratchpad §3.1`), 但**不再是与统一 AtomicOp 互斥的二选一**.

## 待定问题 (下一轮讨论)

- **State 拆分**: RoundState 单一 struct 还是 type-state 按 phase 拆? (`scratchpad §3.1`)
  - 配套子问题: AtomicOp 是否也按 phase 拆 (`AwaitDiscardOp` / `AwaitCallsOp` ...)?
    保持单一 enum 与"统一算子集"愿景一致, 但 type-state 下 driver 调度更别扭.
- **Pass 粒度**: 单一 op (整窗口关闭) vs 按家拆 4 op? 倾向单一. 待最终拍板.
- **错误回退语义**: round_apply Err 时, 老 state 怎么还回 caller?
  (`scratchpad §6`: A. Err 带回 state / B. 内部 clone / C. consume self 失败时 state 丢)

## 与现有 codebase 的对应关系

| 抽象层 | 当前代码 |
|---|---|
| MatchState | `GameState` 顶层字段 (rules / round_wind / kyoku / honba / riichi_sticks / players[].score / dealer) |
| match_apply | 散在 `next_round` + `apply_payments` + `declare_*` 末尾. 重构是把它提取成一个函数. |
| RoundState | `GameState` 全部 (复用) |
| round_apply | `do_*` 方法各一. 重构是合并成单一入口 + AtomicOp dispatch. |
| AtomicOp | 已经有 `RecordedAction` 在 dev/recorder.rs, 几乎对应. 把 `Draw` / `RinshanDraw` 加进去就齐了. |
| summarize_round | 当前 `last_result` 字段 (RoundResult) 已基本是这角色, 加一个 thin extract fn. |

→ 实际改动: 把现有逻辑**提取 + 重命名 + 拆分边界**, 算法不动.
