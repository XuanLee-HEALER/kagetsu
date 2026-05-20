//! ReplayLog: 中性中间表示, 解耦 mjai/天凤等格式.
//!
//! 后续可加 `tenhou_to_replay_log` / `paipu_to_replay_log` 都转成这个 IR,
//! ReplayDriver 只对 ReplayLog 一种输入.

use kagetsu_core::engine::domain::meld::Seat;
use kagetsu_core::engine::domain::tile::Tile;
use kagetsu_core::engine::round_state::RoundWind;

use super::mjai_pai::parse_mjai_pai;
use super::mjai_parser::{MjaiEvent, MjaiYaku};

#[derive(Debug, Clone)]
pub struct ReplayLog {
    pub source: String,
    pub kyokus: Vec<KyokuLog>,
    pub final_scores: Option<[i32; 4]>,
}

#[derive(Debug, Clone)]
pub struct KyokuLog {
    pub round_wind: RoundWind,
    pub kyoku: u8,
    pub honba: u8,
    pub riichi_sticks: u8,
    pub dealer: Seat,
    pub initial_hands: [Vec<Tile>; 4],
    pub initial_scores: [i32; 4],
    pub initial_dora_marker: Tile,
    pub events: Vec<KyokuEvent>,
    pub result: Option<KyokuResult>,
}

#[derive(Debug, Clone)]
pub enum KyokuEvent {
    Tsumo {
        who: Seat,
        tile: Tile,
    },
    Dahai {
        who: Seat,
        tile: Tile,
        tsumogiri: bool,
    },
    Pon {
        who: Seat,
        from: Seat,
        target: Tile,
        consumed: [Tile; 2],
    },
    Chi {
        who: Seat,
        from: Seat,
        target: Tile,
        consumed: [Tile; 2],
    },
    Daiminkan {
        who: Seat,
        from: Seat,
        target: Tile,
        consumed: [Tile; 3],
    },
    Ankan {
        who: Seat,
        consumed: [Tile; 4],
    },
    Kakan {
        who: Seat,
        target: Tile,
        consumed: Vec<Tile>,
    },
    Reach {
        who: Seat,
    },
    ReachAccepted {
        who: Seat,
    },
    Dora {
        tile: Tile,
    },
}

#[derive(Debug, Clone)]
pub enum KyokuResult {
    Hora {
        winner: Seat,
        from: Seat,
        winning_tile: Tile,
        han: u8,
        yakuman: u8,
        fu: u8,
        points: i32,
        deltas: [i32; 4],
        yakus: Vec<(String, u8)>,
        uradora_markers: Vec<Tile>,
    },
    Ryukyoku {
        reason: String,
        deltas: [i32; 4],
        tenpais: Vec<bool>,
    },
}

/// 从 mjai 事件流构建 ReplayLog.
///
/// 假设事件顺序:
/// `[StartGame] StartKyoku event* (Hora|Ryukyoku) EndKyoku ... EndGame`
pub fn build_replay_log(events: Vec<MjaiEvent>) -> Result<ReplayLog, String> {
    let mut log = ReplayLog {
        source: "mjai".into(),
        kyokus: Vec::new(),
        final_scores: None,
    };
    let mut current: Option<KyokuLog> = None;
    // 简化的 unique id 生成器 (Tile.id 在我们的 GameState 内只用于 hand 查找,
    // 不要求与真实 wall 物理 id 一致).
    let mut next_id: u16 = 0;
    let mut alloc_id = || {
        let i = next_id;
        next_id = next_id.wrapping_add(1);
        i
    };

    for ev in events {
        match ev {
            MjaiEvent::StartGame { .. } => {
                // 仅元信息, 忽略.
            }
            MjaiEvent::StartKyoku {
                bakaze,
                kyoku,
                honba,
                riichi_sticks,
                oya,
                scores,
                tehais,
                dora_marker,
            } => {
                if current.is_some() {
                    return Err("StartKyoku 时 上一局未 EndKyoku".into());
                }
                let dealer = idx_to_seat(oya)?;
                let round_wind = parse_bakaze(&bakaze)?;
                let initial_dora_marker = parse_mjai_pai(&dora_marker, alloc_id())?;

                let mut hands: [Vec<Tile>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
                for (i, tehai) in tehais.iter().enumerate() {
                    if tehai.len() != 13 {
                        return Err(format!("seat {i} 初始手牌应 13 张, 实际 {}", tehai.len()));
                    }
                    for s in tehai {
                        hands[i].push(parse_mjai_pai(s, alloc_id())?);
                    }
                }

                current = Some(KyokuLog {
                    round_wind,
                    kyoku,
                    honba,
                    riichi_sticks,
                    dealer,
                    initial_hands: hands,
                    initial_scores: scores,
                    initial_dora_marker,
                    events: Vec::new(),
                    result: None,
                });
            }
            MjaiEvent::Tsumo { actor, pai } => {
                let k = current
                    .as_mut()
                    .ok_or("Tsumo 在 StartKyoku 之前".to_string())?;
                k.events.push(KyokuEvent::Tsumo {
                    who: idx_to_seat(actor)?,
                    tile: parse_mjai_pai(&pai, alloc_id())?,
                });
            }
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                let k = current
                    .as_mut()
                    .ok_or("Dahai 在 StartKyoku 之前".to_string())?;
                k.events.push(KyokuEvent::Dahai {
                    who: idx_to_seat(actor)?,
                    tile: parse_mjai_pai(&pai, alloc_id())?,
                    tsumogiri,
                });
            }
            MjaiEvent::Pon {
                actor,
                target,
                pai,
                consumed,
            } => {
                let k = current
                    .as_mut()
                    .ok_or("Pon 在 StartKyoku 之前".to_string())?;
                let consumed_arr = parse_n_consumed::<2>(&consumed, &mut alloc_id)?;
                k.events.push(KyokuEvent::Pon {
                    who: idx_to_seat(actor)?,
                    from: idx_to_seat(target)?,
                    target: parse_mjai_pai(&pai, alloc_id())?,
                    consumed: consumed_arr,
                });
            }
            MjaiEvent::Chi {
                actor,
                target,
                pai,
                consumed,
            } => {
                let k = current
                    .as_mut()
                    .ok_or("Chi 在 StartKyoku 之前".to_string())?;
                let consumed_arr = parse_n_consumed::<2>(&consumed, &mut alloc_id)?;
                k.events.push(KyokuEvent::Chi {
                    who: idx_to_seat(actor)?,
                    from: idx_to_seat(target)?,
                    target: parse_mjai_pai(&pai, alloc_id())?,
                    consumed: consumed_arr,
                });
            }
            MjaiEvent::Daiminkan {
                actor,
                target,
                pai,
                consumed,
            } => {
                let k = current
                    .as_mut()
                    .ok_or("Daiminkan 在 StartKyoku 之前".to_string())?;
                let consumed_arr = parse_n_consumed::<3>(&consumed, &mut alloc_id)?;
                k.events.push(KyokuEvent::Daiminkan {
                    who: idx_to_seat(actor)?,
                    from: idx_to_seat(target)?,
                    target: parse_mjai_pai(&pai, alloc_id())?,
                    consumed: consumed_arr,
                });
            }
            MjaiEvent::Ankan { actor, consumed } => {
                let k = current
                    .as_mut()
                    .ok_or("Ankan 在 StartKyoku 之前".to_string())?;
                let consumed_arr = parse_n_consumed::<4>(&consumed, &mut alloc_id)?;
                k.events.push(KyokuEvent::Ankan {
                    who: idx_to_seat(actor)?,
                    consumed: consumed_arr,
                });
            }
            MjaiEvent::Kakan {
                actor,
                pai,
                consumed,
            } => {
                let k = current
                    .as_mut()
                    .ok_or("Kakan 在 StartKyoku 之前".to_string())?;
                let mut tiles = Vec::with_capacity(consumed.len());
                for s in &consumed {
                    tiles.push(parse_mjai_pai(s, alloc_id())?);
                }
                k.events.push(KyokuEvent::Kakan {
                    who: idx_to_seat(actor)?,
                    target: parse_mjai_pai(&pai, alloc_id())?,
                    consumed: tiles,
                });
            }
            MjaiEvent::Reach { actor } => {
                let k = current
                    .as_mut()
                    .ok_or("Reach 在 StartKyoku 之前".to_string())?;
                k.events.push(KyokuEvent::Reach {
                    who: idx_to_seat(actor)?,
                });
            }
            MjaiEvent::ReachAccepted { actor, .. } => {
                let k = current
                    .as_mut()
                    .ok_or("ReachAccepted 在 StartKyoku 之前".to_string())?;
                k.events.push(KyokuEvent::ReachAccepted {
                    who: idx_to_seat(actor)?,
                });
            }
            MjaiEvent::Dora { dora_marker } => {
                let k = current
                    .as_mut()
                    .ok_or("Dora 在 StartKyoku 之前".to_string())?;
                k.events.push(KyokuEvent::Dora {
                    tile: parse_mjai_pai(&dora_marker, alloc_id())?,
                });
            }
            MjaiEvent::Hora {
                actor,
                target,
                pai,
                uradora_markers,
                deltas,
                fu,
                han,
                yakuman,
                hora_points,
                yakus,
            } => {
                // 局可能在 StartKyoku 之前找不到 (e.g. 多荣的第二个 Hora 在前一局
                // EndKyoku 之后). 简化: 找不到就忽略 (双荣只保留第一个 winner).
                let Some(k) = current.as_mut() else {
                    continue;
                };
                // 已有 result (e.g. 双荣第二个) → 忽略
                if k.result.is_some() {
                    continue;
                }
                let mut ura = Vec::with_capacity(uradora_markers.len());
                for s in &uradora_markers {
                    ura.push(parse_mjai_pai(s, alloc_id())?);
                }
                k.result = Some(KyokuResult::Hora {
                    winner: idx_to_seat(actor)?,
                    from: idx_to_seat(target)?,
                    winning_tile: parse_mjai_pai(&pai, alloc_id())?,
                    han,
                    yakuman,
                    fu,
                    points: hora_points,
                    deltas,
                    yakus: yakus.into_iter().map(yaku_to_pair).collect(),
                    uradora_markers: ura,
                });
            }
            MjaiEvent::Ryukyoku {
                reason,
                deltas,
                tenpais,
            } => {
                let k = current
                    .as_mut()
                    .ok_or("Ryukyoku 在 StartKyoku 之前".to_string())?;
                k.result = Some(KyokuResult::Ryukyoku {
                    reason,
                    deltas,
                    tenpais,
                });
            }
            MjaiEvent::EndKyoku => {
                if let Some(k) = current.take() {
                    log.kyokus.push(k);
                }
            }
            MjaiEvent::EndGame { scores } => {
                if scores.len() == 4 {
                    log.final_scores = Some([scores[0], scores[1], scores[2], scores[3]]);
                }
            }
            MjaiEvent::None => {}
        }
    }

    if let Some(k) = current.take() {
        // 没明确 EndKyoku 也接受 (有些 mjai 实现省略)
        log.kyokus.push(k);
    }
    Ok(log)
}

fn idx_to_seat(i: u8) -> Result<Seat, String> {
    match i {
        0 => Ok(Seat::East),
        1 => Ok(Seat::South),
        2 => Ok(Seat::West),
        3 => Ok(Seat::North),
        _ => Err(format!("无效座位 idx {i}")),
    }
}

fn parse_bakaze(s: &str) -> Result<RoundWind, String> {
    match s {
        "E" => Ok(RoundWind::East),
        "S" => Ok(RoundWind::South),
        "W" => Ok(RoundWind::West),
        "N" => Ok(RoundWind::North),
        _ => Err(format!("无效 bakaze '{s}'")),
    }
}

fn parse_n_consumed<const N: usize>(
    strs: &[String],
    alloc_id: &mut impl FnMut() -> u16,
) -> Result<[Tile; N], String> {
    if strs.len() != N {
        return Err(format!("consumed 长度应为 {N}, 实际 {}", strs.len()));
    }
    let mut out: Vec<Tile> = Vec::with_capacity(N);
    for s in strs {
        out.push(parse_mjai_pai(s, alloc_id())?);
    }
    out.try_into()
        .map_err(|_| format!("consumed 转 [Tile; {N}] 失败"))
}

fn yaku_to_pair(y: MjaiYaku) -> (String, u8) {
    let han = if y.han > 0 { y.han } else { y.fan };
    (y.name, han)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::mjai_parser::parse_mjai_log;

    /// 极简一局: start_kyoku → tsumo → dahai → ryukyoku → end_kyoku.
    #[test]
    fn build_simple_kyoku() {
        let log = r#"{"type":"start_kyoku","bakaze":"E","kyoku":1,"honba":0,"riichi_sticks":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","E","S","W","N"],["1p","2p","3p","4p","5p","6p","7p","8p","9p","E","S","W","N"],["1s","2s","3s","4s","5s","6s","7s","8s","9s","E","S","W","N"],["1m","1p","1s","E","S","W","N","P","F","C","2m","2p","2s"]],"dora_marker":"5p"}
{"type":"tsumo","actor":0,"pai":"5m"}
{"type":"dahai","actor":0,"pai":"5m","tsumogiri":true}
{"type":"ryukyoku","reason":"fanpai","deltas":[1500,-1500,1500,-1500],"tenpais":[true,false,true,false]}
{"type":"end_kyoku"}"#;
        let evs = parse_mjai_log(log).unwrap();
        let replay = build_replay_log(evs).unwrap();
        assert_eq!(replay.kyokus.len(), 1);
        let k = &replay.kyokus[0];
        assert_eq!(k.kyoku, 1);
        assert_eq!(k.dealer, Seat::East);
        assert_eq!(k.initial_hands[0].len(), 13);
        assert_eq!(k.events.len(), 2); // tsumo + dahai
        match &k.result {
            Some(KyokuResult::Ryukyoku { reason, deltas, .. }) => {
                assert_eq!(reason, "fanpai");
                assert_eq!(deltas[0], 1500);
            }
            _ => panic!("expected Ryukyoku"),
        }
    }

    #[test]
    fn build_hora_kyoku() {
        let log = r#"{"type":"start_kyoku","bakaze":"E","kyoku":1,"honba":0,"riichi_sticks":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","E","S","W","N"],["1p","2p","3p","4p","5p","6p","7p","8p","9p","E","S","W","N"],["1s","2s","3s","4s","5s","6s","7s","8s","9s","E","S","W","N"],["1m","1p","1s","E","S","W","N","P","F","C","2m","2p","2s"]],"dora_marker":"5p"}
{"type":"tsumo","actor":0,"pai":"6m"}
{"type":"hora","actor":0,"target":0,"pai":"6m","uradora_markers":[],"deltas":[2000,-1000,-1000,0],"fu":30,"han":1,"hora_points":2000,"yakus":[{"name":"tsumo","han":1}]}
{"type":"end_kyoku"}"#;
        let evs = parse_mjai_log(log).unwrap();
        let replay = build_replay_log(evs).unwrap();
        match &replay.kyokus[0].result {
            Some(KyokuResult::Hora {
                winner,
                from,
                han,
                fu,
                points,
                yakus,
                ..
            }) => {
                assert_eq!(*winner, Seat::East);
                assert_eq!(*from, Seat::East); // 自摸 from = winner
                assert_eq!(*han, 1);
                assert_eq!(*fu, 30);
                assert_eq!(*points, 2000);
                assert_eq!(yakus.len(), 1);
                assert_eq!(yakus[0].0, "tsumo");
            }
            _ => panic!("expected Hora"),
        }
    }
}
