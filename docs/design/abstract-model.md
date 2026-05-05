# Abstract Model — 三层 fold 结构

> 基于用户提出的 3 层抽象: 庄 / 局 / 操作 (atomic op).
> 概念 + 伪代码, 不涉及实施细节. 配套 scratchpad 见 `pure-functional-refactor.md`.

## 术语

| 中文 | 英文 | 含义 | 时间跨度 |
|---|---|---|---|
| 庄 | **Match** | 整场比赛 (东风 / 半庄 / 一庄等). 有跨局累积的状态. | 数十分钟 ~ 1 小时 |
| 局 | **Round** | 一手牌, 配牌 → 和/流 → 结算. 局间隔离. | 数分钟 |
| 操作 | **AtomicOp** | 局内一次不可分的动作. 摸是一个 op, 切是另一个. | 一瞬 |

注: 「巡」(junme, 一摸一切的循环) 不是这里的"操作", 它在原子模型下退化成"两个连续 op"
(Draw + Discard) 加可能的 Pass / 鸣牌. 我们不把"巡"作为一级抽象.

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
    Draw,            // 等 turn 玩家摸
    AwaitDiscard,    // 等 turn 玩家切 (或自摸/立直/暗杠/加杠)
    AwaitCalls,      // 等其他玩家是否鸣 (Pon/Chi/Kan/Ron) 或 Pass
    RoundEnd,        // 局已结束, 不再接受 op
}
```

### Event 输入 (AtomicOp)

```rust
enum AtomicOp {
    // 引擎自动注入 (Draw 阶段无玩家决策)
    Draw,                                          // 从 wall pop 一张到 turn 玩家
    RinshanDraw,                                   // 杠后从岭上摸

    // 玩家决策 (AwaitDiscard 阶段)
    Discard(Tile),
    Riichi(Tile),                                  // 立直 + 摸切宣告牌
    Tsumo,
    Ankan(TileIndex),
    Shouminkan(TileIndex),

    // 玩家决策 (AwaitCalls 阶段)
    Pon  { who: Seat, hand_tile_ids: [u16; 2] },
    Chi  { who: Seat, hand_tile_ids: [u16; 2] },
    Minkan { who: Seat, hand_tile_ids: [u16; 3] },
    Ron  { who: Seat },

    // 跳过整个鸣牌窗口 (没人响应)
    Pass,
}
```

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
            s.last_discard = Some((s.turn, t));
            s.phase = Phase::AwaitCalls;
        }
        Pon { who, hand_tile_ids } => {
            let (from, called) = s.last_discard.expect("AwaitCalls must have last_discard");
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
            // 暗杠后还要摸岭上, 但那个是下一个 op (RinshanDraw), 不在这里做
            s.phase = Phase::Draw;               // 实际是岭上摸: 用 RinshanDraw op
        }

        Riichi(t) => {
            // 立直 = 一个 op 内含 "标记立直" + "切牌". 也可以拆成 RiichiDeclare + Discard 两个 op.
            // (这里放一个 op 是为了让 record/replay 能区分"立直时切"和"立直后正常摸切")
            apply_riichi(&mut s.players[s.turn.idx()]);
            // 然后等同于 Discard(t)
            apply_discard(&mut s, t);
            s.players[s.turn.idx()].riichi_river_idx = Some(s.players[s.turn.idx()].river.len() - 1);
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
            // (上面 Draw op 内会处理 wall 摸尽 → RoundEnd)
        }
    }
    Ok(s)
}
```

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

## 组合 (Driver / Wrapper 层)

driver 不是 pure 的——它要从外部 (UI / AI / 网络) 拉 op, 推给 round_apply, 处理时序:

```rust
fn play_round(init: RoundState, decision_source: impl FnMut(&RoundState) -> AtomicOp) -> RoundState {
    let mut s = init;
    while s.phase != Phase::RoundEnd {
        let op = match s.phase {
            Phase::Draw       => AtomicOp::Draw,           // 自动
            Phase::AwaitDiscard | Phase::AwaitCalls => decision_source(&s),
            Phase::RoundEnd   => unreachable!(),
        };
        s = round_apply(s, op).unwrap();   // 非法 op 在 driver 层应该不出现
    }
    s
}

fn play_match(init: MatchState, mut driver: impl Driver) -> MatchState {
    let mut m = init;
    while !m.ended {
        let r_init  = init_round_from_match(&m);
        let r_final = play_round(r_init, |s| driver.next_op(s));
        m = match_apply(m, summarize_round(&r_final));
    }
    m
}
```

UI 是 driver 实现:
```rust
impl Driver for UiDriver {
    fn next_op(&mut self, s: &RoundState) -> AtomicOp {
        match s.turn_owner() {
            Owner::Local  => self.wait_for_local_input(s),
            Owner::AI(i)  => self.ai[i].decide(s),
            Owner::Remote => self.recv_from_network(s),
        }
    }
}
```

录像 driver 直接从录像 vec 取:
```rust
impl Driver for ReplayDriver {
    fn next_op(&mut self, _: &RoundState) -> AtomicOp {
        self.ops.pop_front().expect("ops 序列耗尽")
    }
}
```

## 一局完整 trace 示例

initial_round (East 庄, dealer 配 13 张, 其它三家 13 张, wall 70 张):

```
phase=Draw, turn=East
ops:
  1.  Draw                                  → East last_drawn=4m, phase=AwaitDiscard
  2.  Discard(9p)                           → 河 [9p], phase=AwaitCalls, last_discard=(East,9p)
  3.  Pass                                  → turn=South, phase=Draw
  4.  Draw                                  → South last_drawn=2s, phase=AwaitDiscard
  5.  Discard(2s)                           → 河 [2s], phase=AwaitCalls
  6.  Pon{who=North,hand=[2s,2s]}           → North 副露 [2s 2s 2s], turn=North, phase=AwaitDiscard
  7.  Discard(W风)                          → 河 [W], phase=AwaitCalls
  8.  Pass                                  → turn=East, phase=Draw
  9.  Draw                                  → East last_drawn=...
  ... (省略 N 步)
 K.   Riichi(8m)                            → East 立直, riichi_river_idx 记入, phase=AwaitCalls
 K+1. Pass
 K+2. Draw                                  → South 摸
 ... 
 N.   Tsumo                                 → 当前 turn 自摸, last_result=Win{...}, phase=RoundEnd
```

`summarize_round` 抽出 `RoundOutcome::Win{winner, score, payments, ...}`,
喂给 `match_apply` 更新庄状态: dealer 是否换 / honba / kyoku / 整庄是否结束.

下一 round_init 由 `init_round_from_match(&match_state)` 给出 (新的 dealer / honba /
立直棒池 / 重新洗 wall). 进入下一轮 fold.

## 待定问题 (defer 到下一轮讨论)

- **输入模型**: round_apply 的签名是 `(state, op) -> state` 还是分解多入参形式? 用户决定后置.
- **State 拆分**: RoundState 单一 struct 还是 type-state 按 phase 拆? (`scratchpad §3.1`)
- **Riichi 是 1 op 还是 2 op**: 当前模型 `Riichi(t)` 一个 op 内含切. 拆成 `RiichiDeclare` + `Discard(t)` 两个连续 op 也合理 (但要保证只能在 RiichiDeclare 后立刻切, 不能插别的). 影响 record 粒度.
- **岭上摸**: 上面写法是 Ankan 后 phase=Draw + 下个 op 是 RinshanDraw. 也可以把岭上摸合并进 Ankan op 的 effect (Ankan 内部直接摸). 影响 op 粒度 + record.
- **Pass 是单一 op 还是按家拆**: 现在是单一 op, 表示"call window 整个关闭". 按家拆会让录像更细但本质上多余 (没有"部分 Pass" 这种状态).
- **错误回退**: round_apply 失败时 state 丢不丢? (`scratchpad §6`)

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
