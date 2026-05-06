//! 役 (役 / Yaku) 种判定.
//!
//! 役是日麻和了的"特殊牌型条件" — 至少 1 役才能和了, 各役有番数 (Han),
//! 多役叠加. 本模块定义 [`Yaku`] enum (~50+ 种) + [`detect_yaku`] 实现.
//!
//! # 役分类
//!
//! - **标准役 1-6 番**: Riichi / Tanyao / Pinfu / Yakuhai / 等. 完整实现.
//! - **役满** (役満 / Yakuman): 国士无双 / 四暗刻 / 大三元 / 等. 完整实现.
//! - **古役** (古役 / Koteki): 大车轮 / 八连庄 / 等. 类型定义完整, 实现按需补,
//!   默认关闭 (见 `rules.kotekisai`).
//!
//! 详见 `docs/spec/yaku.md`.

use crate::engine::domain::decompose::{Decomposition, Mentsu, WaitKind};
use crate::engine::domain::meld::Meld;
use crate::engine::domain::tile::TileIndex;
use crate::engine::rules::GameRules;

/// 役牌 (役牌 / Yakuhai) 类型 — 1 番役, 来源不同.
///
/// 役牌指雀头 / 刻子 = 三元牌 (白发中) 或 当家相关风牌 (场风 / 自风) 时给的役.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum YakuhaiKind {
    /// 白 (白 / Haku) — 三元之一.
    Haku,
    /// 發 (發 / Hatsu) — 三元之一.
    Hatsu,
    /// 中 (中 / Chun) — 三元之一.
    Chun,
    /// 场风 (場風 / Bakaze) — 当前 round_wind 对应的风牌.
    BakaWind,
    /// 自风 (自風 / Jikaze) — 当家相对庄家位置对应的风牌.
    JikaWind,
    /// 连风 (連風 / 双風 / DoubleWind) — 场风 == 自风 时算 2 番 (例: 东 1 局东家的东).
    DoubleWind,
}

/// 役 (Yaku). 大部分 unit variant; 役满中部分携带"特殊条件" (例:
/// 国士 13 面待 / 四暗刻单骑 / 九莲宝灯 9 面待) 用于双倍役满判定.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Yaku {
    // 1 番
    Riichi,
    Ippatsu,
    Tsumo,
    Pinfu,
    Ippeikou,
    Tanyao,
    Yakuhai(YakuhaiKind),
    Haitei,
    Houtei,
    Rinshan,
    Chankan,
    // 2 番
    DoubleRiichi,
    Chiitoitsu,
    Sanshoku,
    Ittsuu,
    Toitoi,
    Sanankou,
    SanshokuDoukou,
    Sankantsu,
    Chanta,
    Honroutou,
    Shousangen,
    // 3 番
    Ryanpeikou,
    Junchan,
    Honitsu,
    // 6 番
    Chinitsu,
    // 满贯特殊
    NagashiMangan,
    // 役满
    Kokushi { thirteen_wait: bool },
    Suuankou { tanki: bool },
    Daisangen,
    Shousuushii,
    Daisuushii,
    Tsuuiisou,
    Ryuuiisou,
    Chinroutou,
    Chuurenpoutou { nine_wait: bool },
    Suukantsu,
    Tenhou,
    Chiihou,
    // 古役
    Renhou,
    Sanrenkou,
    Surenkou,
    Daisharin,
    Daichikurin,
    Daisuurin,
    Daichisei,
    Parenchan,
    Shisanputaa,
    Heiiisou,
    // dora 不是真役但作为附加番展示
    Dora(u32),
    AkaDora(u32),
    UraDora(u32),
}

impl Yaku {
    pub fn is_yakuman(self) -> bool {
        use Yaku::*;
        matches!(
            self,
            Kokushi { .. }
                | Suuankou { .. }
                | Daisangen
                | Shousuushii
                | Daisuushii
                | Tsuuiisou
                | Ryuuiisou
                | Chinroutou
                | Chuurenpoutou { .. }
                | Suukantsu
                | Tenhou
                | Chiihou
                | Renhou // 当配置为役满时
                | Surenkou
                | Daisharin
                | Daichikurin
                | Daisuurin
                | Daichisei
                | Parenchan
                | Shisanputaa
                | Heiiisou
        )
    }

    pub fn name_zh(&self) -> &'static str {
        use Yaku::*;
        match self {
            Riichi => "立直",
            Ippatsu => "一发",
            Tsumo => "门清自摸",
            Pinfu => "平和",
            Ippeikou => "一杯口",
            Tanyao => "断幺九",
            Yakuhai(_) => "役牌",
            Haitei => "海底捞月",
            Houtei => "河底捞鱼",
            Rinshan => "岭上开花",
            Chankan => "枪杠",
            DoubleRiichi => "两立直",
            Chiitoitsu => "七对子",
            Sanshoku => "三色同顺",
            Ittsuu => "一气通贯",
            Toitoi => "对对和",
            Sanankou => "三暗刻",
            SanshokuDoukou => "三色同刻",
            Sankantsu => "三杠子",
            Chanta => "混全带幺九",
            Honroutou => "混老头",
            Shousangen => "小三元",
            Ryanpeikou => "二杯口",
            Junchan => "纯全带幺九",
            Honitsu => "混一色",
            Chinitsu => "清一色",
            NagashiMangan => "流局满贯",
            Kokushi { .. } => "国士无双",
            Suuankou { .. } => "四暗刻",
            Daisangen => "大三元",
            Shousuushii => "小四喜",
            Daisuushii => "大四喜",
            Tsuuiisou => "字一色",
            Ryuuiisou => "绿一色",
            Chinroutou => "清老头",
            Chuurenpoutou { .. } => "九莲宝灯",
            Suukantsu => "四杠子",
            Tenhou => "天和",
            Chiihou => "地和",
            Renhou => "人和",
            Sanrenkou => "三连刻",
            Surenkou => "四连刻",
            Daisharin => "大车轮",
            Daichikurin => "大竹林",
            Daisuurin => "大数邻",
            Daichisei => "大七星",
            Parenchan => "八连庄",
            Shisanputaa => "十三不塔",
            Heiiisou => "黑一色",
            Dora(_) => "宝牌",
            AkaDora(_) => "赤宝牌",
            UraDora(_) => "里宝牌",
        }
    }
}

/// 一次和牌的完整上下文 — [`detect_yaku`] / [`crate::engine::score::evaluate`] 的输入.
///
/// 整合了拆解结果 + 风位 + 各种"特殊和了情境" flags + 宝牌计数. 由 engine 内部
/// 在和了瞬间从 RoundState 抽出后构造, 调用方通常不直接 build.
#[derive(Debug, Clone)]
pub struct WinContext<'a> {
    /// 牌型分解 (含面子 / 雀头 / 待型).
    pub decomposition: &'a Decomposition,
    /// 自风 (自家相对庄家的位置对应的风牌).
    pub seat_wind: TileIndex,
    /// 场风 (整个圈对应的风牌).
    pub round_wind: TileIndex,
    /// 和牌张 kind.
    pub winning_tile: TileIndex,

    /// 是否自摸 (`true`) 或荣和 (`false`).
    pub is_tsumo: bool,
    /// 是否立直状态.
    pub is_riichi: bool,
    /// 是否双立直 (W立直, 第一巡内立直).
    pub is_double_riichi: bool,
    /// 是否一发 (立直后下一巡内未被打断).
    pub is_ippatsu: bool,
    /// 海底捞月 (海底摸月 / Haitei): 自摸最后一张活牌.
    pub is_haitei: bool,
    /// 河底捞鱼 (河底撈魚 / Houtei): 荣和最后一张弃牌.
    pub is_houtei: bool,
    /// 岭上开花 (嶺上開花 / Rinshan): 杠后岭上摸的那张和.
    pub is_rinshan: bool,
    /// 抢杠 (槍槓 / Chankan): 加杠时被截胡荣和.
    pub is_chankan: bool,
    /// 天和 (天和 / Tenhou): 庄家配牌即和 (役满).
    pub is_tenhou: bool,
    /// 地和 (地和 / Chiihou): 子家自家第 1 摸即和 (役满).
    pub is_chiihou: bool,
    /// 人和 (人和 / Renhou): 子家第 1 巡内荣和上家弃牌 (古役).
    pub is_renhou: bool,

    /// 门前清 (Menzen) — 无他人来源副露; 暗杠不算副露破.
    pub menzen: bool,
    /// 完全闭手 (无任何 melds, 含暗杠也算破).
    pub fully_concealed: bool,

    /// 表宝牌 (ドラ / Dora) 命中数.
    pub dora_count: u32,
    /// 赤宝牌 (赤ドラ / Aka-Dora) 命中数.
    pub aka_count: u32,
    /// 里宝牌 (裏ドラ / Ura-Dora) 命中数. 仅立直方有.
    pub ura_dora_count: u32,

    /// 整庄规则参数 (双倍役满 / 古役开关 / etc.).
    pub rules: &'a GameRules,
}

/// 返回所有命中的役及其番数.
/// 役满成立时只返回役满列表(不与一般役混合).
pub fn detect_yaku(ctx: &WinContext, melds: &[Meld]) -> Vec<(Yaku, u32)> {
    // ---------- 役满 ----------
    let mut yakuman: Vec<(Yaku, u32)> = Vec::new();

    if ctx.is_tenhou {
        yakuman.push((Yaku::Tenhou, 13));
    }
    if ctx.is_chiihou {
        yakuman.push((Yaku::Chiihou, 13));
    }
    if ctx.rules.kotekisai && ctx.rules.kotekisai_renhou && ctx.is_renhou {
        yakuman.push((Yaku::Renhou, 13));
    }

    // 国士
    if let Decomposition::Kokushi { thirteen_wait, .. } = ctx.decomposition {
        let mult = if *thirteen_wait && ctx.rules.double_yakuman {
            2
        } else {
            1
        };
        yakuman.push((
            Yaku::Kokushi {
                thirteen_wait: *thirteen_wait,
            },
            13 * mult,
        ));
    }

    if let Decomposition::Standard { .. } = ctx.decomposition {
        if is_suuankou(ctx) {
            let tanki = matches!(
                ctx.decomposition,
                Decomposition::Standard {
                    wait: WaitKind::Tanki,
                    ..
                }
            );
            let mult = if tanki && ctx.rules.double_yakuman {
                2
            } else {
                1
            };
            yakuman.push((Yaku::Suuankou { tanki }, 13 * mult));
        }
        if is_daisangen(ctx, melds) {
            yakuman.push((Yaku::Daisangen, 13));
        }
        let (sho, dai) = sushii_check(ctx, melds);
        if dai {
            let mult = if ctx.rules.double_yakuman { 2 } else { 1 };
            yakuman.push((Yaku::Daisuushii, 13 * mult));
        } else if sho {
            yakuman.push((Yaku::Shousuushii, 13));
        }
        if is_tsuuiisou(ctx, melds) {
            yakuman.push((Yaku::Tsuuiisou, 13));
        }
        if is_chinroutou(ctx, melds) {
            yakuman.push((Yaku::Chinroutou, 13));
        }
        if is_ryuuiisou(ctx, melds) {
            yakuman.push((Yaku::Ryuuiisou, 13));
        }
        if is_suukantsu(ctx, melds) {
            yakuman.push((Yaku::Suukantsu, 13));
        }
        if let Some(nine_wait) = chuurenpoutou_check(ctx) {
            let mult = if nine_wait && ctx.rules.double_yakuman {
                2
            } else {
                1
            };
            yakuman.push((Yaku::Chuurenpoutou { nine_wait }, 13 * mult));
        }
    }

    if !yakuman.is_empty() {
        return yakuman;
    }

    // ---------- 一般役 ----------
    let mut out: Vec<(Yaku, u32)> = Vec::new();

    if ctx.is_double_riichi {
        out.push((Yaku::DoubleRiichi, 2));
    } else if ctx.is_riichi {
        out.push((Yaku::Riichi, 1));
    }
    if ctx.is_ippatsu && (ctx.is_riichi || ctx.is_double_riichi) {
        out.push((Yaku::Ippatsu, 1));
    }
    if ctx.is_tsumo && ctx.menzen {
        out.push((Yaku::Tsumo, 1));
    }
    if ctx.is_haitei {
        out.push((Yaku::Haitei, 1));
    }
    if ctx.is_houtei {
        out.push((Yaku::Houtei, 1));
    }
    if ctx.is_rinshan {
        out.push((Yaku::Rinshan, 1));
    }
    if ctx.is_chankan {
        out.push((Yaku::Chankan, 1));
    }

    // 牌型相关
    if let Decomposition::Chiitoitsu { .. } = ctx.decomposition {
        out.push((Yaku::Chiitoitsu, 2));
    }

    if let Decomposition::Standard { mentsu, pair, .. } = ctx.decomposition {
        // 役牌 (闭手刻子)
        for m in mentsu {
            if let Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) = m {
                let han = yakuhai_for(*t, ctx);
                if han > 0 {
                    out.push((Yaku::Yakuhai(yakuhai_kind(*t, ctx)), han));
                }
            }
        }
        // 役牌 (副露刻子: pon/kan)
        for meld in melds {
            let first_tile = match &meld.kind {
                crate::engine::domain::meld::MeldKind::Pon { tiles } => Some(tiles[0]),
                crate::engine::domain::meld::MeldKind::Minkan { tiles } => Some(tiles[0]),
                crate::engine::domain::meld::MeldKind::Shouminkan { tiles } => Some(tiles[0]),
                crate::engine::domain::meld::MeldKind::Ankan { tiles } => Some(tiles[0]),
                crate::engine::domain::meld::MeldKind::Chi { .. } => None,
            };
            if let Some(t) = first_tile {
                let han = yakuhai_for(t.kind, ctx);
                if han > 0 {
                    out.push((Yaku::Yakuhai(yakuhai_kind(t.kind, ctx)), han));
                }
            }
        }
        // 平和: 任何鸣牌(含 ankan, 因为 kantsu 破坏 pinfu)都不允许.
        if melds.is_empty() && is_pinfu(ctx) {
            out.push((Yaku::Pinfu, 1));
        }
        // 一杯口/二杯口(门清)
        if ctx.menzen {
            let ipp = count_ippeikou(mentsu);
            if ipp == 2 {
                out.push((Yaku::Ryanpeikou, 3));
            } else if ipp == 1 {
                out.push((Yaku::Ippeikou, 1));
            }
        }
        // 三色同顺 / 一气通贯: 含副露 chi 的顺子也参与判定.
        let all_mentsu = mentsu_with_melds(mentsu, melds);
        if has_sanshoku(&all_mentsu) {
            out.push((Yaku::Sanshoku, if ctx.menzen { 2 } else { 1 }));
        }
        if has_ittsuu(&all_mentsu) {
            out.push((Yaku::Ittsuu, if ctx.menzen { 2 } else { 1 }));
        }
        // 对对和: 闭手全刻 + 副露全刻 (chi 视作顺子, 不计)
        let melds_all_koutsu = melds
            .iter()
            .all(|m| !matches!(m.kind, crate::engine::domain::meld::MeldKind::Chi { .. }));
        if mentsu.iter().all(|m| !matches!(m, Mentsu::Shuntsu(_))) && melds_all_koutsu {
            out.push((Yaku::Toitoi, 2));
        }
        // 三暗刻
        if count_concealed_koutsu(ctx) >= 3 {
            out.push((Yaku::Sanankou, 2));
        }
        // 三色同刻: 含副露 pon/kan 的刻子.
        if has_sanshoku_doukou(&all_mentsu) {
            out.push((Yaku::SanshokuDoukou, 2));
        }
        // 三杠子: 闭手 + 副露合并后 Kantsu count == 3 (4 杠是 Suukantsu 役满, 不重叠).
        let kan_total = all_mentsu
            .iter()
            .filter(|m| matches!(m, Mentsu::Kantsu(_, _)))
            .count();
        if kan_total == 3 {
            out.push((Yaku::Sankantsu, 2));
        }
        // 混全/纯全
        let (chanta, junchan) = chanta_check(mentsu, *pair, melds);
        if junchan {
            out.push((Yaku::Junchan, if ctx.menzen { 3 } else { 2 }));
        } else if chanta {
            out.push((Yaku::Chanta, if ctx.menzen { 2 } else { 1 }));
        }
        // 混老头
        if is_honroutou(mentsu, *pair, melds) {
            out.push((Yaku::Honroutou, 2));
        }
        // 小三元
        if is_shousangen(mentsu, *pair) {
            out.push((Yaku::Shousangen, 2));
        }
    }

    // 断幺九
    if is_tanyao(ctx.decomposition, melds) && (ctx.menzen || ctx.rules.kuitan) {
        out.push((Yaku::Tanyao, 1));
    }

    // 清一/混一(适用于标准型和七对子)
    if let Some(suit) = single_suit(ctx.decomposition, melds) {
        if suit.is_some() {
            out.push((Yaku::Chinitsu, if ctx.menzen { 6 } else { 5 }));
        } else {
            out.push((Yaku::Honitsu, if ctx.menzen { 3 } else { 2 }));
        }
    }

    // 去重: 役牌可能多次, 其他互斥的(平和/对对和等结构上不会同时存在).
    // 一杯口与七对子互斥: 七对子结构下 mentsu 为空, 一杯口判定也不会触发.

    // ---------- dora ----------
    if ctx.dora_count > 0 {
        out.push((Yaku::Dora(ctx.dora_count), ctx.dora_count));
    }
    if ctx.aka_count > 0 {
        out.push((Yaku::AkaDora(ctx.aka_count), ctx.aka_count));
    }
    if ctx.ura_dora_count > 0 && (ctx.is_riichi || ctx.is_double_riichi) {
        out.push((Yaku::UraDora(ctx.ura_dora_count), ctx.ura_dora_count));
    }

    out
}

// ============== helper ==============

fn yakuhai_for(t: TileIndex, ctx: &WinContext) -> u32 {
    if t == TileIndex::HAKU || t == TileIndex::HATSU || t == TileIndex::CHUN {
        return 1;
    }
    let mut han = 0;
    if t == ctx.round_wind {
        han += 1;
    }
    if t == ctx.seat_wind {
        han += 1;
    }
    han
}

fn yakuhai_kind(t: TileIndex, ctx: &WinContext) -> YakuhaiKind {
    if t == TileIndex::HAKU {
        return YakuhaiKind::Haku;
    }
    if t == TileIndex::HATSU {
        return YakuhaiKind::Hatsu;
    }
    if t == TileIndex::CHUN {
        return YakuhaiKind::Chun;
    }
    let is_round = t == ctx.round_wind;
    let is_seat = t == ctx.seat_wind;
    match (is_round, is_seat) {
        (true, true) => YakuhaiKind::DoubleWind,
        (true, false) => YakuhaiKind::BakaWind,
        (false, true) => YakuhaiKind::JikaWind,
        _ => YakuhaiKind::Haku, // 不应到达
    }
}

fn count_concealed_koutsu(ctx: &WinContext) -> u32 {
    let Decomposition::Standard {
        mentsu,
        wait,
        winning_tile,
        ..
    } = ctx.decomposition
    else {
        return 0;
    };
    let mut count = 0u32;
    for m in mentsu {
        match m {
            Mentsu::Koutsu(t, true) => {
                // 荣和 + 双碰时, winning_tile 完成的刻子按明刻
                if !ctx.is_tsumo && *wait == WaitKind::Shanpon && *t == *winning_tile {
                    continue;
                }
                count += 1;
            }
            Mentsu::Kantsu(_, true) => count += 1,
            _ => {}
        }
    }
    count
}

fn is_suuankou(ctx: &WinContext) -> bool {
    count_concealed_koutsu(ctx) == 4
}

fn is_daisangen(ctx: &WinContext, melds: &[Meld]) -> bool {
    let Decomposition::Standard { mentsu, .. } = ctx.decomposition else {
        return false;
    };
    let all = mentsu_with_melds(mentsu, melds);
    let dragons = [TileIndex::HAKU, TileIndex::HATSU, TileIndex::CHUN];
    dragons.iter().all(|d| {
        all.iter().any(|m| match m {
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => t == d,
            _ => false,
        })
    })
}

/// 返回 (小四喜, 大四喜).
fn sushii_check(ctx: &WinContext, melds: &[Meld]) -> (bool, bool) {
    let Decomposition::Standard { mentsu, pair, .. } = ctx.decomposition else {
        return (false, false);
    };
    let all = mentsu_with_melds(mentsu, melds);
    let winds = [
        TileIndex::EAST,
        TileIndex::SOUTH,
        TileIndex::WEST,
        TileIndex::NORTH,
    ];
    let mut wind_koutsu = 0;
    let mut pair_is_wind = false;
    for w in winds {
        if all.iter().any(|m| match m {
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => *t == w,
            _ => false,
        }) {
            wind_koutsu += 1;
        }
        if *pair == w {
            pair_is_wind = true;
        }
    }
    (wind_koutsu == 3 && pair_is_wind, wind_koutsu == 4)
}

fn is_tsuuiisou(ctx: &WinContext, melds: &[Meld]) -> bool {
    // 副露含非字牌 → 非字一色
    for m in melds {
        for t in m.tiles() {
            if !t.kind.is_honor() {
                return false;
            }
        }
    }
    let Decomposition::Standard { mentsu, pair, .. } = ctx.decomposition else {
        return false;
    };
    pair.is_honor()
        && mentsu.iter().all(|m| match m {
            Mentsu::Shuntsu(_) => false,
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => t.is_honor(),
        })
}

fn is_chinroutou(ctx: &WinContext, melds: &[Meld]) -> bool {
    let Decomposition::Standard { mentsu, pair, .. } = ctx.decomposition else {
        return false;
    };
    let all = mentsu_with_melds(mentsu, melds);
    pair.is_terminal()
        && all.iter().all(|m| match m {
            Mentsu::Shuntsu(_) => false,
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => t.is_terminal(),
        })
}

fn is_ryuuiisou(ctx: &WinContext, melds: &[Meld]) -> bool {
    let Decomposition::Standard { mentsu, pair, .. } = ctx.decomposition else {
        return false;
    };
    if !pair.is_green() {
        return false;
    }
    let all = mentsu_with_melds(mentsu, melds);
    for m in &all {
        match m {
            Mentsu::Shuntsu(start) => {
                // 只 234s 是绿色顺子.
                if start.0 != 19 {
                    return false;
                }
            }
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => {
                if !t.is_green() {
                    return false;
                }
            }
        }
    }
    true
}

fn is_suukantsu(ctx: &WinContext, melds: &[Meld]) -> bool {
    let Decomposition::Standard { mentsu, .. } = ctx.decomposition else {
        return false;
    };
    let all = mentsu_with_melds(mentsu, melds);
    all.iter()
        .filter(|m| matches!(m, Mentsu::Kantsu(_, _)))
        .count()
        == 4
}

/// 九莲: 同花色 1112345678999 + 任一同花数牌.
/// 返回 Some(nine_wait) 表示成立, nine_wait=true 为纯正九莲.
fn chuurenpoutou_check(ctx: &WinContext) -> Option<bool> {
    if !ctx.menzen {
        return None;
    }
    let Decomposition::Standard {
        mentsu,
        pair,
        winning_tile,
        ..
    } = ctx.decomposition
    else {
        return None;
    };
    if !pair.is_suupai() {
        return None;
    }
    let suit = pair.0 / 9;
    let mut count = [0u8; 9];
    count[(pair.0 % 9) as usize] += 2;
    for m in mentsu {
        match m {
            Mentsu::Shuntsu(start) => {
                if start.0 / 9 != suit {
                    return None;
                }
                let r = (start.0 % 9) as usize;
                count[r] += 1;
                count[r + 1] += 1;
                count[r + 2] += 1;
            }
            Mentsu::Koutsu(t, _) => {
                if !t.is_suupai() || t.0 / 9 != suit {
                    return None;
                }
                count[(t.0 % 9) as usize] += 3;
            }
            Mentsu::Kantsu(_, _) => return None,
        }
    }
    let expected = [3u8, 1, 1, 1, 1, 1, 1, 1, 3];
    let mut found_extra = false;
    for i in 0..9 {
        if count[i] == expected[i] + 1 {
            if found_extra {
                return None;
            }
            found_extra = true;
        } else if count[i] != expected[i] {
            return None;
        }
    }
    if !found_extra {
        return None;
    }
    if !winning_tile.is_suupai() || winning_tile.0 / 9 != suit {
        return None;
    }
    let r = (winning_tile.0 % 9) as usize;
    let mut rest = count;
    rest[r] -= 1;
    let nine_wait = rest == expected;
    Some(nine_wait)
}

fn is_pinfu(ctx: &WinContext) -> bool {
    if !ctx.menzen {
        return false;
    }
    let Decomposition::Standard {
        mentsu, pair, wait, ..
    } = ctx.decomposition
    else {
        return false;
    };
    if *wait != WaitKind::Ryanmen {
        return false;
    }
    if mentsu.iter().any(|m| !matches!(m, Mentsu::Shuntsu(_))) {
        return false;
    }
    // 雀头不可为役牌(三元/场风/自风).
    if pair.is_dragon() || *pair == ctx.round_wind || *pair == ctx.seat_wind {
        return false;
    }
    true
}

fn count_ippeikou(mentsu: &[Mentsu]) -> u32 {
    let mut shuntsus: Vec<TileIndex> = mentsu
        .iter()
        .filter_map(|m| match m {
            Mentsu::Shuntsu(s) => Some(*s),
            _ => None,
        })
        .collect();
    shuntsus.sort();
    let mut pairs = 0u32;
    let mut i = 0;
    while i + 1 < shuntsus.len() {
        if shuntsus[i] == shuntsus[i + 1] {
            pairs += 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    pairs
}

fn has_sanshoku(mentsu: &[Mentsu]) -> bool {
    let mut have = [[false; 9]; 3];
    for m in mentsu {
        if let Mentsu::Shuntsu(s) = m {
            let suit = (s.0 / 9) as usize;
            let rank = (s.0 % 9) as usize;
            if suit < 3 {
                have[suit][rank] = true;
            }
        }
    }
    (0..7).any(|r| have[0][r] && have[1][r] && have[2][r])
}

fn has_sanshoku_doukou(mentsu: &[Mentsu]) -> bool {
    let mut have = [[false; 9]; 3];
    for m in mentsu {
        if let Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) = m {
            if !t.is_suupai() {
                continue;
            }
            let suit = (t.0 / 9) as usize;
            let rank = (t.0 % 9) as usize;
            have[suit][rank] = true;
        }
    }
    (0..9).any(|r| have[0][r] && have[1][r] && have[2][r])
}

/// 把 closed mentsu 与 副露 melds 合并成统一 Mentsu 视图.
/// chi → Shuntsu, pon → Koutsu(open), minkan/shouminkan → Kantsu(open), ankan → Kantsu(closed).
fn mentsu_with_melds(closed: &[Mentsu], melds: &[Meld]) -> Vec<Mentsu> {
    let mut out: Vec<Mentsu> = closed.to_vec();
    for m in melds {
        match &m.kind {
            crate::engine::domain::meld::MeldKind::Chi { tiles, .. } => {
                let mut kinds = [tiles[0].kind.0, tiles[1].kind.0, tiles[2].kind.0];
                kinds.sort();
                out.push(Mentsu::Shuntsu(TileIndex(kinds[0])));
            }
            crate::engine::domain::meld::MeldKind::Pon { tiles } => {
                out.push(Mentsu::Koutsu(tiles[0].kind, false));
            }
            crate::engine::domain::meld::MeldKind::Minkan { tiles }
            | crate::engine::domain::meld::MeldKind::Shouminkan { tiles } => {
                out.push(Mentsu::Kantsu(tiles[0].kind, false));
            }
            crate::engine::domain::meld::MeldKind::Ankan { tiles } => {
                out.push(Mentsu::Kantsu(tiles[0].kind, true));
            }
        }
    }
    out
}

fn has_ittsuu(mentsu: &[Mentsu]) -> bool {
    for suit in 0..3u8 {
        let base = suit * 9;
        let need = [base, base + 3, base + 6];
        if need.iter().all(|&n| {
            mentsu
                .iter()
                .any(|m| matches!(m, Mentsu::Shuntsu(s) if s.0 == n))
        }) {
            return true;
        }
    }
    false
}

/// 返回 (chanta, junchan). 含副露分析.
fn chanta_check(mentsu: &[Mentsu], pair: TileIndex, melds: &[Meld]) -> (bool, bool) {
    let mut has_shuntsu = false;
    let mut has_honor = false;
    let mut all_yaochuu = true;
    if pair.is_honor() {
        has_honor = true;
    }
    if !pair.is_yaochuu() {
        all_yaochuu = false;
    }
    for m in mentsu {
        match m {
            Mentsu::Shuntsu(s) => {
                has_shuntsu = true;
                let r = s.0 % 9;
                // 顺子起始为 1 (123) 或 7 (789) 才包含 1 或 9.
                if r != 0 && r != 6 {
                    all_yaochuu = false;
                }
            }
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => {
                if t.is_honor() {
                    has_honor = true;
                }
                if !t.is_yaochuu() {
                    all_yaochuu = false;
                }
            }
        }
    }
    // 副露分析: chi 算 shuntsu (取最小 kind 判起始), pon/kan 算 koutsu/kantsu
    for meld in melds {
        match &meld.kind {
            crate::engine::domain::meld::MeldKind::Chi { .. } => {
                has_shuntsu = true;
                let min_kind = meld.tiles().iter().map(|t| t.kind.0).min().unwrap();
                let r = min_kind % 9;
                if r != 0 && r != 6 {
                    all_yaochuu = false;
                }
            }
            _ => {
                let first_kind = meld.tiles()[0].kind;
                if first_kind.is_honor() {
                    has_honor = true;
                }
                if !first_kind.is_yaochuu() {
                    all_yaochuu = false;
                }
            }
        }
    }
    if !all_yaochuu {
        return (false, false);
    }
    let junchan = !has_honor && has_shuntsu;
    let chanta = has_honor && has_shuntsu;
    (chanta, junchan)
}

fn is_honroutou(mentsu: &[Mentsu], pair: TileIndex, melds: &[Meld]) -> bool {
    if !pair.is_yaochuu() {
        return false;
    }
    for m in mentsu {
        match m {
            Mentsu::Shuntsu(_) => return false,
            Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => {
                if !t.is_yaochuu() {
                    return false;
                }
            }
        }
    }
    // 副露含 chi → 含顺子 → 非 honroutou; 副露含非 yaochuu → 非 honroutou
    for meld in melds {
        if matches!(meld.kind, crate::engine::domain::meld::MeldKind::Chi { .. }) {
            return false;
        }
        if !meld.tiles()[0].kind.is_yaochuu() {
            return false;
        }
    }
    true
}

fn is_shousangen(mentsu: &[Mentsu], pair: TileIndex) -> bool {
    let dragons = [TileIndex::HAKU, TileIndex::HATSU, TileIndex::CHUN];
    let mut k_count = 0;
    let mut pair_dragon = None;
    if dragons.contains(&pair) {
        pair_dragon = Some(pair);
    }
    for m in mentsu {
        if let Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) = m
            && dragons.contains(t)
        {
            k_count += 1;
        }
    }
    k_count == 2 && pair_dragon.is_some()
}

fn is_tanyao(d: &Decomposition, melds: &[Meld]) -> bool {
    // 任何副露含 yaochuu 牌 → 非 tanyao
    for m in melds {
        for t in m.tiles() {
            if t.kind.is_yaochuu() {
                return false;
            }
        }
    }
    match d {
        Decomposition::Standard { mentsu, pair, .. } => {
            if pair.is_yaochuu() {
                return false;
            }
            for m in mentsu {
                match m {
                    Mentsu::Shuntsu(s) => {
                        let r = s.0 % 9;
                        // 顺子起始 r ∈ [1, 5] 才不含 1/9 (即 234..678).
                        // r=0 起始 1 含 1; r=6,7,8 起始 7/8/9 (8 9 不存在), r=6 含 9.
                        if !(1..=5).contains(&r) {
                            return false;
                        }
                    }
                    Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => {
                        if t.is_yaochuu() {
                            return false;
                        }
                    }
                }
            }
            true
        }
        Decomposition::Chiitoitsu { pairs, .. } => pairs.iter().all(|t| !t.is_yaochuu()),
        Decomposition::Kokushi { .. } => false,
    }
}

/// 返回 Some(Some(()))=清一; Some(None)=混一; None=既非.
/// 必须检查闭手分解 + 所有副露 (鸣牌牌也属于一色).
fn single_suit(d: &Decomposition, melds: &[Meld]) -> Option<Option<()>> {
    let mut suits = [false; 3]; // m/p/s
    let mut has_honor = false;
    let mut record = |t: TileIndex| {
        if t.is_honor() {
            has_honor = true;
        } else {
            suits[(t.0 / 9) as usize] = true;
        }
    };
    match d {
        Decomposition::Standard { mentsu, pair, .. } => {
            record(*pair);
            for m in mentsu {
                match m {
                    Mentsu::Shuntsu(s) => record(*s),
                    Mentsu::Koutsu(t, _) | Mentsu::Kantsu(t, _) => record(*t),
                }
            }
        }
        Decomposition::Chiitoitsu { pairs, .. } => {
            for t in pairs {
                record(*t);
            }
        }
        Decomposition::Kokushi { .. } => return None,
    }
    // 副露中的牌也参与
    for m in melds {
        for t in m.tiles() {
            record(t.kind);
        }
    }
    let suit_count = suits.iter().filter(|&&b| b).count();
    match (suit_count, has_honor) {
        (1, false) => Some(Some(())),
        (1, true) => Some(None),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::decompose::decompose;
    use crate::engine::rules::GameRules;

    fn ctx_for(d: &Decomposition, menzen: bool) -> WinContext<'_> {
        // 预先构造一个临时上下文.config 必须是引用,但我们这里返回 owned,
        // 用 Box::leak 简化测试.
        let cfg: &'static GameRules = Box::leak(Box::new(GameRules::default()));
        WinContext {
            decomposition: d,
            seat_wind: TileIndex::EAST,
            round_wind: TileIndex::EAST,
            winning_tile: match d {
                Decomposition::Standard { winning_tile, .. } => *winning_tile,
                Decomposition::Chiitoitsu { winning_tile, .. } => *winning_tile,
                Decomposition::Kokushi { winning_tile, .. } => *winning_tile,
            },
            is_tsumo: false,
            is_riichi: false,
            is_double_riichi: false,
            is_ippatsu: false,
            is_haitei: false,
            is_houtei: false,
            is_rinshan: false,
            is_chankan: false,
            is_tenhou: false,
            is_chiihou: false,
            is_renhou: false,
            menzen,
            fully_concealed: menzen,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            rules: cfg,
        }
    }

    #[test]
    fn detect_chiitoitsu() {
        let mut hand = [0u8; 34];
        for &k in &[0u8, 2, 4, 6, 9, 11, 33] {
            hand[k as usize] = 2;
        }
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r
            .iter()
            .find(|d| matches!(d, Decomposition::Chiitoitsu { .. }))
            .unwrap();
        let ctx = ctx_for(d, true);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Chiitoitsu)));
    }

    #[test]
    fn detect_pinfu_basic() {
        // 234m 234p 234s 567m 11s, 等 5m (kanchan) → 不是 pinfu
        // 让 ryanmen: 234m 234p 234s 67m + 22s, 等 5m or 8m → ryanmen
        // 234m + 234p + 234s + 67m + winning 5m (ryanmen) + 22s 雀头
        let mut hand = [0u8; 34];
        // 234m
        hand[1] = 1;
        hand[2] = 1;
        hand[3] = 1;
        // 234p
        hand[10] = 1;
        hand[11] = 1;
        hand[12] = 1;
        // 234s
        hand[19] = 1;
        hand[20] = 1;
        hand[21] = 1;
        // 67m + winning 5m → 567m (ryanmen)
        hand[4] = 1; // 5m
        hand[5] = 1; // 6m
        hand[6] = 1; // 7m
        // 雀头 8s 8s
        hand[25] = 2;
        let r = decompose(&hand, &[], TileIndex(4)); // winning 5m
        let d = r
            .iter()
            .find(|d| {
                matches!(
                    d,
                    Decomposition::Standard {
                        wait: WaitKind::Ryanmen,
                        ..
                    }
                )
            })
            .unwrap();
        let ctx = ctx_for(d, true);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Pinfu)),
            "应识别 pinfu, got {:?}",
            yakus
        );
    }

    #[test]
    fn detect_tanyao() {
        // 234m 567p 234s 666m 55s, 等 ... 全是 2-8.
        let mut hand = [0u8; 34];
        hand[1] = 1;
        hand[2] = 1;
        hand[3] = 1; // 234m
        hand[13] = 1;
        hand[14] = 1;
        hand[15] = 1; // 567p
        hand[19] = 1;
        hand[20] = 1;
        hand[21] = 1; // 234s
        hand[5] = 3; // 666m
        hand[22] = 2; // 55s
        let r = decompose(&hand, &[], TileIndex(1));
        let d = r
            .iter()
            .find(|d| matches!(d, Decomposition::Standard { .. }))
            .unwrap();
        let ctx = ctx_for(d, true);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Tanyao)));
    }

    #[test]
    fn detect_kokushi() {
        let mut hand = [0u8; 34];
        for &k in &[0u8, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33] {
            hand[k as usize] = 1;
        }
        hand[0] = 2;
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r
            .iter()
            .find(|d| matches!(d, Decomposition::Kokushi { .. }))
            .unwrap();
        let ctx = ctx_for(d, true);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Kokushi { .. })));
    }

    // ===== 各番数等级 + 役满代表测试 =====
    //
    // 按"等价类"覆盖: 每个 yaku 检测路径选一个最简代表牌型. 不穷举牌种.

    fn h(spec: &[(u8, u8)]) -> [u8; 34] {
        let mut a = [0u8; 34];
        for &(k, c) in spec {
            a[k as usize] = c;
        }
        a
    }

    /// 修改 ctx 给指定 winning_tile 的标准 14 张型 (含 winning), menzen + 可选 tsumo.
    fn std_ctx<'a>(
        d: &'a Decomposition,
        menzen: bool,
        is_tsumo: bool,
        is_riichi: bool,
        is_ippatsu: bool,
    ) -> WinContext<'a> {
        let cfg: &'static GameRules = Box::leak(Box::new(GameRules::default()));
        WinContext {
            decomposition: d,
            seat_wind: TileIndex::EAST,
            round_wind: TileIndex::EAST,
            winning_tile: match d {
                Decomposition::Standard { winning_tile, .. } => *winning_tile,
                Decomposition::Chiitoitsu { winning_tile, .. } => *winning_tile,
                Decomposition::Kokushi { winning_tile, .. } => *winning_tile,
            },
            is_tsumo,
            is_riichi,
            is_double_riichi: false,
            is_ippatsu,
            is_haitei: false,
            is_houtei: false,
            is_rinshan: false,
            is_chankan: false,
            is_tenhou: false,
            is_chiihou: false,
            is_renhou: false,
            menzen,
            fully_concealed: menzen,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            rules: cfg,
        }
    }

    // ---- 1 番 ----

    #[test]
    fn detect_riichi_tsumo_pinfu_combo() {
        // menzen + riichi + tsumo + ryanmen pinfu 牌, 应同时出 Riichi/Tsumo/Pinfu.
        let hand = h(&[
            (1, 1),
            (2, 1),
            (3, 1), // 234m
            (4, 1),
            (5, 1),
            (6, 1), // 567m (ryanmen wait via winning=4m)
            (10, 1),
            (11, 1),
            (12, 1), // 234p
            (19, 1),
            (20, 1),
            (21, 1), // 234s
            (8, 2),  // 99m 雀头 (非役牌)
        ]);
        let r = decompose(&hand, &[], TileIndex(3));
        let d = r
            .iter()
            .find(|d| matches!(d, Decomposition::Standard { wait: WaitKind::Ryanmen, .. }))
            .expect("应有 ryanmen 拆解");
        let ctx = std_ctx(d, true, true, true, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Riichi)));
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Tsumo)));
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Pinfu)));
    }

    #[test]
    fn detect_yakuhai_haku() {
        // 雀头不能算 yakuhai, 必须是刻子. 14 张: 234m+234p+234s+白白白+99m.
        let hand = h(&[
            (1, 1),
            (2, 1),
            (3, 1),
            (10, 1),
            (11, 1),
            (12, 1),
            (19, 1),
            (20, 1),
            (21, 1),
            (31, 3), // 白×3 (yakuhai 三元)
            (8, 2),  // 99m 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(31));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Yakuhai(_))),
            "白刻应识别 yakuhai, got {:?}",
            yakus
        );
    }

    // ---- 2 番 ----

    #[test]
    fn detect_ittsuu() {
        // 一气通贯 = 同色 1-9 三个顺子.
        // 14 张: 123m + 456m + 789m + 234p + 99p 雀头.
        let hand = h(&[
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 1),
            (5, 1),
            (6, 1),
            (7, 1),
            (8, 1), // 1-9m
            (10, 1),
            (11, 1),
            (12, 1), // 234p
            (17, 2), // 99p 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r
            .iter()
            .find(|d| matches!(d, Decomposition::Standard { .. }))
            .expect("应有标准拆解");
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Ittsuu)),
            "1-9m 一气应识别 Ittsuu, got {:?}",
            yakus
        );
    }

    #[test]
    fn detect_sanshoku_doujun() {
        // 三色同顺 234m+234p+234s.
        let hand = h(&[
            (1, 1),
            (2, 1),
            (3, 1), // 234m
            (10, 1),
            (11, 1),
            (12, 1), // 234p
            (19, 1),
            (20, 1),
            (21, 1), // 234s
            (4, 1),
            (5, 1),
            (6, 1), // 567m 第 4 顺
            (8, 2), // 99m 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(1));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Sanshoku)),
            "234 三色应识别, got {:?}",
            yakus
        );
    }

    #[test]
    fn detect_toitoi() {
        // 对对和 = 4 刻 + 雀头.
        // 14 张: 111m + 333p + 555s + 777m + 99m.
        // 但 7m 跟 99m 不冲突, 1m/3p/5s/7m 各 3, 9m 2. 总 12+2=14 ✓
        let hand = h(&[
            (0, 3),  // 111m
            (11, 3), // 333p
            (22, 3), // 555s -- wait, 5s = TileIndex(22), correct
            (6, 3),  // 777m
            (8, 2),  // 99m 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Toitoi)),
            "4 刻应识别 Toitoi, got {:?}",
            yakus
        );
    }

    // ---- 3 番 ----

    #[test]
    fn detect_honitsu() {
        // 混一色 = 单色 + 字牌. 14 张: 111m + 234m + 567m + 中中中 + 99m.
        let hand = h(&[
            (0, 3),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 1),
            (5, 1),
            (6, 1),
            (8, 2),
            (33, 3),
        ]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Honitsu)),
            "m + 字 应识别 Honitsu, got {:?}",
            yakus
        );
    }

    // ---- 6 番 ----

    #[test]
    fn detect_chinitsu() {
        // 清一色 = 单色无字. 14 张全 m: 222m+333m+444m+555m+99m (4 刻 + 雀头).
        let hand = h(&[(1, 3), (2, 3), (3, 3), (4, 3), (8, 2)]);
        let r = decompose(&hand, &[], TileIndex(1));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Chinitsu)),
            "全 m 应识别 Chinitsu, got {:?}",
            yakus
        );
    }

    // ---- 役满 ----

    #[test]
    fn detect_suuankou_yakuman() {
        // 四暗刻 = 4 暗刻 + 雀头, 必须 menzen + 4 koutsu 全 concealed.
        // 14 张: 111m + 222p + 333s + 444m + 99m.
        let hand = h(&[
            (0, 3),
            (10, 3),
            (20, 3),
            (3, 3),
            (8, 2),
        ]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, true, false, false); // tsumo 让所有刻 concealed
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Suuankou { .. })),
            "4 暗刻应识别 Suuankou, got {:?}",
            yakus
        );
    }

    #[test]
    fn detect_daisangen_yakuman() {
        // 大三元 = 白刻 + 发刻 + 中刻 + 任意面子 + 任意雀头.
        // 14 张: 白白白 + 发发发 + 中中中 + 234m + 99m.
        let hand = h(&[
            (1, 1),
            (2, 1),
            (3, 1), // 234m
            (8, 2),  // 99m
            (31, 3), // 白
            (32, 3), // 发
            (33, 3), // 中
        ]);
        let r = decompose(&hand, &[], TileIndex(31));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Daisangen)),
            "三元各刻应识别 Daisangen, got {:?}",
            yakus
        );
    }

    // ===== 残余覆盖率补充 =====

    #[test]
    fn name_zh_returns_for_all_variants() {
        // 一次性遍历所有 Yaku variant, 验 name_zh 不 panic 且非空.
        // 覆盖 yaku.rs:134-189 的 51 个分支.
        use Yaku::*;
        let all: Vec<Yaku> = vec![
            Riichi, Ippatsu, Tsumo, Pinfu, Ippeikou, Tanyao,
            Yakuhai(YakuhaiKind::Haku), Haitei, Houtei, Rinshan, Chankan,
            DoubleRiichi, Chiitoitsu, Sanshoku, Ittsuu, Toitoi,
            Sanankou, SanshokuDoukou, Sankantsu, Chanta, Honroutou,
            Shousangen, Ryanpeikou, Junchan, Honitsu, Chinitsu,
            NagashiMangan,
            Kokushi { thirteen_wait: false }, Kokushi { thirteen_wait: true },
            Suuankou { tanki: false }, Suuankou { tanki: true },
            Daisangen, Shousuushii, Daisuushii, Tsuuiisou, Ryuuiisou, Chinroutou,
            Chuurenpoutou { nine_wait: false }, Chuurenpoutou { nine_wait: true },
            Suukantsu, Tenhou, Chiihou, Renhou,
            Sanrenkou, Surenkou, Daisharin, Daichikurin, Daisuurin,
            Daichisei, Parenchan, Shisanputaa, Heiiisou,
            Dora(1), AkaDora(1), UraDora(1),
        ];
        for y in &all {
            let name = y.name_zh();
            assert!(!name.is_empty(), "{:?} name_zh 不应空", y);
        }
    }

    #[test]
    fn detect_double_riichi_and_haitei_and_houtei() {
        // double_riichi / haitei / houtei 由 ctx flag 触发, 跟牌型组合.
        // 用 chiitoitsu 作底牌型 (确保有役).
        let hand = h(&[(0, 2), (2, 2), (4, 2), (6, 2), (9, 2), (33, 2), (29, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Chiitoitsu { .. })).unwrap();

        let cfg: &'static GameRules = Box::leak(Box::new(GameRules::default()));
        let mut ctx = WinContext {
            decomposition: d,
            seat_wind: TileIndex::EAST,
            round_wind: TileIndex::EAST,
            winning_tile: TileIndex(0),
            is_tsumo: true,
            is_riichi: false,
            is_double_riichi: true,
            is_ippatsu: true,
            is_haitei: true,
            is_houtei: false,
            is_rinshan: false,
            is_chankan: false,
            is_tenhou: false,
            is_chiihou: false,
            is_renhou: false,
            menzen: true,
            fully_concealed: true,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            rules: cfg,
        };
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::DoubleRiichi)));
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Ippatsu)));
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Haitei)));

        // 改 houtei: ron 终牌.
        ctx.is_tsumo = false;
        ctx.is_haitei = false;
        ctx.is_houtei = true;
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Houtei)));
    }

    #[test]
    fn detect_rinshan_and_chankan_and_tenhou_chiihou() {
        // 各 yakuman / 1番役由 ctx flag 触发. 用任意有效牌型即可.
        let hand = h(&[(0, 2), (2, 2), (4, 2), (6, 2), (9, 2), (33, 2), (29, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Chiitoitsu { .. })).unwrap();

        let cfg: &'static GameRules = Box::leak(Box::new(GameRules::default()));
        let mut ctx = WinContext {
            decomposition: d,
            seat_wind: TileIndex::EAST,
            round_wind: TileIndex::EAST,
            winning_tile: TileIndex(0),
            is_tsumo: true,
            is_riichi: false,
            is_double_riichi: false,
            is_ippatsu: false,
            is_haitei: false,
            is_houtei: false,
            is_rinshan: true,
            is_chankan: false,
            is_tenhou: false,
            is_chiihou: false,
            is_renhou: false,
            menzen: true,
            fully_concealed: true,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            rules: cfg,
        };
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Rinshan)));

        ctx.is_rinshan = false;
        ctx.is_tsumo = false;
        ctx.is_chankan = true;
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Chankan)));

        // 天和 / 地和 — 役满 (覆盖 yakuman 路径).
        ctx.is_chankan = false;
        ctx.is_tsumo = true;
        ctx.is_tenhou = true;
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Tenhou)));

        ctx.is_tenhou = false;
        ctx.is_chiihou = true;
        let yakus = detect_yaku(&ctx, &[]);
        assert!(yakus.iter().any(|(y, _)| matches!(y, Yaku::Chiihou)));
    }

    #[test]
    fn detect_tsuuiisou_yakuman() {
        // 字一色 = 全字牌. 14 张: 白×3 + 发×3 + 中×3 + 东×3 + 南×2.
        let hand = h(&[(31, 3), (32, 3), (33, 3), (27, 3), (28, 2)]);
        let r = decompose(&hand, &[], TileIndex(31));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Tsuuiisou)),
            "全字牌应识别 Tsuuiisou, got {:?}", yakus
        );
    }

    #[test]
    fn detect_chinroutou_yakuman() {
        // 清老头 = 全 1/9 数牌. 14 张: 1m×3 + 9m×3 + 1p×3 + 9p×3 + 1s×2.
        let hand = h(&[(0, 3), (8, 3), (9, 3), (17, 3), (18, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Chinroutou)),
            "全幺九数牌应识别 Chinroutou, got {:?}", yakus
        );
    }

    #[test]
    fn detect_daisuushii_yakuman() {
        // 大四喜 = 4 风刻 + 任意雀头.
        // 14 张: 东×3 + 南×3 + 西×3 + 北×3 + 1m×2.
        let hand = h(&[(27, 3), (28, 3), (29, 3), (30, 3), (0, 2)]);
        let r = decompose(&hand, &[], TileIndex(27));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Daisuushii)),
            "4 风刻应识别 Daisuushii, got {:?}", yakus
        );
    }

    #[test]
    fn detect_shousuushii_yakuman() {
        // 小四喜 = 3 风刻 + 1 风雀头 + 任意面子.
        // 14 张: 东×3 + 南×3 + 西×3 + 北×2 + 234m.
        let hand = h(&[
            (27, 3), (28, 3), (29, 3), (30, 2),
            (1, 1), (2, 1), (3, 1),
        ]);
        let r = decompose(&hand, &[], TileIndex(27));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Shousuushii)),
            "3 风刻+1 风雀头应识别 Shousuushii, got {:?}", yakus
        );
    }

    #[test]
    fn detect_chanta_with_yaochuu_in_each_mentsu() {
        // 混全带幺九 = 每面子+雀头都含 1/9/字.
        // 14 张: 123m + 789p + 111s + 中中中 + 99m.
        let hand = h(&[
            (0, 1), (1, 1), (2, 1), // 123m (含 1m)
            (15, 1), (16, 1), (17, 1), // 789p (含 9p)
            (18, 3), // 111s
            (33, 3), // 中中中
            (8, 2), // 99m 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Chanta)),
            "Chanta 应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_junchan_only_terminals() {
        // 纯全带幺九 = 每面子+雀头都含 1/9, 不含字牌.
        // 14 张: 123m + 789m + 123p + 789s + 99p.
        let hand = h(&[
            (0, 1), (1, 1), (2, 1),    // 123m
            (6, 1), (7, 1), (8, 1),    // 789m
            (9, 1), (10, 1), (11, 1),  // 123p
            (24, 1), (25, 1), (26, 1), // 789s
            (17, 2),                    // 99p 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Junchan)),
            "Junchan 应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_honroutou() {
        // 混老头 = 全 1/9 + 字, 4 刻 + 雀头. 跟 Toitoi 共存.
        // 14 张: 1m×3 + 9p×3 + 中×3 + 白×3 + 9m×2.
        let hand = h(&[(0, 3), (17, 3), (33, 3), (31, 3), (8, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Honroutou)),
            "Honroutou 应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_ippeikou_two_identical_shuntsu() {
        // 一杯口 = 同色 2 个相同顺子 (门清).
        // 14 张: 234m + 234m + 567p + 789s + 99m.
        let hand = h(&[
            (1, 2), (2, 2), (3, 2), // 234m × 2
            (13, 1), (14, 1), (15, 1), // 567p
            (24, 1), (25, 1), (26, 1), // 789s
            (8, 2), // 99m 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(1));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Ippeikou)),
            "Ippeikou 应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_shousangen() {
        // 小三元 = 2 个三元刻 + 1 个三元雀头.
        // 14 张: 白×3 + 发×3 + 中×2 + 234m + 234p.
        let hand = h(&[
            (31, 3), (32, 3), (33, 2),
            (1, 1), (2, 1), (3, 1),
            (10, 1), (11, 1), (12, 1),
        ]);
        let r = decompose(&hand, &[], TileIndex(33));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Shousangen)),
            "Shousangen 应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_sanankou_three_concealed_triplets() {
        // 三暗刻 = 3 个暗刻 + 1 顺子或刻 + 雀头. tsumo 让所有 koutsu 暗.
        // 14 张: 111m + 333p + 555s + 234m + 99m.
        let hand = h(&[(0, 3), (11, 3), (22, 3), (1, 1), (2, 1), (3, 1), (8, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, true, false, false); // tsumo
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Sanankou)),
            "Sanankou 应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_yakuhai_via_pon_meld() {
        // 副露 Pon 三元/风牌 → 触发 detect_yaku 副露 yakuhai 路径 (line 376-384).
        // 闭手 11 张 + Pon 中×3 (副露). 14 张: 234m + 234p + 234s + 99m + Pon 中.
        let closed = h(&[
            (1, 1), (2, 1), (3, 1),
            (10, 1), (11, 1), (12, 1),
            (19, 1), (20, 1), (21, 1),
            (8, 2),
        ]);
        use crate::engine::domain::meld::{Meld, MeldKind, Seat};
        use crate::engine::domain::tile::Tile;
        let melds = vec![Meld {
            kind: MeldKind::Pon {
                tiles: [
                    Tile { kind: TileIndex(33), red: false, id: 0 },
                    Tile { kind: TileIndex(33), red: false, id: 1 },
                    Tile { kind: TileIndex(33), red: false, id: 2 },
                ],
            },
            from: Some(Seat::West),
        }];
        let r = decompose(&closed, &melds, TileIndex(1));
        let d = r.iter().next().expect("应有拆解");
        let ctx = std_ctx(d, false, false, false, false); // 副露 → 非 menzen
        let yakus = detect_yaku(&ctx, &melds);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Yakuhai(YakuhaiKind::Chun))),
            "Pon 中应识别 Yakuhai(Chun), got {:?}", yakus
        );
    }

    #[test]
    fn detect_chuurenpoutou_yakuman() {
        // 九莲宝灯 = 同色 1112345678999 + 任意一张额外凑成和牌型.
        // 14 张全 m: 1m×3 + 2m×1 + 3m×1 + 4m×1 + 5m×1 + 6m×1 + 7m×1 + 8m×1 + 9m×4? 不对, 总 13+1.
        // 正型: 1m=3, 2m=1, 3m=1, 4m=1, 5m=1, 6m=1, 7m=1, 8m=1, 9m=3 = 13张. winning 任何 m.
        // winning=5m → hand 5m=2.
        let hand = h(&[(0, 3), (1, 1), (2, 1), (3, 1), (4, 2), (5, 1), (6, 1), (7, 1), (8, 3)]);
        let r = decompose(&hand, &[], TileIndex(4));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Chuurenpoutou { .. })),
            "九莲宝灯应识别 Chuurenpoutou, got {:?}", yakus
        );
    }

    #[test]
    fn detect_sanshoku_doukou() {
        // 三色同刻 = 同 kind 在 m/p/s 各成刻 (例: 555m + 555p + 555s).
        // 14 张: 555m + 555p + 555s + 234m + 99m.
        let hand = h(&[
            (4, 3), (13, 3), (22, 3), // 555 三色同刻
            (1, 1), (2, 1), (3, 1),    // 234m
            (8, 2),                     // 99m 雀头
        ]);
        let r = decompose(&hand, &[], TileIndex(4));
        let d = r.iter().find(|d| matches!(d, Decomposition::Standard { .. })).unwrap();
        let ctx = std_ctx(d, true, false, false, false);
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::SanshokuDoukou)),
            "555 三色同刻应识别, got {:?}", yakus
        );
    }

    #[test]
    fn detect_sankantsu_three_kans() {
        // 三杠子 = 3 个杠 + 1 面子 + 雀头. 副露 3 杠 + 闭手 1 mentsu + 1 雀头.
        use crate::engine::domain::meld::{Meld, MeldKind, Seat};
        use crate::engine::domain::tile::Tile;
        let closed = h(&[(1, 1), (2, 1), (3, 1), (8, 2)]);
        let mk_kan = |kind: u8, base_id: u16| Meld {
            kind: MeldKind::Minkan {
                tiles: [
                    Tile { kind: TileIndex(kind), red: false, id: base_id },
                    Tile { kind: TileIndex(kind), red: false, id: base_id + 1 },
                    Tile { kind: TileIndex(kind), red: false, id: base_id + 2 },
                    Tile { kind: TileIndex(kind), red: false, id: base_id + 3 },
                ],
            },
            from: Some(Seat::West),
        };
        let melds = vec![mk_kan(13, 0), mk_kan(22, 10), mk_kan(0, 20)];
        let r = decompose(&closed, &melds, TileIndex(1));
        let d = r.iter().next().expect("应有拆解");
        let ctx = std_ctx(d, false, false, false, false);
        let yakus = detect_yaku(&ctx, &melds);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Sankantsu)),
            "3 副露杠应识别 Sankantsu, got {:?}", yakus
        );
    }

    #[test]
    fn detect_kotekisai_renhou_when_enabled() {
        // 古役"人和" — 子家第 1 巡内荣和上家弃牌. 需 rules.kotekisai +
        // rules.kotekisai_renhou 同时开 + ctx.is_renhou.
        let hand = h(&[(0, 2), (2, 2), (4, 2), (6, 2), (9, 2), (33, 2), (29, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        let d = r.iter().find(|d| matches!(d, Decomposition::Chiitoitsu { .. })).unwrap();

        let cfg: &'static GameRules = Box::leak(Box::new(GameRules {
            kotekisai: true,
            kotekisai_renhou: true,
            ..GameRules::default()
        }));
        let ctx = WinContext {
            decomposition: d,
            seat_wind: TileIndex::SOUTH,
            round_wind: TileIndex::EAST,
            winning_tile: TileIndex(0),
            is_tsumo: false,
            is_riichi: false,
            is_double_riichi: false,
            is_ippatsu: false,
            is_haitei: false,
            is_houtei: false,
            is_rinshan: false,
            is_chankan: false,
            is_tenhou: false,
            is_chiihou: false,
            is_renhou: true,
            menzen: true,
            fully_concealed: true,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            rules: cfg,
        };
        let yakus = detect_yaku(&ctx, &[]);
        assert!(
            yakus.iter().any(|(y, _)| matches!(y, Yaku::Renhou)),
            "kotekisai 开启 + is_renhou 应识别 Renhou, got {:?}", yakus
        );
    }
}
