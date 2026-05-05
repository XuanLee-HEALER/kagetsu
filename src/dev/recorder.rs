//! 局录像 + replay (dev-tools feature).
//!
//! 一局一个 [`RoundRecording`]: 含局开始时的 [`GameState`] 完整 snapshot,
//! 加上 UI/AI 在该局做的全部决策序列 ([`RecordedAction`]).
//! [`replay`] 把 recording 跑成相同终态的 GameState.
//!
//! Recorder 由 [`crate::engine::state::GameState`] 自身在每个 do_* / declare_*
//! 入口推 action; UI 层 ([`crate::ui::screens::game::GameScreenState`])
//! 负责在 round 起始时 snapshot, RoundEnd 时 flush 到磁盘.

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::TileIndex;
use crate::engine::state::GameState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 单条决策事件. 复现该局唯一所需的最小信息.
///
/// 记录的是 *who* 做了 *什么* 决策, 不是结果事件 (后者用 [`crate::engine::event::GameEvent`]).
/// 所有 tile 用 id 标识, replay 时在当前手牌中按 id 找回.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordedAction {
    /// 当前家 (turn) 弃牌. AwaitDiscard.
    Discard { tile_id: u16 },
    /// 立直 + 弃牌 (engine 内部会调 do_discard, 但 replay 走 do_riichi).
    Riichi { tile_id: u16 },
    /// 自摸. 当前家 turn = winner.
    Tsumo,
    /// 暗杠.
    Ankan { kind: u8 },
    /// 加杠.
    Shouminkan { kind: u8 },
    /// 碰. who = 鸣方, hand_tile_ids = 鸣方手里出的两张.
    Pon { who: Seat, hand_tile_ids: [u16; 2] },
    /// 吃. 同上.
    Chi { who: Seat, hand_tile_ids: [u16; 2] },
    /// 明杠. who = 鸣方, hand_tile_ids = 鸣方手里出的三张.
    Minkan { who: Seat, hand_tile_ids: [u16; 3] },
    /// 荣和. who = 和方, from = 放铳方 (从 last_discard 推出, 但显式记录便于读 log).
    Ron { who: Seat, from: Seat },
    /// 没人鸣 / 都跳过, 进入下一家摸牌. 用作显式 advance 信号.
    Pass,
}

/// 一局完整录像.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundRecording {
    /// 局开始时的 GameState (start_round 末尾的状态: 配牌完成, phase=Draw).
    pub initial_state: GameState,
    /// 该局做的全部决策 (按时间顺序).
    pub actions: Vec<RecordedAction>,
}

/// recordings 目录, 不存在时创建.
pub fn recordings_dir() -> std::io::Result<PathBuf> {
    let mut dir = dirs::config_dir().ok_or_else(|| {
        std::io::Error::other("无可用配置目录 (dirs::config_dir 返回 None)")
    })?;
    dir.push("tui-majo");
    dir.push("recordings");
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// 把 recording 写到 recordings_dir / `<filename>.json`. 返回最终路径.
pub fn save(rec: &RoundRecording, filename: &str) -> std::io::Result<PathBuf> {
    let mut path = recordings_dir()?;
    path.push(format!("{}.json", filename));
    let s = serde_json::to_string_pretty(rec).map_err(std::io::Error::other)?;
    std::fs::write(&path, s)?;
    Ok(path)
}

/// 从 recordings_dir / `<filename>.json` 读 recording.
pub fn load(filename: &str) -> std::io::Result<RoundRecording> {
    let mut path = recordings_dir()?;
    path.push(format!("{}.json", filename));
    let s = std::fs::read_to_string(&path)?;
    serde_json::from_str(&s).map_err(std::io::Error::other)
}

/// 用 recording 复现一个局, 返回终态 GameState.
///
/// 流程: clone initial_state → 顺序 apply 每个 action. 每个 action 之间
/// 状态机会自动 do_draw / advance_turn (与正常游戏一致).
pub fn replay(rec: &RoundRecording) -> Result<GameState, String> {
    // 用 serde roundtrip 复制 initial_state (GameState 没 derive Clone).
    let s = serde_json::to_string(&rec.initial_state).map_err(|e| e.to_string())?;
    let mut g: GameState = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    // 关掉录制, 否则会嵌套.
    g.recorded_actions = None;

    use crate::engine::phase::Phase;
    use crate::engine::state::CallOptions;

    let mut iter = rec.actions.iter();
    loop {
        match g.phase {
            Phase::Draw => {
                if g.do_draw().is_none() {
                    // 牌山摸尽 → 流局, 不继续 apply action.
                    break;
                }
            }
            Phase::AwaitDiscard | Phase::AwaitCalls => {
                let Some(act) = iter.next() else { break };
                apply_action(&mut g, act)?;
            }
            Phase::RoundEnd | Phase::GameEnd | Phase::Deal => break,
        }
    }
    // 静止变量警告.
    let _ = CallOptions::default();
    Ok(g)
}

/// Apply 单条 action 到 g. AwaitDiscard / AwaitCalls 阶段调用.
fn apply_action(g: &mut GameState, act: &RecordedAction) -> Result<(), String> {
    match *act {
        RecordedAction::Discard { tile_id } => {
            let tile = find_tile_in_hand(g, g.turn, tile_id)
                .ok_or_else(|| format!("Discard: tile id {} 不在 turn={:?} 手中", tile_id, g.turn))?;
            g.do_discard(tile).map_err(String::from)?;
        }
        RecordedAction::Riichi { tile_id } => {
            let tile = find_tile_in_hand(g, g.turn, tile_id)
                .ok_or_else(|| format!("Riichi: tile id {} 不在手中", tile_id))?;
            g.do_riichi(tile).map_err(String::from)?;
        }
        RecordedAction::Tsumo => {
            let score = g
                .try_tsumo()
                .ok_or_else(|| "Tsumo: try_tsumo 不成立".to_string())?;
            g.declare_tsumo(score);
        }
        RecordedAction::Ankan { kind } => {
            g.do_ankan(TileIndex(kind)).map_err(String::from)?;
        }
        RecordedAction::Shouminkan { kind } => {
            g.do_shouminkan(TileIndex(kind)).map_err(String::from)?;
        }
        RecordedAction::Pon { who, hand_tile_ids } => {
            let two = find_two_in_hand(g, who, hand_tile_ids)?;
            g.do_pon(who, two).map_err(String::from)?;
        }
        RecordedAction::Chi { who, hand_tile_ids } => {
            let two = find_two_in_hand(g, who, hand_tile_ids)?;
            g.do_chi(who, two).map_err(String::from)?;
        }
        RecordedAction::Minkan { who, hand_tile_ids } => {
            let three = find_three_in_hand(g, who, hand_tile_ids)?;
            g.do_minkan(who, three).map_err(String::from)?;
        }
        RecordedAction::Ron { who, .. } => {
            let score = g
                .try_ron(who)
                .ok_or_else(|| format!("Ron: try_ron({:?}) 不成立", who))?;
            g.declare_ron(who, score);
        }
        RecordedAction::Pass => {
            // 没人鸣, 让状态机 advance. AwaitCalls → 下家 Draw.
            g.advance_turn();
        }
    }
    Ok(())
}

fn find_tile_in_hand(
    g: &GameState,
    seat: Seat,
    tile_id: u16,
) -> Option<crate::engine::domain::tile::Tile> {
    g.players[seat.index()]
        .hand
        .closed
        .iter()
        .find(|t| t.id == tile_id)
        .copied()
}

fn find_two_in_hand(
    g: &GameState,
    seat: Seat,
    ids: [u16; 2],
) -> Result<[crate::engine::domain::tile::Tile; 2], String> {
    let a = find_tile_in_hand(g, seat, ids[0])
        .ok_or_else(|| format!("hand_tile_ids[0]={} 不在 {:?} 手中", ids[0], seat))?;
    let b = find_tile_in_hand(g, seat, ids[1])
        .ok_or_else(|| format!("hand_tile_ids[1]={} 不在 {:?} 手中", ids[1], seat))?;
    Ok([a, b])
}

fn find_three_in_hand(
    g: &GameState,
    seat: Seat,
    ids: [u16; 3],
) -> Result<[crate::engine::domain::tile::Tile; 3], String> {
    let a = find_tile_in_hand(g, seat, ids[0])
        .ok_or_else(|| format!("Minkan id[0]={} 不在手中", ids[0]))?;
    let b = find_tile_in_hand(g, seat, ids[1])
        .ok_or_else(|| format!("Minkan id[1]={} 不在手中", ids[1]))?;
    let c = find_tile_in_hand(g, seat, ids[2])
        .ok_or_else(|| format!("Minkan id[2]={} 不在手中", ids[2]))?;
    Ok([a, b, c])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::dummy::ai_choose_discard;
    use crate::engine::domain::action::Action;
    use crate::engine::phase::Phase;
    use crate::engine::rules::GameRules;

    /// 用 serde 复制 GameState (struct 没 derive Clone 也能用).
    fn clone_via_serde(g: &GameState) -> GameState {
        serde_json::from_str(&serde_json::to_string(g).unwrap()).unwrap()
    }

    /// 比较两个 GameState 在 serde 视角下相等.
    fn assert_states_eq(a: &GameState, b: &GameState, ctx: &str) {
        let sa = serde_json::to_string(a).unwrap();
        let sb = serde_json::to_string(b).unwrap();
        assert_eq!(sa, sb, "{} replay 终态与原局不一致", ctx);
    }

    /// 跑一局 AI 摸切 (东南西北循环, 不鸣牌), 直到流局或自摸. 录像 + replay 必须终态一致.
    fn play_one_round(seed: u64) -> (GameState, RoundRecording) {
        let mut g = GameState::new(GameRules::default());
        g.start_round(seed);
        let initial_state = clone_via_serde(&g);
        g.recorded_actions = Some(Vec::new());

        loop {
            match g.phase {
                Phase::Draw => {
                    if g.do_draw().is_none() {
                        break;
                    }
                }
                Phase::AwaitDiscard => match ai_choose_discard(&g) {
                    Action::Discard(t) => {
                        g.do_discard(t).unwrap();
                    }
                    Action::Tsumo => {
                        let score = g.try_tsumo().unwrap();
                        g.declare_tsumo(score);
                    }
                    _ => break,
                },
                Phase::AwaitCalls => {
                    // AI 不鸣 (dummy AI 只 ron 不鸣牌, 这里没有 ron 检查所以直接 advance).
                    g.advance_turn();
                }
                Phase::RoundEnd | Phase::GameEnd | Phase::Deal => break,
            }
        }

        let actions = g.recorded_actions.take().unwrap();
        let rec = RoundRecording {
            initial_state,
            actions,
        };
        (g, rec)
    }

    #[test]
    fn replay_roundtrip_seed_42() {
        let (original, rec) = play_one_round(42);
        let replayed = replay(&rec).unwrap();
        assert_states_eq(&original, &replayed, "seed=42");
    }

    #[test]
    fn replay_roundtrip_seed_7() {
        let (original, rec) = play_one_round(7);
        let replayed = replay(&rec).unwrap();
        assert_states_eq(&original, &replayed, "seed=7");
    }

    #[test]
    fn replay_roundtrip_seed_1234567() {
        let (original, rec) = play_one_round(1234567);
        let replayed = replay(&rec).unwrap();
        assert_states_eq(&original, &replayed, "seed=1234567");
    }

    #[test]
    fn save_load_recording_roundtrip() {
        let (_, rec) = play_one_round(42);
        let slot = format!("__test_rec_{}", std::process::id());
        let path = save(&rec, &slot).unwrap();
        let loaded = load(&slot).unwrap();
        assert_eq!(rec.actions, loaded.actions);
        let _ = std::fs::remove_file(path);
    }
}
