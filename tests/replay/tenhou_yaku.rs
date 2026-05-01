//! 天凤役 id (0-54) ↔ mjai 役名映射.
//!
//! 表来自 mjx-project 与天凤公开规范.
//! mjai 役名用 snake_case 罗马字 (mjai 协议 spec).

/// 天凤 id → mjai 役名 (snake_case).
pub fn tenhou_yaku_id_to_mjai(id: u8) -> Option<&'static str> {
    Some(match id {
        // 1 番役
        0 => "menzentsumo",
        1 => "riichi",
        2 => "ippatsu",
        3 => "chankan",
        4 => "rinshankaihou",
        5 => "haiteiraoyue",
        6 => "houteiraoyui",
        7 => "pinfu",
        8 => "tanyao",
        9 => "iipeikou",
        // 10-13: 自风牌
        10 => "jikaze_e",
        11 => "jikaze_s",
        12 => "jikaze_w",
        13 => "jikaze_n",
        // 14-17: 场风牌
        14 => "bakaze_e",
        15 => "bakaze_s",
        16 => "bakaze_w",
        17 => "bakaze_n",
        18 => "haku",
        19 => "hatsu",
        20 => "chun",
        // 2 番役
        21 => "double_riichi",
        22 => "chiitoitsu",
        23 => "chanta",
        24 => "ittsu",
        25 => "sanshoku_doujun",
        26 => "sanshoku_doukou",
        27 => "sankantsu",
        28 => "toitoi",
        29 => "sanankou",
        30 => "shousangen",
        31 => "honroutou",
        // 3 番役
        32 => "junchan",
        33 => "honitsu",
        // 3 番役 (二盃口算 3 番)
        34 => "ryanpeikou",
        // 6 番役
        35 => "chinitsu",
        // 役満
        36 => "renhou", // 人和 (天凤当役満, 部分规则当满贯)
        37 => "tenhou",
        38 => "chiihou",
        39 => "daisangen",
        40 => "suuankou",
        41 => "suuankou_tanki",
        42 => "tsuuiisou",
        43 => "ryuuiisou",
        44 => "chinroutou",
        45 => "kokushimusou",
        46 => "kokushimusou_juusanmenmachi",
        47 => "daisuushii",
        48 => "shousuushii",
        49 => "suukantsu",
        50 => "chuurenpoutou",
        51 => "chuurenpoutou_kyuumenmachi",
        // 特殊
        52 => "dora",
        53 => "uradora",
        54 => "akadora",
        _ => return None,
    })
}

/// 反向: mjai 役名 → 天凤 id (用于双向 round-trip).
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
        "junchan" | "junchantaiyaochuu" => 32,
        "honitsu" | "honiisou" => 33,
        "ryanpeikou" => 34,
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
        "kokushimusou" => 45,
        "kokushimusou_juusanmenmachi" | "kokushimusou_13" => 46,
        "daisuushii" | "daisuushi" => 47,
        "shousuushii" | "shousuushi" => 48,
        "suukantsu" => 49,
        "chuurenpoutou" | "chuuren_poutou" => 50,
        "chuurenpoutou_kyuumenmachi" => 51,
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
        // mjai 名字有别称, 但都映射到同一 id
        assert_eq!(mjai_yaku_to_tenhou_id("tsumo"), Some(0));
        assert_eq!(mjai_yaku_to_tenhou_id("menzentsumo"), Some(0));
        assert_eq!(mjai_yaku_to_tenhou_id("ittsu"), Some(24));
        assert_eq!(mjai_yaku_to_tenhou_id("ittsuu"), Some(24));
    }
}
