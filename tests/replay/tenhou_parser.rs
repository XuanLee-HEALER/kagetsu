//! 天凤 mjlog XML parser.
//!
//! 解析格式参考: <https://m77.hatenablog.com/entry/2017/05/21/214529>
//!
//! ```xml
//! <mjloggm ver="2.3">
//!   <SHUFFLE seed="..." ref=""/>
//!   <GO type="169" lobby="0"/>
//!   <UN n0="..." n1="..." n2="..." n3="..." dan="..." rate="..." sx="..."/>
//!   <TAIKYOKU oya="0"/>
//!   <INIT seed="kyoku,honba,sticks,dice0,dice1,dora_indicator"
//!         ten="250,250,250,250" oya="0"
//!         hai0="..." hai1="..." hai2="..." hai3="..."/>
//!   <T81/> <D117/>      <!-- East 摸 81, 切 117 -->
//!   <U130/> <E118/>     <!-- South 摸 130, 切 118 -->
//!   <V15/> <F109/>      <!-- West -->
//!   <W75/> <G0/>        <!-- North -->
//!   <N who="3" m="44042"/>
//!   <DORA hai="51"/>
//!   <REACH who="0" step="1"/>
//!   <REACH who="0" step="2" ten="240,250,250,250"/>
//!   <AGARI ba="0,0" hai="..." ten="20,5200,1" yaku="1,1,7,1" who="0" fromWho="2" sc="..."/>
//!   <RYUUKYOKU ba="0,0" sc="..."/>
//! </mjloggm>
//! ```

use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;

/// 解析后的天凤事件 (lossy: 我们只保留 driver 关心的字段).
#[derive(Debug, Clone)]
pub enum TenhouEvent {
    /// 一局开始. seed = "kyoku,honba,sticks,dice0,dice1,dora_indicator".
    Init {
        seed: Vec<u16>, // 6 elements
        ten: [i32; 4],  // 分数 (× 100)
        oya: u8,
        hais: [Vec<u16>; 4], // 4 家初始 13 张 tile id
    },
    /// 摸牌. who ∈ 0..4, tile_id 0..136.
    Tsumo { who: u8, tile_id: u16 },
    /// 切牌.
    Dahai {
        who: u8,
        tile_id: u16,
        tsumogiri: bool, // 摸切 (天凤通过比对前一摸推算, 此 phase 默认 false)
    },
    /// 鸣牌. m 是 16-bit 编码.
    Naki { who: u8, m: u16 },
    /// 翻新表 dora.
    Dora { tile_id: u16 },
    /// 立直 step (1=宣言, 2=成立).
    Reach {
        who: u8,
        step: u8,
        ten: Option<[i32; 4]>,
    },
    /// 和牌.
    Agari {
        ba: [u8; 2],        // [本场, 立直棒]
        hai: Vec<u16>,      // 和牌时手牌 tile id (含 winning tile)
        machi: u16,         // 待牌 tile id
        ten: Vec<i32>,      // [fu, points, limit_kind]
        yaku: Vec<u8>,      // [id, han, id, han, ...] 普通役
        yakuman: Vec<u8>,   // 役満 id 列表 (无 han, 默认 13 番)
        dora_hai: Vec<u16>, // 表 dora 指示牌 ids
        ura_hai: Vec<u16>,  // 里 dora 指示牌 ids
        who: u8,
        from_who: u8,
        sc: Vec<i32>, // 分数变化 [before0, delta0, before1, delta1, ...]
    },
    /// 流局.
    Ryuukyoku {
        ba: [u8; 2],
        sc: Vec<i32>,
        reason: Option<String>,      // type="yao9" / "nm" / 缺则普通流局
        hais: [Option<Vec<u16>>; 4], // 听牌方手牌 (如有)
    },
    /// 全局元: 玩家名 / 房间类型. 解析为字符串保留, driver 不一定用.
    GameMeta { game_type: Option<String> },
}

/// 解析整个 mjlog XML 内容.
pub fn parse_mjlog(xml: &str) -> Result<Vec<TenhouEvent>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut events = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| format!("XML 解析错误 at {}: {e}", reader.buffer_position()))?
        {
            XmlEvent::Eof => break,
            XmlEvent::Start(ref e) | XmlEvent::Empty(ref e) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .map_err(|err| format!("非 UTF-8 元素名: {err}"))?
                    .to_string();
                let attrs: Vec<(String, String)> = e
                    .attributes()
                    .filter_map(|a| a.ok())
                    .map(|a| {
                        let k = std::str::from_utf8(a.key.as_ref())
                            .unwrap_or("")
                            .to_string();
                        let v = std::str::from_utf8(&a.value).unwrap_or("").to_string();
                        (k, v)
                    })
                    .collect();
                if let Some(ev) = decode_element(&name, &attrs)? {
                    events.push(ev);
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(events)
}

fn decode_element(name: &str, attrs: &[(String, String)]) -> Result<Option<TenhouEvent>, String> {
    // 单字符摸切 element name (T/U/V/W/D/E/F/G + 数字)
    if name.len() >= 2
        && let Some(first) = name.chars().next()
        && let Ok(tile_id) = name[1..].parse::<u16>()
    {
        // 摸: T(0)/U(1)/V(2)/W(3)
        if let Some(who) = match first {
            'T' => Some(0u8),
            'U' => Some(1),
            'V' => Some(2),
            'W' => Some(3),
            _ => None,
        } {
            return Ok(Some(TenhouEvent::Tsumo { who, tile_id }));
        }
        // 切: D(0)/E(1)/F(2)/G(3)
        if let Some(who) = match first {
            'D' => Some(0u8),
            'E' => Some(1),
            'F' => Some(2),
            'G' => Some(3),
            _ => None,
        } {
            return Ok(Some(TenhouEvent::Dahai {
                who,
                tile_id,
                tsumogiri: false,
            }));
        }
    }

    match name {
        "INIT" => Ok(Some(decode_init(attrs)?)),
        "N" => {
            let who: u8 = get_attr(attrs, "who")?
                .parse()
                .map_err(|e| format!("N who: {e}"))?;
            let m: u16 = get_attr(attrs, "m")?
                .parse()
                .map_err(|e| format!("N m: {e}"))?;
            Ok(Some(TenhouEvent::Naki { who, m }))
        }
        "DORA" => {
            let tile_id: u16 = get_attr(attrs, "hai")?
                .parse()
                .map_err(|e| format!("DORA hai: {e}"))?;
            Ok(Some(TenhouEvent::Dora { tile_id }))
        }
        "REACH" => {
            let who: u8 = get_attr(attrs, "who")?
                .parse()
                .map_err(|e| format!("REACH who: {e}"))?;
            let step: u8 = get_attr(attrs, "step")?
                .parse()
                .map_err(|e| format!("REACH step: {e}"))?;
            let ten = get_optional_attr(attrs, "ten").and_then(|s| parse_i32_csv_4(&s).ok());
            Ok(Some(TenhouEvent::Reach { who, step, ten }))
        }
        "AGARI" => Ok(Some(decode_agari(attrs)?)),
        "RYUUKYOKU" => Ok(Some(decode_ryuukyoku(attrs)?)),
        "GO" => {
            let game_type = get_optional_attr(attrs, "type");
            Ok(Some(TenhouEvent::GameMeta { game_type }))
        }
        // SHUFFLE / UN / TAIKYOKU 元信息, driver 不需要
        _ => Ok(None),
    }
}

fn decode_init(attrs: &[(String, String)]) -> Result<TenhouEvent, String> {
    let seed_str = get_attr(attrs, "seed")?;
    let seed: Vec<u16> = seed_str
        .split(',')
        .map(|s| s.parse::<u16>().map_err(|e| format!("INIT seed: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    if seed.len() != 6 {
        return Err(format!("INIT seed 应有 6 元素, 实际 {}", seed.len()));
    }
    let ten = parse_i32_csv_4(&get_attr(attrs, "ten")?)?;
    let oya: u8 = get_attr(attrs, "oya")?
        .parse()
        .map_err(|e| format!("oya: {e}"))?;
    let mut hais: [Vec<u16>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for (i, slot) in hais.iter_mut().enumerate() {
        let key = format!("hai{i}");
        let s = get_attr(attrs, &key)?;
        *slot = s
            .split(',')
            .map(|v| v.parse::<u16>().map_err(|e| format!("{key}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        if slot.len() != 13 {
            return Err(format!("{key} 应 13 张, 实际 {}", slot.len()));
        }
    }
    Ok(TenhouEvent::Init {
        seed,
        ten,
        oya,
        hais,
    })
}

fn decode_agari(attrs: &[(String, String)]) -> Result<TenhouEvent, String> {
    let ba_str = get_attr(attrs, "ba")?;
    let ba_parts: Vec<u8> = ba_str
        .split(',')
        .map(|s| s.parse::<u8>().map_err(|e| format!("ba: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    if ba_parts.len() != 2 {
        return Err("ba 应 2 元素".into());
    }
    let ba = [ba_parts[0], ba_parts[1]];

    let hai = parse_u16_csv(&get_attr(attrs, "hai")?)?;
    let machi: u16 = get_attr(attrs, "machi")?
        .parse()
        .map_err(|e| format!("machi: {e}"))?;
    let ten = parse_i32_csv(&get_attr(attrs, "ten")?)?;
    let yaku = if let Some(s) = get_optional_attr(attrs, "yaku") {
        parse_u8_csv(&s)?
    } else {
        Vec::new()
    };
    let yakuman = if let Some(s) = get_optional_attr(attrs, "yakuman") {
        parse_u8_csv(&s)?
    } else {
        Vec::new()
    };
    let dora_hai = if let Some(s) = get_optional_attr(attrs, "doraHai") {
        parse_u16_csv(&s)?
    } else {
        Vec::new()
    };
    let ura_hai = if let Some(s) = get_optional_attr(attrs, "doraHaiUra") {
        parse_u16_csv(&s)?
    } else {
        Vec::new()
    };
    let who: u8 = get_attr(attrs, "who")?
        .parse()
        .map_err(|e| format!("who: {e}"))?;
    let from_who: u8 = get_attr(attrs, "fromWho")?
        .parse()
        .map_err(|e| format!("fromWho: {e}"))?;
    let sc = parse_i32_csv(&get_attr(attrs, "sc")?)?;

    Ok(TenhouEvent::Agari {
        ba,
        hai,
        machi,
        ten,
        yaku,
        yakuman,
        dora_hai,
        ura_hai,
        who,
        from_who,
        sc,
    })
}

fn decode_ryuukyoku(attrs: &[(String, String)]) -> Result<TenhouEvent, String> {
    let ba_str = get_attr(attrs, "ba")?;
    let ba_parts: Vec<u8> = ba_str
        .split(',')
        .map(|s| s.parse::<u8>().map_err(|e| format!("ba: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let ba = [ba_parts[0], ba_parts[1]];
    let sc = parse_i32_csv(&get_attr(attrs, "sc")?)?;
    let reason = get_optional_attr(attrs, "type");
    let mut hais: [Option<Vec<u16>>; 4] = [None, None, None, None];
    for (i, slot) in hais.iter_mut().enumerate() {
        if let Some(s) = get_optional_attr(attrs, &format!("hai{i}")) {
            *slot = Some(parse_u16_csv(&s)?);
        }
    }
    Ok(TenhouEvent::Ryuukyoku {
        ba,
        sc,
        reason,
        hais,
    })
}

// ============================================================================
// helpers
// ============================================================================

fn get_attr(attrs: &[(String, String)], key: &str) -> Result<String, String> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| format!("缺少属性 '{key}'"))
}

fn get_optional_attr(attrs: &[(String, String)], key: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn parse_i32_csv(s: &str) -> Result<Vec<i32>, String> {
    s.split(',')
        .map(|v| v.parse::<i32>().map_err(|e| format!("i32: {e}")))
        .collect()
}

fn parse_i32_csv_4(s: &str) -> Result<[i32; 4], String> {
    let v = parse_i32_csv(s)?;
    if v.len() != 4 {
        return Err(format!("应 4 元素, 实际 {}", v.len()));
    }
    Ok([v[0], v[1], v[2], v[3]])
}

fn parse_u16_csv(s: &str) -> Result<Vec<u16>, String> {
    s.split(',')
        .map(|v| v.parse::<u16>().map_err(|e| format!("u16: {e}")))
        .collect()
}

fn parse_u8_csv(s: &str) -> Result<Vec<u8>, String> {
    s.split(',')
        .map(|v| v.parse::<u8>().map_err(|e| format!("u8: {e}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_mjlog() {
        let xml = r#"<mjloggm ver="2.3">
<INIT seed="0,0,0,3,5,114" ten="250,250,250,250" oya="0"
      hai0="86,123,1,40,93,82,82,55,12,24,79,113,33"
      hai1="51,76,124,93,127,108,3,2,118,22,16,17,15"
      hai2="62,131,99,57,121,7,42,77,68,46,4,11,10"
      hai3="48,49,5,28,8,73,9,98,80,38,30,6,37"/>
<T74/><D0/>
<U91/><E64/>
<RYUUKYOKU ba="0,0" sc="240,15,250,-15,250,-15,250,15"/>
</mjloggm>"#;
        let events = parse_mjlog(xml).expect("parse OK");
        assert!(events.iter().any(|e| matches!(e, TenhouEvent::Init { .. })));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, TenhouEvent::Tsumo { .. }))
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, TenhouEvent::Dahai { .. }))
                .count(),
            2
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TenhouEvent::Ryuukyoku { .. }))
        );
    }

    #[test]
    fn parse_init_fields() {
        let xml = r#"<mjloggm><INIT seed="2,3,1,5,5,130" ten="163,405,227,205" oya="2"
            hai0="44,34,8,31,85,7,74,123,62,88,61,47,21"
            hai1="38,109,23,19,64,51,41,129,16,0,17,11,42"
            hai2="101,70,48,9,86,126,98,40,103,105,115,69,57"
            hai3="131,81,106,26,37,120,52,94,80,97,32,127,35"/></mjloggm>"#;
        let events = parse_mjlog(xml).unwrap();
        match &events[0] {
            TenhouEvent::Init {
                seed,
                ten,
                oya,
                hais,
            } => {
                assert_eq!(seed, &vec![2u16, 3, 1, 5, 5, 130]);
                assert_eq!(ten, &[163, 405, 227, 205]);
                assert_eq!(*oya, 2);
                assert_eq!(hais[0].len(), 13);
                assert_eq!(hais[0][0], 44);
            }
            _ => panic!("expected INIT"),
        }
    }

    #[test]
    fn parse_naki() {
        let xml = r#"<mjloggm><N who="3" m="44042" /></mjloggm>"#;
        let events = parse_mjlog(xml).unwrap();
        match &events[0] {
            TenhouEvent::Naki { who, m } => {
                assert_eq!(*who, 3);
                assert_eq!(*m, 44042);
            }
            _ => panic!("expected Naki"),
        }
    }

    #[test]
    fn parse_reach_two_steps() {
        let xml = r#"<mjloggm>
<REACH who="1" step="1"/>
<REACH who="1" step="2" ten="245,237,261,247"/>
</mjloggm>"#;
        let events = parse_mjlog(xml).unwrap();
        match &events[0] {
            TenhouEvent::Reach { step, ten, .. } => {
                assert_eq!(*step, 1);
                assert!(ten.is_none());
            }
            _ => panic!(),
        }
        match &events[1] {
            TenhouEvent::Reach { step, ten, .. } => {
                assert_eq!(*step, 2);
                assert!(ten.is_some());
                assert_eq!(ten.unwrap(), [245, 237, 261, 247]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_agari_with_yaku() {
        let xml = r#"<mjloggm><AGARI ba="0,1" hai="52,54,55,56,60,61,64,67,69,77,79,97,101,106"
            machi="61" ten="30,18000,2"
            yaku="1,1,0,1,52,3,54,1,53,0"
            doraHai="51" doraHaiUra="111"
            who="1" fromWho="1" sc="245,-60,237,190,261,-60,247,-60"/></mjloggm>"#;
        let events = parse_mjlog(xml).unwrap();
        match &events[0] {
            TenhouEvent::Agari {
                ba,
                hai,
                machi,
                ten,
                yaku,
                who,
                from_who,
                sc,
                ..
            } => {
                assert_eq!(ba, &[0, 1]);
                assert_eq!(hai.len(), 14);
                assert_eq!(*machi, 61);
                assert_eq!(ten, &vec![30, 18000, 2]);
                // yaku 是 [id, han, id, han, ...] 配对
                assert_eq!(yaku.len() % 2, 0);
                assert_eq!(*who, 1);
                assert_eq!(*from_who, 1);
                assert_eq!(sc.len(), 8);
            }
            _ => panic!("expected Agari"),
        }
    }

    #[test]
    fn parse_agari_yakuman() {
        let xml = r#"<mjloggm><AGARI ba="0,0" hai="0,4,8,12,16,20,24,28,32,108,112,116,120,124"
            machi="0" ten="0,32000,5"
            yakuman="40"
            who="0" fromWho="0" sc="250,32,250,-10,250,-10,250,-12"/></mjloggm>"#;
        let events = parse_mjlog(xml).unwrap();
        match &events[0] {
            TenhouEvent::Agari { yakuman, yaku, .. } => {
                assert_eq!(yakuman, &vec![40u8]);
                assert!(yaku.is_empty());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_ryuukyoku_with_hais() {
        let xml = r#"<mjloggm><RYUUKYOKU ba="1,0" sc="185,-10,427,-10,201,-10,187,30"
            hai3="4,10,133,135"/></mjloggm>"#;
        let events = parse_mjlog(xml).unwrap();
        match &events[0] {
            TenhouEvent::Ryuukyoku { ba, sc, hais, .. } => {
                assert_eq!(ba, &[1, 0]);
                assert_eq!(sc.len(), 8);
                assert!(hais[0].is_none());
                assert!(hais[3].is_some());
                assert_eq!(hais[3].as_ref().unwrap(), &vec![4u16, 10, 133, 135]);
            }
            _ => panic!(),
        }
    }
}
