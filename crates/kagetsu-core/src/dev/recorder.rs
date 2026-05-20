//! 局录像 + replay (dev-tools feature).
//!
//! 一局一个 [`RoundRecording`]: 含局开始时的 [`GameEngine`] snapshot, 加上
//! 该局送给 [`round_apply`] 的全部 [`AtomicOp`] 序列.
//! [`replay`] 把 recording 顺序应用回去, 复现相同终态.
//!
//! 录像 hook 由 [`GameEngine`] 自身在每个 do_*/declare_*/advance_turn
//! 内部 push (走统一的 apply 包装). UI 层只负责在 round 起始时
//! `recorded_actions = Some(vec![])`, RoundEnd 时 take 出来 + flush 磁盘.
//!
//! [`round_apply`]: crate::engine::round_state::round_apply

use crate::engine::op::AtomicOp;
use crate::engine::round_state::round_apply;
use crate::game_engine::GameEngine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 录像中的单条决策事件. 直接复用 engine 的 [`AtomicOp`] —
/// 录的就是真正送进 round_apply 的算子.
pub type RecordedAction = AtomicOp;

/// 一局完整录像.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundRecording {
    /// 局开始时的 GameEngine snapshot (start_round 末尾: 配牌完成, 待摸).
    pub initial_state: GameEngine,
    /// 该局做的全部决策 (按 round_apply 调用顺序).
    pub actions: Vec<RecordedAction>,
}

/// recordings 目录, 不存在时创建.
pub fn recordings_dir() -> std::io::Result<PathBuf> {
    let mut dir = dirs::config_dir()
        .ok_or_else(|| std::io::Error::other("无可用配置目录 (dirs::config_dir 返回 None)"))?;
    dir.push("kagetsu");
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

/// 用 recording 复现一个局, 返回终态 GameEngine.
///
/// 流程: clone initial_state → 顺序 round_apply 每条 AtomicOp + 累积 emit 的
/// events (与原局 GameEngine.events buffer 行为一致).
/// 不录嵌套 (replayed engine 的 recorded_actions 设为 None).
pub fn replay(rec: &RoundRecording) -> Result<GameEngine, String> {
    let mut e = rec.initial_state.clone();
    e.recorded_actions = None; // 不嵌套
    for op in &rec.actions {
        let (next, evs) = round_apply(&e.round, op.clone())
            .map_err(|err| format!("replay: round_apply {:?} 失败: {:?}", op, err))?;
        e.round = next;
        // 累积 events 到滚动 buffer (跟 GameEngine.apply 行为一致).
        for ev in evs {
            if e.events.len() >= crate::game_engine::MAX_EVENTS {
                e.events.pop_front();
            }
            e.events.push_back(ev);
        }
        if let Some(r) = e.round.result() {
            e.last_result = Some(r.clone());
        }
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::dummy::ai_choose_discard;
    use crate::engine::domain::action::Action;
    use crate::engine::phase::Phase;
    use crate::engine::rules::GameRules;

    /// 跑一局 AI 摸切 (东南西北循环, 不鸣牌), 直到流局或自摸. 录像 + replay 必须终态一致.
    fn play_one_round(seed: u64) -> (GameEngine, RoundRecording) {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(seed);
        let initial_state = e.clone();
        e.recorded_actions = Some(Vec::new());

        loop {
            match e.phase() {
                Phase::Draw => {
                    if e.do_draw().is_none() {
                        break;
                    }
                }
                Phase::AwaitDiscard => match ai_choose_discard(&e.round) {
                    Action::Discard(t) => {
                        e.do_discard(t).unwrap();
                    }
                    Action::Tsumo => {
                        let score = e.try_tsumo().unwrap();
                        e.declare_tsumo(score);
                    }
                    _ => break,
                },
                Phase::AwaitCalls => {
                    // dummy AI 不鸣 (只 ron, 这里跳过 ron 检查直接 advance).
                    e.advance_turn();
                }
                Phase::RoundEnd | Phase::GameEnd | Phase::Deal => break,
            }
        }

        let actions = e.recorded_actions.take().unwrap();
        let rec = RoundRecording {
            initial_state,
            actions,
        };
        (e, rec)
    }

    /// 比较两个 GameEngine 在 serde 视角下相等.
    fn assert_engines_eq(a: &GameEngine, b: &GameEngine, ctx: &str) {
        let sa = serde_json::to_string(a).unwrap();
        let sb = serde_json::to_string(b).unwrap();
        assert_eq!(sa, sb, "{} replay 终态与原局不一致", ctx);
    }

    #[test]
    fn replay_roundtrip_seed_42() {
        let (original, rec) = play_one_round(42);
        let replayed = replay(&rec).unwrap();
        assert_engines_eq(&original, &replayed, "seed=42");
    }

    #[test]
    fn replay_roundtrip_seed_7() {
        let (original, rec) = play_one_round(7);
        let replayed = replay(&rec).unwrap();
        assert_engines_eq(&original, &replayed, "seed=7");
    }

    #[test]
    fn replay_roundtrip_seed_1234567() {
        let (original, rec) = play_one_round(1234567);
        let replayed = replay(&rec).unwrap();
        assert_engines_eq(&original, &replayed, "seed=1234567");
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
