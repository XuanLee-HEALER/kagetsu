//! MpPhase — MpPlayerActor 阶段状态机 (M5.B.3).
//!
//! 每个 phase 决定 actor 接受哪些 cmd / event:
//! - **KeyExchange**: 4 方 keygen + Schnorr broadcast, 等收齐 → aggregate jpk
//! - **Shuffling**: 顺序协议 1 shuffle round, 4 轮完成后 → final_deck
//! - **Playing**: 正式游戏中, 摸牌 / 弃 / 鸣 / 杠 / dora 揭示
//! - **GameOver**: 一局结束 (流局 / 和牌)
//!
//! Transition 严格单向, 不允许回退. 任一 phase 中收到 mismatch cmd 应忽略 +
//! log warn (防 DoS / 协议错乱).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MpPhase {
    /// 初始: 4 玩家广播 KeyShare, 收齐后 aggregate jpk.
    KeyExchange,
    /// jpk 完成后: 顺序 shuffle 4 round, 每轮 verify cnc proof.
    Shuffling,
    /// 协议 1 完成后: 进入正式游戏, 摸 / 弃 / 鸣 / 杠 / dora 揭示.
    Playing,
    /// 一局结束 (流局 / 和牌). 等下一局或解散.
    GameOver,
}

impl MpPhase {
    /// 当前 phase 是否允许 transition 到 next.
    pub fn can_transition_to(self, next: MpPhase) -> bool {
        use MpPhase::*;
        matches!(
            (self, next),
            (KeyExchange, Shuffling)
                | (Shuffling, Playing)
                | (Playing, GameOver)
                | (GameOver, KeyExchange) // 下一局
        )
    }

    /// 是否在游戏中 (Shuffling 或 Playing) — UI 显示状态用.
    pub fn is_in_progress(self) -> bool {
        matches!(self, MpPhase::Shuffling | MpPhase::Playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_transitions_allowed() {
        assert!(MpPhase::KeyExchange.can_transition_to(MpPhase::Shuffling));
        assert!(MpPhase::Shuffling.can_transition_to(MpPhase::Playing));
        assert!(MpPhase::Playing.can_transition_to(MpPhase::GameOver));
        assert!(MpPhase::GameOver.can_transition_to(MpPhase::KeyExchange)); // 下一局
    }

    #[test]
    fn invalid_transitions_rejected() {
        assert!(!MpPhase::KeyExchange.can_transition_to(MpPhase::Playing));
        assert!(!MpPhase::KeyExchange.can_transition_to(MpPhase::GameOver));
        assert!(!MpPhase::Playing.can_transition_to(MpPhase::Shuffling)); // 不可回退
        assert!(!MpPhase::GameOver.can_transition_to(MpPhase::Playing));
    }

    #[test]
    fn no_self_loop_except_via_next_round() {
        for p in [
            MpPhase::KeyExchange,
            MpPhase::Shuffling,
            MpPhase::Playing,
            MpPhase::GameOver,
        ] {
            assert!(!p.can_transition_to(p), "phase {p:?} 不应自循环");
        }
    }

    #[test]
    fn is_in_progress_only_shuffling_and_playing() {
        assert!(!MpPhase::KeyExchange.is_in_progress());
        assert!(MpPhase::Shuffling.is_in_progress());
        assert!(MpPhase::Playing.is_in_progress());
        assert!(!MpPhase::GameOver.is_in_progress());
    }

    #[test]
    fn phase_serde_roundtrip() {
        for p in [
            MpPhase::KeyExchange,
            MpPhase::Shuffling,
            MpPhase::Playing,
            MpPhase::GameOver,
        ] {
            let s = serde_json::to_string(&p).unwrap();
            let back: MpPhase = serde_json::from_str(&s).unwrap();
            assert_eq!(p, back);
        }
    }
}
