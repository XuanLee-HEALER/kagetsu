//! 役种判定.
//!
//! 详见 docs/spec/yaku.md
//!
//! 实现优先级:
//! - 全部标准役 (1-6 番) 完整实现
//! - 全部役满完整实现
//! - 古役: 类型完整, 实现按需逐个补 (默认关闭)

use crate::domain::decompose::{Decomposition, Mentsu, WaitKind};
use crate::domain::meld::Meld;
use crate::domain::tile::TileIndex;
use crate::engine::rules::GameRules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum YakuhaiKind {
    Haku,
    Hatsu,
    Chun,
    BakaWind,   // 场风
    JikaWind,   // 自风
    DoubleWind, // 场风=自风(连风)
}

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

/// 一次和牌的上下文.
#[derive(Debug, Clone)]
pub struct WinContext<'a> {
    pub decomposition: &'a Decomposition,
    pub seat_wind: TileIndex,
    pub round_wind: TileIndex,
    pub winning_tile: TileIndex,

    pub is_tsumo: bool,
    pub is_riichi: bool,
    pub is_double_riichi: bool,
    pub is_ippatsu: bool,
    pub is_haitei: bool,
    pub is_houtei: bool,
    pub is_rinshan: bool,
    pub is_chankan: bool,
    pub is_tenhou: bool,
    pub is_chiihou: bool,
    pub is_renhou: bool,

    /// 门清(无副露; 暗杠不算副露).
    pub menzen: bool,
    /// 完全无副露(包括暗杠).
    pub fully_concealed: bool,

    pub dora_count: u32,
    pub aka_count: u32,
    pub ura_dora_count: u32,

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
                crate::domain::meld::MeldKind::Pon { tiles } => Some(tiles[0]),
                crate::domain::meld::MeldKind::Minkan { tiles } => Some(tiles[0]),
                crate::domain::meld::MeldKind::Shouminkan { tiles } => Some(tiles[0]),
                crate::domain::meld::MeldKind::Ankan { tiles } => Some(tiles[0]),
                crate::domain::meld::MeldKind::Chi { .. } => None,
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
            .all(|m| !matches!(m.kind, crate::domain::meld::MeldKind::Chi { .. }));
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
        // 三杠子
        if mentsu
            .iter()
            .filter(|m| matches!(m, Mentsu::Kantsu(_, _)))
            .count()
            >= 3
        {
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
            crate::domain::meld::MeldKind::Chi { tiles, .. } => {
                let mut kinds = [tiles[0].kind.0, tiles[1].kind.0, tiles[2].kind.0];
                kinds.sort();
                out.push(Mentsu::Shuntsu(TileIndex(kinds[0])));
            }
            crate::domain::meld::MeldKind::Pon { tiles } => {
                out.push(Mentsu::Koutsu(tiles[0].kind, false));
            }
            crate::domain::meld::MeldKind::Minkan { tiles }
            | crate::domain::meld::MeldKind::Shouminkan { tiles } => {
                out.push(Mentsu::Kantsu(tiles[0].kind, false));
            }
            crate::domain::meld::MeldKind::Ankan { tiles } => {
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
            crate::domain::meld::MeldKind::Chi { .. } => {
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
        if matches!(meld.kind, crate::domain::meld::MeldKind::Chi { .. }) {
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
    use crate::domain::decompose::decompose;
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
}
