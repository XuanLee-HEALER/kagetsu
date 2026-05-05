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

/// 多家同时荣和的处理规则.
///
/// 当一张弃牌可被多家荣和时按本规则裁定.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiRonRule {
    /// 头跳 (頭ハネ / Atamahane). 仅 *上家最近* 一家可和, 其余不能和.
    /// 默认规则.
    Atamahane,
    /// 双家荣和 (ダブロン / Double Ron). 最多 2 家同时和, 各自独立结算.
    DoubleRon,
    /// 三家荣和 (トリロン / Triple Ron). 3 家同时和, 一些规则下视为流局
    /// (三家和了流局).
    TripleRon,
}

/// 整庄长度规则.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LengthRule {
    /// 半庄 (半荘 / Hanchan). 东 1..东 4 + 南 1..南 4 共 8 局, 是日麻最常见赛制.
    Hanchan,
    /// 东风战 (東風戦 / Tonpuusen). 仅东风圈 4 局, 比赛快.
    Tonpuusen,
}

/// 一庄游戏规则参数 (lobby 由房主控制, 开庄进入 [`MatchState`] 后冻结).
///
/// 不是软件级 `config` (主题 / 语言 / 键位等用户偏好见 [`crate::config`]).
/// `GameRules` 是 *游戏规则* — 各家共享的桌面参数, 影响 evaluate 役 / 计分 /
/// 终局判定等.
///
/// 默认采用 WRC 2022 (世界规则) 主基: 食断开 / 赤宝牌开 / 一发 / 里宝牌 / etc.
/// 古役 (kotekisai_*) 默认全关. 详见 `docs/spec/README.md`.
///
/// [`MatchState`]: crate::engine::match_state::MatchState
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRules {
    /// 食断 (喰い断 / 食い断幺 / Kuitan). 鸣牌后断幺九 (Tanyao) 是否成立.
    /// 默认 true (WRC). 关闭时仅门前清的断幺九算役.
    pub kuitan: bool,
    /// 赤宝牌 (赤ドラ / Aka-Dora). 各花色 5 各 1 张红五, 算 1 番宝牌.
    pub aka_dora: bool,
    /// 一发 (一発 / Ippatsu). 立直后下一巡内和了的 1 番役.
    pub ippatsu: bool,
    /// 里宝牌 (裏ドラ / Ura-Dora). 立直方和了时翻看死墙里宝牌指示, 命中加番.
    pub ura_dora: bool,
    /// 累计役满 (数え役満 / Kazoe Yakuman). 13+ 番视为役满 1 倍.
    pub kazoe_yakuman: bool,
    /// 双倍役满 (W役満 / Double Yakuman). 国士 13 面待 / 纯正九莲宝灯 /
    /// 四暗刻单骑待 / 大四喜 是否给 2 倍役满.
    pub double_yakuman: bool,
    /// 多家荣和规则 (见 [`MultiRonRule`]).
    pub multi_ron: MultiRonRule,
    /// 整庄长度 (半庄 vs 东风, 见 [`LengthRule`]).
    pub length: LengthRule,
    /// 西入 (西入 / 西場 / Saiirin). 半庄南 4 结束时若所有家 < target_score,
    /// 是否进入西场延长.
    pub west_round: bool,
    /// 击飞 (トビ / Tobi / 飛び). 任一家分数 < 0 时是否强制结束整庄.
    pub minus_score_end: bool,
    /// 古役 (古役 / Koteki) 总开关. 默认 false. 开启后下面 kotekisai_* 子项才生效.
    pub kotekisai: bool,
    /// 古役 — 人和 (人和 / Renhou): 子家第一巡内荣和上家弃牌.
    pub kotekisai_renhou: bool,
    /// 古役 — 三连刻 (三連刻 / Sanrenkou): 同花色连续 3 个刻子.
    pub kotekisai_sanrenkou: bool,
    /// 古役 — 四连刻 (四連刻 / Surenkou): 4 个连续同花色刻子 (役满).
    pub kotekisai_surenkou: bool,
    /// 古役 — 大车轮 (大車輪 / Daisharin): 筒子 2-8 各一对.
    pub kotekisai_daisharin: bool,
    /// 古役 — 大七星 (大七星 / Daichisei): 七对子 + 全字牌 (役满).
    pub kotekisai_daichisei: bool,
    /// 古役 — 八连庄 (八連荘 / Parenchan): 庄家连和 8 局 (役满).
    pub kotekisai_parenchan: bool,
    /// 古役 — 十三不塔 (十三不塔 / Shiisanputaa): 配牌即所有牌互不搭 (役满).
    pub kotekisai_shisanputaa: bool,
    /// 起始持点 (各家整庄初始点数). 默认 25000.
    pub starting_score: i32,
    /// 目标点 (オカ用). 顺位结算时持点 - 目标点 = 个人得分.
    /// 默认 30000 (5000 点 oka 由头名独得).
    pub target_score: i32,
    /// 顺位奖罚 (ウマ / Uma). 索引 0..3 对应 1 位..4 位 的额外加减分,
    /// 通常零和. 默认 `[15, 5, -5, -15]` (5-15 制).
    pub uma: [i32; 4],
    /// 玩家单步思考时长 (秒). `None` = 不限时.
    /// 单机模式默认 30 秒, 网络对局通常更短.
    pub thinking_time_secs: Option<u32>,
    /// 鸣牌响应窗口 (秒). 切牌后等他家碰/吃/杠/荣的时间, 超时视为 Pass.
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
