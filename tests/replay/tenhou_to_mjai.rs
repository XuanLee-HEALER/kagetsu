//! 把 [`TenhouEvent`] 流转换成 [`MjaiEvent`] 流, 接到 P3-P5 的 driver.
//!
//! ## 状态追踪
//!
//! - 上一次 Tsumo (检测 tsumogiri)
//! - 当前 oya / round_wind / honba (推算 mjai start_kyoku 字段)
//! - INIT 时记录 4 家初始手牌
//!
//! ## 局编号语义
//!
//! 天凤 INIT.seed[0] 是"绝对" kyoku index: 0..7 表示 东1..东4, 南1..南4.
//! mjai 用 (bakaze, kyoku) 二元: bakaze='E'/'S'/'W', kyoku=1..4.
//! 转换: bakaze = ['E','S','W','N'][seed[0]/4], kyoku = seed[0]%4 + 1.

use crate::replay::mjai_pai::tile_to_mjai_pai;
use crate::replay::mjai_parser::{MjaiEvent, MjaiYaku};
use crate::replay::tenhou_meld::{DecodedMeld, relative_to_seat};
use crate::replay::tenhou_pai::tenhou_id_to_tile;
use crate::replay::tenhou_parser::TenhouEvent;
use crate::replay::tenhou_yaku::tenhou_yaku_id_to_mjai;
use tui_majo::meld::{MeldKind, Seat};
use tui_majo::tile::Tile;

/// 转换器: 状态机, 吃 TenhouEvent, 吐 MjaiEvent.
pub fn tenhou_to_mjai(events: &[TenhouEvent]) -> Result<Vec<MjaiEvent>, String> {
    let mut out: Vec<MjaiEvent> = Vec::new();
    let mut last_tsumo_tile: [Option<u16>; 4] = [None; 4];

    out.push(MjaiEvent::StartGame { names: Vec::new() });

    for ev in events {
        match ev {
            TenhouEvent::GameMeta { .. } => {
                // 不直接转 mjai, 留 in StartGame
            }
            TenhouEvent::Init {
                seed,
                ten,
                oya,
                hais,
            } => {
                // 上一局未结束时先 EndKyoku
                if !out.is_empty()
                    && let Some(MjaiEvent::StartKyoku { .. }) = last_kyoku_marker(&out)
                {
                    out.push(MjaiEvent::EndKyoku);
                }

                let abs_kyoku = seed[0];
                let bakaze = match abs_kyoku / 4 {
                    0 => "E",
                    1 => "S",
                    2 => "W",
                    _ => "N",
                };
                let kyoku = (abs_kyoku % 4 + 1) as u8;
                let honba = seed[1] as u8;
                let riichi_sticks = seed[2] as u8;
                let dora_id = seed[5];
                let dora_marker = tile_id_to_mjai(dora_id)?;

                // 4 家初始手牌
                let mut tehais: [Vec<String>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
                for (i, slot) in tehais.iter_mut().enumerate() {
                    for &id in &hais[i] {
                        slot.push(tile_id_to_mjai(id)?);
                    }
                }

                // mjai scores 是绝对点数 (e.g. 25000), 天凤 ten 是 100s (250 = 25000)
                let scores: [i32; 4] = [ten[0] * 100, ten[1] * 100, ten[2] * 100, ten[3] * 100];

                out.push(MjaiEvent::StartKyoku {
                    bakaze: bakaze.into(),
                    kyoku,
                    honba,
                    riichi_sticks,
                    oya: *oya,
                    scores,
                    tehais,
                    dora_marker,
                });

                // reset 上次 tsumo 状态
                last_tsumo_tile = [None; 4];
            }
            TenhouEvent::Tsumo { who, tile_id } => {
                last_tsumo_tile[*who as usize] = Some(*tile_id);
                out.push(MjaiEvent::Tsumo {
                    actor: *who,
                    pai: tile_id_to_mjai(*tile_id)?,
                });
            }
            TenhouEvent::Dahai {
                who,
                tile_id,
                tsumogiri: _,
            } => {
                let tsumogiri = last_tsumo_tile[*who as usize] == Some(*tile_id);
                last_tsumo_tile[*who as usize] = None;
                out.push(MjaiEvent::Dahai {
                    actor: *who,
                    pai: tile_id_to_mjai(*tile_id)?,
                    tsumogiri,
                });
            }
            TenhouEvent::Naki { who, m } => {
                let who_seat = idx_to_seat(*who)?;
                let decoded = DecodedMeld::decode(*m, who_seat)
                    .map_err(|e| format!("Naki who={who} m={m}: {e}"))?;
                let from_seat = relative_to_seat(who_seat, decoded.from_relative);
                let from_idx = from_seat.index() as u8;
                let consumed_tiles = match &decoded.meld.kind {
                    MeldKind::Chi { tiles } | MeldKind::Pon { tiles } => tiles.to_vec(),
                    MeldKind::Minkan { tiles }
                    | MeldKind::Shouminkan { tiles }
                    | MeldKind::Ankan { tiles } => tiles.to_vec(),
                };

                match &decoded.meld.kind {
                    MeldKind::Chi { tiles } => {
                        // mjai chi: pai = called tile, consumed = 自家两张
                        // 我们的 chi 三张 tiles[0..2] 是顺子各位置, 不知道哪张是 called.
                        // 简化: 取 tiles[stolen_idx]. stolen_idx = (m >> 10) % 3.
                        let stolen_idx = ((*m >> 10) % 3) as usize;
                        let pai = tile_to_mjai_pai(tiles[stolen_idx]);
                        let consumed: Vec<String> = (0..3)
                            .filter(|&i| i != stolen_idx)
                            .map(|i| tile_to_mjai_pai(tiles[i]))
                            .collect();
                        out.push(MjaiEvent::Chi {
                            actor: *who,
                            target: from_idx,
                            pai,
                            consumed,
                        });
                    }
                    MeldKind::Pon { .. } => {
                        // mjai pon: pai = called tile, consumed = 自家两张 same kind
                        let stolen_idx = ((*m >> 9) % 3) as usize;
                        let pai = tile_to_mjai_pai(consumed_tiles[stolen_idx]);
                        let consumed: Vec<String> = (0..3)
                            .filter(|&i| i != stolen_idx)
                            .map(|i| tile_to_mjai_pai(consumed_tiles[i]))
                            .collect();
                        out.push(MjaiEvent::Pon {
                            actor: *who,
                            target: from_idx,
                            pai,
                            consumed,
                        });
                    }
                    MeldKind::Minkan { tiles } => {
                        let pai = tile_to_mjai_pai(tiles[0]);
                        let consumed: Vec<String> =
                            tiles.iter().skip(1).map(|t| tile_to_mjai_pai(*t)).collect();
                        out.push(MjaiEvent::Daiminkan {
                            actor: *who,
                            target: from_idx,
                            pai,
                            consumed,
                        });
                    }
                    MeldKind::Ankan { tiles } => {
                        let consumed: Vec<String> =
                            tiles.iter().map(|t| tile_to_mjai_pai(*t)).collect();
                        out.push(MjaiEvent::Ankan {
                            actor: *who,
                            consumed,
                        });
                    }
                    MeldKind::Shouminkan { tiles } => {
                        // 加杠: 第 4 张 = unused_offset 那张, mjai pai = 加杠那张.
                        let unused_offset = ((*m & 0b0110_0000) >> 5) as usize;
                        let added = tiles[unused_offset];
                        let consumed: Vec<String> = (0..4)
                            .filter(|&i| i != unused_offset)
                            .map(|i| tile_to_mjai_pai(tiles[i]))
                            .collect();
                        out.push(MjaiEvent::Kakan {
                            actor: *who,
                            pai: tile_to_mjai_pai(added),
                            consumed,
                        });
                    }
                }
            }
            TenhouEvent::Dora { tile_id } => {
                out.push(MjaiEvent::Dora {
                    dora_marker: tile_id_to_mjai(*tile_id)?,
                });
            }
            TenhouEvent::Reach { who, step, ten } => {
                if *step == 1 {
                    out.push(MjaiEvent::Reach { actor: *who });
                } else {
                    // step == 2: 立直成立, 棒被收
                    out.push(MjaiEvent::ReachAccepted {
                        actor: *who,
                        deltas: None,
                        scores: ten.map(|t| [t[0] * 100, t[1] * 100, t[2] * 100, t[3] * 100]),
                    });
                }
            }
            TenhouEvent::Agari {
                ba,
                hai: _,
                machi,
                ten,
                yaku,
                yakuman,
                dora_hai: _,
                ura_hai,
                who,
                from_who,
                sc,
            } => {
                // ten = [fu, points, limit_kind]
                let fu = ten.first().copied().unwrap_or(0) as u8;
                let points = ten.get(1).copied().unwrap_or(0);
                // han = sum(yaku[2*i+1])
                let total_han: u32 = yaku
                    .chunks(2)
                    .map(|c| c.get(1).copied().unwrap_or(0) as u32)
                    .sum();
                // yakus
                let mut yakus_out: Vec<MjaiYaku> = Vec::new();
                for chunk in yaku.chunks(2) {
                    if chunk.len() == 2 {
                        let id = chunk[0];
                        let han = chunk[1];
                        let name = tenhou_yaku_id_to_mjai(id)
                            .ok_or_else(|| format!("未知 tenhou yaku id {id}"))?;
                        yakus_out.push(MjaiYaku {
                            name: name.into(),
                            han,
                            fan: 0,
                        });
                    }
                }
                for &id in yakuman {
                    let name = tenhou_yaku_id_to_mjai(id)
                        .ok_or_else(|| format!("未知 yakuman id {id}"))?;
                    yakus_out.push(MjaiYaku {
                        name: name.into(),
                        han: 0,
                        fan: 0,
                    });
                }
                let yakuman_count = if yakuman.is_empty() {
                    0
                } else {
                    // 多个 yakuman id 计算: 单独不知道每个 +几; 简化只 count 个数
                    yakuman.len() as u8
                };
                // sc 是 [before0, delta0, before1, delta1, ...], 单位 100 点
                let mut deltas = [0i32; 4];
                for (i, d) in deltas.iter_mut().enumerate() {
                    *d = sc.get(i * 2 + 1).copied().unwrap_or(0) * 100;
                }
                let mut uradora_markers: Vec<String> = Vec::new();
                for &id in ura_hai {
                    uradora_markers.push(tile_id_to_mjai(id)?);
                }
                out.push(MjaiEvent::Hora {
                    actor: *who,
                    target: *from_who,
                    pai: tile_id_to_mjai(*machi)?,
                    uradora_markers,
                    deltas,
                    fu,
                    han: total_han as u8,
                    yakuman: yakuman_count,
                    hora_points: points,
                    yakus: yakus_out,
                });
                let _ = ba; // 本场已在 INIT 中含 honba
                out.push(MjaiEvent::EndKyoku);
            }
            TenhouEvent::Ryuukyoku {
                ba: _,
                sc,
                reason,
                hais,
            } => {
                let mut deltas = [0i32; 4];
                for (i, d) in deltas.iter_mut().enumerate() {
                    *d = sc.get(i * 2 + 1).copied().unwrap_or(0) * 100;
                }
                let tenpais: Vec<bool> = hais.iter().map(|h| h.is_some()).collect();
                out.push(MjaiEvent::Ryukyoku {
                    reason: reason.clone().unwrap_or_else(|| "fanpai".into()),
                    deltas,
                    tenpais,
                });
                out.push(MjaiEvent::EndKyoku);
            }
        }
    }

    out.push(MjaiEvent::EndGame { scores: Vec::new() });
    Ok(out)
}

fn tile_id_to_mjai(id: u16) -> Result<String, String> {
    let t = tenhou_id_to_tile(id)?;
    Ok(tile_to_mjai_pai(t))
}

fn idx_to_seat(i: u8) -> Result<Seat, String> {
    match i {
        0 => Ok(Seat::East),
        1 => Ok(Seat::South),
        2 => Ok(Seat::West),
        3 => Ok(Seat::North),
        _ => Err(format!("无效 seat idx {i}")),
    }
}

/// 找最近的 StartKyoku marker, 决定是否要插 EndKyoku.
fn last_kyoku_marker(events: &[MjaiEvent]) -> Option<&MjaiEvent> {
    for ev in events.iter().rev() {
        match ev {
            MjaiEvent::StartKyoku { .. } => return Some(ev),
            MjaiEvent::EndKyoku => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::tenhou_parser::parse_mjlog;

    #[test]
    fn convert_minimal_init_tsumo_dahai_ryuukyoku() {
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
        let tenhou_events = parse_mjlog(xml).unwrap();
        let mjai_events = tenhou_to_mjai(&tenhou_events).unwrap();
        assert!(
            mjai_events
                .iter()
                .any(|e| matches!(e, MjaiEvent::StartKyoku { .. }))
        );
        assert!(
            mjai_events
                .iter()
                .any(|e| matches!(e, MjaiEvent::Tsumo { actor: 0, .. }))
        );
        assert!(
            mjai_events
                .iter()
                .any(|e| matches!(e, MjaiEvent::Ryukyoku { .. }))
        );
        assert!(
            mjai_events
                .iter()
                .any(|e| matches!(e, MjaiEvent::EndGame { .. }))
        );
    }

    #[test]
    fn convert_agari_with_yakus() {
        let xml = r#"<mjloggm><INIT seed="0,0,0,3,5,114" ten="250,250,250,250" oya="0"
            hai0="86,123,1,40,93,82,82,55,12,24,79,113,33"
            hai1="51,76,124,93,127,108,3,2,118,22,16,17,15"
            hai2="62,131,99,57,121,7,42,77,68,46,4,11,10"
            hai3="48,49,5,28,8,73,9,98,80,38,30,6,37"/>
            <AGARI ba="0,0" hai="7,9,11,14,15,18,36,41,45,48,50,56,61,64"
                machi="61" ten="30,1100,0" yaku="0,1" doraHai="31"
                who="2" fromWho="2" sc="250,-5,250,-3,250,11,250,-3"/></mjloggm>"#;
        let tenhou_events = parse_mjlog(xml).unwrap();
        let mjai_events = tenhou_to_mjai(&tenhou_events).unwrap();
        let hora = mjai_events
            .iter()
            .find(|e| matches!(e, MjaiEvent::Hora { .. }))
            .unwrap();
        match hora {
            MjaiEvent::Hora {
                fu,
                han,
                yakus,
                hora_points,
                deltas,
                ..
            } => {
                assert_eq!(*fu, 30);
                assert_eq!(*han, 1);
                assert_eq!(*hora_points, 1100);
                assert_eq!(yakus.len(), 1);
                assert_eq!(yakus[0].name, "menzentsumo");
                // sc[5] = 11, * 100 = 1100
                assert_eq!(deltas[2], 1100);
                assert_eq!(deltas[0], -500);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn convert_round_starts_with_correct_bakaze() {
        // seed[0] = 4 = 南 1 局
        let xml = r#"<mjloggm><INIT seed="4,0,0,3,5,114" ten="250,250,250,250" oya="0"
            hai0="86,123,1,40,93,82,82,55,12,24,79,113,33"
            hai1="51,76,124,93,127,108,3,2,118,22,16,17,15"
            hai2="62,131,99,57,121,7,42,77,68,46,4,11,10"
            hai3="48,49,5,28,8,73,9,98,80,38,30,6,37"/></mjloggm>"#;
        let tenhou_events = parse_mjlog(xml).unwrap();
        let mjai_events = tenhou_to_mjai(&tenhou_events).unwrap();
        let sk = mjai_events
            .iter()
            .find(|e| matches!(e, MjaiEvent::StartKyoku { .. }))
            .unwrap();
        match sk {
            MjaiEvent::StartKyoku { bakaze, kyoku, .. } => {
                assert_eq!(bakaze, "S");
                assert_eq!(*kyoku, 1);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn convert_tsumogiri_detection() {
        // 摸 81, 切 81 = 摸切
        let xml = r#"<mjloggm><INIT seed="0,0,0,3,5,114" ten="250,250,250,250" oya="0"
            hai0="86,123,1,40,93,82,82,55,12,24,79,113,33"
            hai1="51,76,124,93,127,108,3,2,118,22,16,17,15"
            hai2="62,131,99,57,121,7,42,77,68,46,4,11,10"
            hai3="48,49,5,28,8,73,9,98,80,38,30,6,37"/>
            <T81/><D81/></mjloggm>"#;
        let tenhou_events = parse_mjlog(xml).unwrap();
        let mjai_events = tenhou_to_mjai(&tenhou_events).unwrap();
        let dahai = mjai_events
            .iter()
            .find(|e| matches!(e, MjaiEvent::Dahai { .. }))
            .unwrap();
        match dahai {
            MjaiEvent::Dahai { tsumogiri, .. } => {
                assert!(*tsumogiri, "摸 81 切 81 应是 tsumogiri");
            }
            _ => panic!(),
        }
    }

    /// _ unused 让 mod 不报 dead_code
    #[allow(dead_code)]
    fn _hint() {
        let _ = MeldKind::Chi {
            tiles: [
                Tile {
                    id: 0,
                    kind: tui_majo::tile::TileIndex(0),
                    red: false,
                },
                Tile {
                    id: 0,
                    kind: tui_majo::tile::TileIndex(0),
                    red: false,
                },
                Tile {
                    id: 0,
                    kind: tui_majo::tile::TileIndex(0),
                    red: false,
                },
            ],
        };
    }
}
