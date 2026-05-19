//! mjai NDJSON parser.
//!
//! mjai 是日麻 AI 比赛通用协议, 每行一个 JSON 事件. 协议参考:
//! <https://mjai.app/docs/mjai-protocol>
//!
//! 例:
//! ```json
//! {"type":"start_kyoku","bakaze":"E","kyoku":1,"honba":0,"riichi_sticks":0,
//!  "oya":0,"scores":[25000,25000,25000,25000],
//!  "tehais":[["1m",...],["1p",...],["1s",...],["E",...]],
//!  "dora_marker":"5p"}
//! {"type":"tsumo","actor":0,"pai":"5m"}
//! {"type":"dahai","actor":0,"pai":"5m","tsumogiri":true}
//! {"type":"hora","actor":0,"target":2,"pai":"7p","fu":40,"han":3,
//!  "hora_points":5200,"deltas":[5200,-1700,-1700,-1800],
//!  "yakus":[{"name":"riichi","han":1}],"uradora_markers":[]}
//! {"type":"ryukyoku","reason":"fanpai","deltas":[1500,-1500,1500,-1500],
//!  "tenpais":[true,false,true,false]}
//! ```

use serde::Deserialize;

/// mjai 事件 (tagged enum). 用 serde tag = "type".
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MjaiEvent {
    StartGame {
        #[serde(default)]
        names: Vec<String>,
    },
    StartKyoku {
        bakaze: String,
        kyoku: u8,
        honba: u8,
        #[serde(default)]
        riichi_sticks: u8,
        /// 庄家座位 0..=3 (East..North).
        oya: u8,
        scores: [i32; 4],
        /// 4 家初始手牌 (各 13 张).
        tehais: [Vec<String>; 4],
        dora_marker: String,
    },
    Tsumo {
        actor: u8,
        pai: String,
    },
    Dahai {
        actor: u8,
        pai: String,
        tsumogiri: bool,
    },
    Pon {
        actor: u8,
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Chi {
        actor: u8,
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Daiminkan {
        actor: u8,
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Ankan {
        actor: u8,
        consumed: Vec<String>,
    },
    Kakan {
        actor: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Reach {
        actor: u8,
    },
    ReachAccepted {
        actor: u8,
        #[serde(default)]
        deltas: Option<[i32; 4]>,
        #[serde(default)]
        scores: Option<[i32; 4]>,
    },
    Hora {
        actor: u8,
        target: u8,
        pai: String,
        #[serde(default)]
        uradora_markers: Vec<String>,
        deltas: [i32; 4],
        fu: u8,
        /// han 总数 (含 dora). 役満时可能 0 (用 yakuman 字段).
        #[serde(default)]
        han: u8,
        #[serde(default)]
        yakuman: u8,
        #[serde(default)]
        hora_points: i32,
        #[serde(default)]
        yakus: Vec<MjaiYaku>,
    },
    Ryukyoku {
        #[serde(default)]
        reason: String,
        deltas: [i32; 4],
        #[serde(default)]
        tenpais: Vec<bool>,
    },
    Dora {
        dora_marker: String,
    },
    EndKyoku,
    EndGame {
        #[serde(default)]
        scores: Vec<i32>,
    },
    /// 不行动响应 (mjai 玩家通过此跳过鸣牌窗口).
    None,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MjaiYaku {
    pub name: String,
    #[serde(default)]
    pub han: u8,
    #[serde(default)]
    pub fan: u8,
}

/// 解析整个 mjai NDJSON 文件 (整段字符串).
pub fn parse_mjai_log(contents: &str) -> Result<Vec<MjaiEvent>, String> {
    let mut out = Vec::new();
    for (line_no, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        match serde_json::from_str::<MjaiEvent>(trimmed) {
            Ok(ev) => out.push(ev),
            Err(e) => {
                return Err(format!(
                    "mjai parse error at line {}: {} (raw: {trimmed})",
                    line_no + 1,
                    e,
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_start_kyoku() {
        let line = r#"{"type":"start_kyoku","bakaze":"E","kyoku":1,"honba":0,"riichi_sticks":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","E","S","W","N"],["1p","2p","3p","4p","5p","6p","7p","8p","9p","E","S","W","N"],["1s","2s","3s","4s","5s","6s","7s","8s","9s","E","S","W","N"],["1m","1p","1s","E","S","W","N","P","F","C","2m","2p","2s"]],"dora_marker":"5p"}"#;
        let parsed = parse_mjai_log(line).unwrap();
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            MjaiEvent::StartKyoku {
                bakaze,
                kyoku,
                oya,
                tehais,
                dora_marker,
                ..
            } => {
                assert_eq!(bakaze, "E");
                assert_eq!(*kyoku, 1);
                assert_eq!(*oya, 0);
                assert_eq!(tehais[0].len(), 13);
                assert_eq!(dora_marker, "5p");
            }
            _ => panic!("expected StartKyoku"),
        }
    }

    #[test]
    fn parse_tsumo_dahai_sequence() {
        let log = r#"{"type":"tsumo","actor":0,"pai":"5m"}
{"type":"dahai","actor":0,"pai":"5m","tsumogiri":true}"#;
        let parsed = parse_mjai_log(log).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[0], MjaiEvent::Tsumo { actor: 0, .. }));
        assert!(matches!(
            parsed[1],
            MjaiEvent::Dahai {
                actor: 0,
                tsumogiri: true,
                ..
            }
        ));
    }

    #[test]
    fn parse_pon_chi_kan() {
        let log = r#"{"type":"pon","actor":1,"target":0,"pai":"5p","consumed":["5p","5p"]}
{"type":"chi","actor":2,"target":1,"pai":"3s","consumed":["1s","2s"]}
{"type":"daiminkan","actor":3,"target":2,"pai":"E","consumed":["E","E","E"]}
{"type":"ankan","actor":0,"consumed":["P","P","P","P"]}
{"type":"kakan","actor":1,"pai":"5p","consumed":["5p","5p","5p"]}"#;
        let parsed = parse_mjai_log(log).unwrap();
        assert_eq!(parsed.len(), 5);
    }

    #[test]
    fn parse_hora_with_yakus() {
        let line = r#"{"type":"hora","actor":0,"target":2,"pai":"7p","uradora_markers":["3s"],"deltas":[5200,-1700,-1700,-1800],"fu":40,"han":3,"hora_points":5200,"yakus":[{"name":"riichi","han":1},{"name":"tsumo","han":1},{"name":"dora","han":1}]}"#;
        let parsed = parse_mjai_log(line).unwrap();
        match &parsed[0] {
            MjaiEvent::Hora { fu, han, yakus, .. } => {
                assert_eq!(*fu, 40);
                assert_eq!(*han, 3);
                assert_eq!(yakus.len(), 3);
                assert_eq!(yakus[0].name, "riichi");
                assert_eq!(yakus[0].han, 1);
            }
            _ => panic!("expected Hora"),
        }
    }

    #[test]
    fn parse_ryukyoku() {
        let line = r#"{"type":"ryukyoku","reason":"fanpai","deltas":[1500,-1500,1500,-1500],"tenpais":[true,false,true,false]}"#;
        let parsed = parse_mjai_log(line).unwrap();
        match &parsed[0] {
            MjaiEvent::Ryukyoku {
                reason,
                deltas,
                tenpais,
            } => {
                assert_eq!(reason, "fanpai");
                assert_eq!(deltas[0], 1500);
                assert_eq!(tenpais.len(), 4);
            }
            _ => panic!("expected Ryukyoku"),
        }
    }

    #[test]
    fn parse_yakuman_no_han() {
        // 役満 (国士无双) 通常 yakuman=1, han=0
        let line = r#"{"type":"hora","actor":0,"target":0,"pai":"E","uradora_markers":[],"deltas":[48000,-16000,-16000,-16000],"fu":30,"yakuman":1,"yakus":[{"name":"kokushimusou"}]}"#;
        let parsed = parse_mjai_log(line).unwrap();
        match &parsed[0] {
            MjaiEvent::Hora { han, yakuman, .. } => {
                assert_eq!(*han, 0);
                assert_eq!(*yakuman, 1);
            }
            _ => panic!("expected Hora"),
        }
    }

    #[test]
    fn skip_empty_and_comment_lines() {
        let log = r#"
// 这是注释
{"type":"none"}

{"type":"end_kyoku"}
"#;
        let parsed = parse_mjai_log(log).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn invalid_json_returns_error() {
        let log = r#"{"type":"tsumo","actor":0
{"type":"dahai"}"#;
        assert!(parse_mjai_log(log).is_err());
    }
}
