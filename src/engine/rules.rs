//! GameRules — 游戏规则参数 (开局 freeze, 直到整庄结束).
//!
//! 不是软件级 "config" — 软件级用户偏好 (主题/语言/键位等) 见 [`crate::config`].
//!
//! GameRules 本质是 [`crate::engine::match_state::MatchState`] 的初始化输入数据,
//! 由 RoomActor 在 lobby 阶段持有并允许整体替换 (`ClientMsg::UpdateRules`),
//! 开局后通过 `MatchState::new` 转移所有权进入状态机, 不再热更新.
//!
//! 默认采用 WRC 2022 主基, 古役默认关闭(用户可开启).
//! 详见 docs/spec/README.md

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiRonRule {
    /// 头跳: 仅最近一家可和.
    Atamahane,
    /// 双家荣和.
    DoubleRon,
    /// 三家荣和.
    TripleRon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LengthRule {
    /// 半庄(东+南).
    Hanchan,
    /// 东风战.
    Tonpuusen,
}

/// 一庄游戏规则参数 (房间共享, lobby 由房主控制, InGame 转入 MatchState 后冻结).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRules {
    /// 食断(鸣牌后断幺九成立).
    pub kuitan: bool,
    /// 赤宝牌(各花色五各 1 张赤).
    pub aka_dora: bool,
    /// 一发.
    pub ippatsu: bool,
    /// 里 dora.
    pub ura_dora: bool,
    /// 13+ 番视为役满.
    pub kazoe_yakuman: bool,
    /// 双倍役满(国士13面/纯九莲/四暗刻单骑/大四喜).
    pub double_yakuman: bool,
    /// 多家荣和规则.
    pub multi_ron: MultiRonRule,
    /// 半庄/东风.
    pub length: LengthRule,
    /// 西入(南 4 < 30000 → 进入西场).
    pub west_round: bool,
    /// 击飞(任一家 < 0 强制结束).
    pub minus_score_end: bool,
    /// 古役总开关.
    pub kotekisai: bool,
    /// 古役细分(仅在 kotekisai = true 下生效).
    pub kotekisai_renhou: bool,
    pub kotekisai_sanrenkou: bool,
    pub kotekisai_surenkou: bool,
    pub kotekisai_daisharin: bool,
    pub kotekisai_daichisei: bool,
    pub kotekisai_parenchan: bool,
    pub kotekisai_shisanputaa: bool,
    /// 起始点棒.
    pub starting_score: i32,
    /// 目标点棒(用于 oka).
    pub target_score: i32,
    /// uma (顺位奖罚).
    pub uma: [i32; 4],
    /// 玩家单步思考时长(秒). None = 不限时.
    pub thinking_time_secs: Option<u32>,
    /// 鸣牌响应窗口(秒). 切牌后等他家碰/吃/杠/荣的时间, 超时视为 Pass.
    /// 取值 3-10 秒, 默认 5. 不允许 None (会卡住整桌).
    #[serde(default = "default_call_window")]
    pub call_window_secs: u8,
}

fn default_call_window() -> u8 {
    5
}

impl Default for GameRules {
    fn default() -> Self {
        Self {
            kuitan: true,
            aka_dora: true,
            ippatsu: true,
            ura_dora: true,
            kazoe_yakuman: true,
            double_yakuman: true,
            multi_ron: MultiRonRule::Atamahane,
            length: LengthRule::Hanchan,
            west_round: true,
            minus_score_end: false,
            kotekisai: false,
            kotekisai_renhou: false,
            kotekisai_sanrenkou: false,
            kotekisai_surenkou: false,
            kotekisai_daisharin: false,
            kotekisai_daichisei: false,
            kotekisai_parenchan: false,
            kotekisai_shisanputaa: false,
            starting_score: 25_000,
            target_score: 30_000,
            uma: [15, 5, -5, -15],
            thinking_time_secs: Some(30),
            call_window_secs: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_starting_25000_target_30000() {
        let r = GameRules::default();
        assert_eq!(r.starting_score, 25_000);
        assert_eq!(r.target_score, 30_000);
    }

    #[test]
    fn default_uma_zero_sum() {
        let r = GameRules::default();
        let sum: i32 = r.uma.iter().sum();
        assert_eq!(sum, 0, "uma 应零和");
    }

    #[test]
    fn default_uma_descending() {
        let r = GameRules::default();
        for i in 1..r.uma.len() {
            assert!(
                r.uma[i - 1] >= r.uma[i],
                "uma[{}]={} 应 ≥ uma[{}]={}",
                i - 1,
                r.uma[i - 1],
                i,
                r.uma[i]
            );
        }
    }

    #[test]
    fn default_length_hanchan() {
        assert_eq!(GameRules::default().length, LengthRule::Hanchan);
    }

    #[test]
    fn default_kotekisai_off() {
        let r = GameRules::default();
        assert!(!r.kotekisai);
        assert!(!r.kotekisai_renhou);
        assert!(!r.kotekisai_sanrenkou);
        assert!(!r.kotekisai_daichisei);
    }

    #[test]
    fn default_call_window_in_range() {
        let r = GameRules::default();
        assert!(
            (3..=10).contains(&r.call_window_secs),
            "call_window {} 应 ∈ [3, 10]",
            r.call_window_secs
        );
    }

    #[test]
    fn rules_serde_roundtrip() {
        let r = GameRules::default();
        let s = serde_json::to_string(&r).unwrap();
        let back: GameRules = serde_json::from_str(&s).unwrap();
        assert_eq!(r.starting_score, back.starting_score);
        assert_eq!(r.uma, back.uma);
        assert_eq!(r.length, back.length);
        assert_eq!(r.multi_ron, back.multi_ron);
    }

    #[test]
    fn missing_call_window_uses_default() {
        // 旧 schema 没 call_window_secs 时 #[serde(default)] 给 5
        let json = r#"{
            "kuitan": true, "aka_dora": true, "ippatsu": true, "ura_dora": true,
            "kazoe_yakuman": true, "double_yakuman": true,
            "multi_ron": "Atamahane", "length": "Hanchan",
            "west_round": true, "minus_score_end": false,
            "kotekisai": false,
            "kotekisai_renhou": false, "kotekisai_sanrenkou": false,
            "kotekisai_surenkou": false, "kotekisai_daisharin": false,
            "kotekisai_daichisei": false, "kotekisai_parenchan": false,
            "kotekisai_shisanputaa": false,
            "starting_score": 25000, "target_score": 30000,
            "uma": [15, 5, -5, -15],
            "thinking_time_secs": 30
        }"#;
        let r: GameRules = serde_json::from_str(json).expect("parse legacy schema");
        assert_eq!(r.call_window_secs, 5, "缺字段时应取 default 5");
    }

    #[test]
    fn multi_ron_variants_distinct() {
        assert_ne!(MultiRonRule::Atamahane, MultiRonRule::DoubleRon);
        assert_ne!(MultiRonRule::DoubleRon, MultiRonRule::TripleRon);
    }

    #[test]
    fn length_serde() {
        let s = serde_json::to_string(&LengthRule::Tonpuusen).unwrap();
        assert!(s.contains("Tonpuusen"));
        let back: LengthRule = serde_json::from_str(&s).unwrap();
        assert_eq!(back, LengthRule::Tonpuusen);
    }
}
