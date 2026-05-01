//! 游戏规则配置.
//!
//! 默认采用 WRC 2022 主基, 古役默认关闭(用户可开启).
//! 详见 docs/spec/README.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiRonRule {
    /// 头跳: 仅最近一家可和.
    Atamahane,
    /// 双家荣和.
    DoubleRon,
    /// 三家荣和.
    TripleRon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthRule {
    /// 半庄(东+南).
    Hanchan,
    /// 东风战.
    Tonpuusen,
}

#[derive(Debug, Clone)]
pub struct GameConfig {
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
}

impl Default for GameConfig {
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
        }
    }
}
