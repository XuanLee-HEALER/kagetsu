//! 天凤役 id (0-54) ↔ mjai 役名映射.
//!
//! 表来自 mjx-project/mjx (官方 C++ 实现):
//! <https://github.com/mjx-project/mjx/blob/master/include/mjx/internal/types.h>
//!
//! mjai 役名用 snake_case 罗马字 (mjai 协议 spec).

/// 天凤 id → mjai 役名 (snake_case).
pub fn tenhou_yaku_id_to_mjai(id: u8) -> Option<&'static str> {
    Some(match id {
        // 1 番役
        0 => "menzentsumo",   // kFullyConcealedHand
        1 => "riichi",        // kRiichi
        2 => "ippatsu",       // kIppatsu
        3 => "chankan",       // kRobbingKan
        4 => "rinshankaihou", // kAfterKan
        5 => "haiteiraoyue",  // kBottomOfTheSea
        6 => "houteiraoyui",  // kBottomOfTheRiver
        7 => "pinfu",         // kPinfu
        8 => "tanyao",        // kAllSimples
        9 => "iipeikou",      // kPureDoubleChis
        10 => "jikaze_e",     // kSeatWindEast
        11 => "jikaze_s",     // kSeatWindSouth
        12 => "jikaze_w",     // kSeatWindWest
        13 => "jikaze_n",     // kSeatWindNorth
        14 => "bakaze_e",     // kPrevalentWindEast
        15 => "bakaze_s",     // kPrevalentWindSouth
        16 => "bakaze_w",     // kPrevalentWindWest
        17 => "bakaze_n",     // kPrevalentWindNorth
        18 => "haku",         // kWhiteDragon
        19 => "hatsu",        // kGreenDragon
        20 => "chun",         // kRedDragon
        // 2 番役
        21 => "double_riichi",   // kDoubleRiichi
        22 => "chiitoitsu",      // kSevenPairs
        23 => "chanta",          // kOutsideHand
        24 => "ittsu",           // kPureStraight
        25 => "sanshoku_doujun", // kMixedTripleChis
        26 => "sanshoku_doukou", // kTriplePons
        27 => "sankantsu",       // kThreeKans
        28 => "toitoi",          // kAllPons
        29 => "sanankou",        // kThreeConcealedPons
        30 => "shousangen",      // kLittleThreeDragons
        31 => "honroutou",       // kAllTermsAndHonours
        // 3 番役
        32 => "ryanpeikou", // kTwicePureDoubleChis (二盃口)
        33 => "junchan",    // kTerminalsInAllSets (纯全帯)
        34 => "honitsu",    // kHalfFlush (混一色)
        // 6 番役
        35 => "chinitsu", // kFullFlush (清一色)
        // 满贯
        36 => "renhou", // kBlessingOfMan (人和, 满贯)
        // 役満
        37 => "tenhou",                      // kBlessingOfHeaven
        38 => "chiihou",                     // kBlessingOfEarth
        39 => "daisangen",                   // kBigThreeDragons
        40 => "suuankou",                    // kFourConcealedPons
        41 => "suuankou_tanki",              // kCompletedFourConcealedPons
        42 => "tsuuiisou",                   // kAllHonours (字一色)
        43 => "ryuuiisou",                   // kAllGreen (绿一色)
        44 => "chinroutou",                  // kAllTerminals (清老头)
        45 => "chuurenpoutou",               // kNineGates (九莲)
        46 => "chuurenpoutou_kyuumenmachi",  // kPureNineGates (纯正九莲)
        47 => "kokushimusou",                // kThirteenOrphans
        48 => "kokushimusou_juusanmenmachi", // kCompletedThirteenOrphans
        49 => "daisuushii",                  // kBigFourWinds
        50 => "shousuushii",                 // kLittleFourWinds
        51 => "suukantsu",                   // kFourKans
        // dora
        52 => "dora",
        53 => "uradora",
        54 => "akadora",
        _ => return None,
    })
}

/// 反向: mjai 役名 → 天凤 id.
pub fn mjai_yaku_to_tenhou_id(name: &str) -> Option<u8> {
    Some(match name {
        "menzentsumo" | "tsumo" => 0,
        "riichi" => 1,
        "ippatsu" => 2,
        "chankan" => 3,
        "rinshankaihou" => 4,
        "haiteiraoyue" | "haitei" => 5,
        "houteiraoyui" | "houtei" => 6,
        "pinfu" => 7,
        "tanyao" => 8,
        "iipeikou" => 9,
        "jikaze_e" | "ton" => 10,
        "jikaze_s" | "nan" => 11,
        "jikaze_w" | "shaa" => 12,
        "jikaze_n" | "pei" => 13,
        "bakaze_e" => 14,
        "bakaze_s" => 15,
        "bakaze_w" => 16,
        "bakaze_n" => 17,
        "haku" => 18,
        "hatsu" => 19,
        "chun" => 20,
        "double_riichi" | "daburu_riichi" => 21,
        "chiitoitsu" => 22,
        "chanta" => 23,
        "ittsu" | "ittsuu" => 24,
        "sanshoku_doujun" | "sanshoku" => 25,
        "sanshoku_doukou" => 26,
        "sankantsu" => 27,
        "toitoi" | "toitoihou" => 28,
        "sanankou" => 29,
        "shousangen" => 30,
        "honroutou" => 31,
        "ryanpeikou" => 32,
        "junchan" | "junchantaiyaochuu" => 33,
        "honitsu" | "honiisou" => 34,
        "chinitsu" | "chiniisou" => 35,
        "renhou" => 36,
        "tenhou" => 37,
        "chiihou" => 38,
        "daisangen" => 39,
        "suuankou" => 40,
        "suuankou_tanki" => 41,
        "tsuuiisou" | "tsuiisou" => 42,
        "ryuuiisou" => 43,
        "chinroutou" => 44,
        "chuurenpoutou" | "chuuren_poutou" => 45,
        "chuurenpoutou_kyuumenmachi" => 46,
        "kokushimusou" => 47,
        "kokushimusou_juusanmenmachi" | "kokushimusou_13" => 48,
        "daisuushii" | "daisuushi" => 49,
        "shousuushii" | "shousuushi" => 50,
        "suukantsu" => 51,
        "dora" => 52,
        "uradora" | "ura_dora" => 53,
        "akadora" | "aka_dora" => 54,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_ids() {
        for id in 0u8..=54 {
            let name = tenhou_yaku_id_to_mjai(id).unwrap();
            let back = mjai_yaku_to_tenhou_id(name).unwrap();
            assert_eq!(back, id, "id {id} ↔ '{name}' round-trip 失败");
        }
    }

    #[test]
    fn unknown_returns_none() {
        assert!(tenhou_yaku_id_to_mjai(55).is_none());
        assert!(tenhou_yaku_id_to_mjai(255).is_none());
        assert!(mjai_yaku_to_tenhou_id("not_a_yaku").is_none());
    }

    #[test]
    fn aliases_work() {
        assert_eq!(mjai_yaku_to_tenhou_id("tsumo"), Some(0));
        assert_eq!(mjai_yaku_to_tenhou_id("menzentsumo"), Some(0));
        assert_eq!(mjai_yaku_to_tenhou_id("ittsu"), Some(24));
        assert_eq!(mjai_yaku_to_tenhou_id("ittsuu"), Some(24));
    }

    /// 关键 id 验证 (与 mjx-project 标准对齐).
    #[test]
    fn critical_ids_match_mjx() {
        assert_eq!(tenhou_yaku_id_to_mjai(8), Some("tanyao"));
        assert_eq!(tenhou_yaku_id_to_mjai(9), Some("iipeikou"));
        assert_eq!(tenhou_yaku_id_to_mjai(32), Some("ryanpeikou"));
        assert_eq!(tenhou_yaku_id_to_mjai(33), Some("junchan"));
        assert_eq!(tenhou_yaku_id_to_mjai(34), Some("honitsu"));
        assert_eq!(tenhou_yaku_id_to_mjai(35), Some("chinitsu"));
        assert_eq!(tenhou_yaku_id_to_mjai(42), Some("tsuuiisou"));
        assert_eq!(tenhou_yaku_id_to_mjai(47), Some("kokushimusou"));
    }
}
