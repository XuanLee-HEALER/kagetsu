//! 网络协议消息定义.
//!
//! Client ↔ Server 用 JSON over WebSocket. 协议本身不做加密 / 鉴权 (LAN
//! 假设可信). 互联网模式后续再加 TLS + 房间密码.
//!
//! ## 类型分组
//!
//! - [`ClientMsg`] / [`ServerMsg`] —— 顶层消息
//! - [`NetAction`] —— 游戏内动作 (client 上报)
//! - [`RoomView`] / [`PlayerSlot`] / [`RoomLifecycle`] —— 房间状态
//! - [`GameStateView`] / [`PlayerView`] —— server 投影给某 client 的视图
//! - [`RoundResultView`] / [`GameOverView`] —— 局/庄结算

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::domain::meld::{Meld, Seat};
use crate::engine::domain::tile::{Tile, TileIndex};
use crate::engine::event::GameEvent;
use crate::engine::phase::Phase;
use crate::engine::round_state::RoundWind;
use crate::engine::rules::GameRules;
use crate::engine::score::Ranking;

// ============================================================================
// TileSpec — 网络协议层的牌种说明符
// ============================================================================

/// 牌种说明符: 接受 "5p" / "p5" / "五筒" / "東" 等. 由 `NetAction::Discard/Riichi`
/// 协议使用.
///
/// 历史上定义在 `ui::screens::game`, 但本身没有 ratatui 依赖, 是纯协议域类型.
/// 解 net → ui 反向耦合时上移至此, 便于 kagetsu-core / kagetsu 分 crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileSpec {
    pub kind: TileIndex,
}

impl TileSpec {
    pub fn matches(&self, k: TileIndex) -> bool {
        self.kind == k
    }
}

/// 接受的牌输入:
/// - ASCII: "5p" / "p5" / "5m" / "9s" / "1z"-"7z" (z = 字牌, 1=東 .. 7=中)
/// - 中文数字 + 花色: "五筒" / "三索" / "九萬"
/// - 字牌: "东南西北白发中" / "東南西北白發中"
pub fn parse_tile_spec(s: &str) -> Option<TileSpec> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // 单字符字牌
    let kind = match s {
        "東" | "东" | "1z" | "z1" => Some(TileIndex::EAST),
        "南" | "2z" | "z2" => Some(TileIndex::SOUTH),
        "西" | "3z" | "z3" => Some(TileIndex::WEST),
        "北" | "4z" | "z4" => Some(TileIndex::NORTH),
        "白" | "5z" | "z5" => Some(TileIndex::HAKU),
        "發" | "发" | "6z" | "z6" => Some(TileIndex::HATSU),
        "中" | "7z" | "z7" => Some(TileIndex::CHUN),
        _ => None,
    };
    if let Some(k) = kind {
        return Some(TileSpec { kind: k });
    }
    // ASCII 数字 + 花色 (5p / p5)
    let ascii_lo = s.to_lowercase();
    let (n, suit) = parse_num_suit_ascii(&ascii_lo)?;
    if !(1..=9).contains(&n) {
        return None;
    }
    let base = match suit {
        'm' => 0u8,
        'p' => 9,
        's' => 18,
        _ => return None,
    };
    Some(TileSpec {
        kind: TileIndex(base + (n - 1) as u8),
    })
}

fn parse_num_suit_ascii(s: &str) -> Option<(u32, char)> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() != 2 {
        // 试中文数字 + 花色 (e.g. "五筒")
        return parse_cn_num_suit(s);
    }
    let (a, b) = (chars[0], chars[1]);
    if a.is_ascii_digit() {
        Some((a.to_digit(10)?, b))
    } else if b.is_ascii_digit() {
        Some((b.to_digit(10)?, a))
    } else {
        parse_cn_num_suit(s)
    }
}

fn parse_cn_num_suit(s: &str) -> Option<(u32, char)> {
    const CN_NUM: &[(&str, u32)] = &[
        ("一", 1),
        ("二", 2),
        ("三", 3),
        ("四", 4),
        ("五", 5),
        ("六", 6),
        ("七", 7),
        ("八", 8),
        ("九", 9),
    ];
    for (cn, n) in CN_NUM {
        if let Some(rest) = s.strip_prefix(cn) {
            let suit = match rest {
                "萬" | "万" => 'm',
                "筒" | "饼" => 'p',
                "索" | "条" => 's',
                _ => return None,
            };
            return Some((*n, suit));
        }
    }
    None
}

// ============================================================================
// Client → Server
// ============================================================================

/// Client 发给 server 的消息.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// 加入房间. `reconnect_token` 非空表示带 token 重连.
    Join {
        nickname: String,
        reconnect_token: Option<Uuid>,
    },
    /// 切换准备状态 (lobby 阶段).
    Ready { ready: bool },
    /// 房主开始游戏.
    StartGame,
    /// 房主修改房间配置 (仅 lobby 阶段生效).
    UpdateRules(GameRules),
    /// 游戏中提交动作.
    Action(NetAction),
    /// RoundEnd 选择回房间 (改配置).
    BackToRoom,
    /// RoundEnd 选择继续 (用旧配置开新一庄).
    ContinueGame,
    /// 主动离开 (房主离开 = 解散).
    Leave,
    /// 心跳应答.
    Pong { id: u64 },
}

/// 游戏内玩家动作.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetAction {
    Discard(TileSpec),
    Riichi(TileSpec),
    Tsumo,
    Pon,
    /// 多种吃法: 选 chi_options[idx].
    Chi(usize),
    Minkan,
    Ankan(TileIndex),
    Shouminkan(TileIndex),
    /// 跳过响应他家弃牌.
    Pass,
    /// RoundEnd 时按 N 推进下一局.
    NextRound,
}

// ============================================================================
// Server → Client
// ============================================================================

/// Server 发给 client 的消息.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Join 成功后第一条消息. 含 reconnect_token (持久化以便重连).
    Welcome {
        player_id: u32,
        reconnect_token: Uuid,
        room: Box<RoomView>,
    },
    /// 房间状态更新 (lobby 阶段广播 / 玩家加入退出 / 配置改).
    RoomUpdate(Box<RoomView>),
    /// 游戏状态更新 (每个 client 拿不同 view, 隐藏他家手牌).
    GameStateView(Box<GameStateView>),
    /// 提示 client 当前可执行的动作 + 截止时刻.
    ActionRequired {
        hints: Vec<NetAction>,
        deadline_unix_ms: i64,
    },
    /// 一局结算结果.
    RoundResult(RoundResultView),
    /// 整庄结束.
    GameEnd(GameOverView),
    /// 回房间 (有人退出 / 玩家选择回房间).
    BackToRoom,
    /// 错误 / 拒绝原因 (e.g. "房主已离开" / "房间已满" / "无效操作").
    Error { message: String },
    /// 心跳.
    Ping { id: u64 },
    /// ZeroTrust 模式开局信号 (M5.B.8). 房主决定 StartGame + mode=ZeroTrust 时,
    /// RoomActor 给 4 个真人玩家各发一条 MpStart, own_index 不同, 其他字段一致.
    /// client 收到后 spawn MpPlayerActor(cfg) 接管协议层, RoomActor 退到旁观.
    MpStart {
        /// 4 玩家 PeerId 字节 (按 own_index 0..3 顺序).
        all_peer_ids: Vec<Vec<u8>>,
        /// 本 client 在 4 玩家中的 own_index.
        own_index: u32,
        /// 4 方共享的 session_label = hash(room_id || sorted_peer_ids).
        session_label: Vec<u8>,
        /// 牌山大小 (常规一手 = 136).
        deck_size: u32,
        /// Cut-and-Choose K (协议 1 安全参数, 默认 80).
        cnc_k_rounds: u32,
    },
}

// ============================================================================
// 房间相关
// ============================================================================

/// 房间生命周期.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomLifecycle {
    /// 准备中, 玩家可加入/退出/ready/改配置.
    Lobby,
    /// 游戏中.
    InGame,
    /// 一庄结束, 玩家选 BackToRoom 或 ContinueGame.
    GameEnd,
}

/// 房间状态 (lobby 广播 / 重连恢复用).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomView {
    pub room_id: String,
    pub host_id: u32,
    pub config: GameRules,
    pub players: Vec<PlayerSlot>,
    pub state: RoomLifecycle,
    /// 房间信任模式 (M5.B.2). 老 schema 没此字段时 default = Standard.
    #[serde(default)]
    pub mode: crate::net::p2p::RoomMode,
}

/// 房间内某个 slot. 含 AI slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSlot {
    pub id: u32,
    pub nickname: String,
    pub ready: bool,
    /// 开局后 server 分配, lobby 阶段为 None.
    pub seat: Option<Seat>,
    pub is_ai: bool,
    pub is_host: bool,
    pub connected: bool,
}

// ============================================================================
// 游戏状态视图 (server 投影给 client)
// ============================================================================

/// 服务器投给某 client 的当前游戏状态视图. 隐藏他家手牌内容.
///
/// 注: 复杂内嵌字段 (RoundResultView/GameOverView) 在 phase 3 (RoomActor)
/// 实施时再具体填充, 此处定义最小骨架.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameStateView {
    pub round_wind: RoundWind,
    pub kyoku: u8,
    pub honba: u8,
    pub riichi_sticks: u32,
    pub dealer: Seat,
    pub turn: Seat,
    pub phase: Phase,
    /// 收消息这一方所在的座位.
    pub my_seat: Seat,
    /// 自家手牌 (含 last_drawn).
    pub my_hand: Vec<Tile>,
    pub my_last_drawn: Option<Tile>,
    /// 4 家公开信息 (含自家, 但 hand_count 字段忽略, 看 my_hand).
    pub players: [PlayerView; 4],
    /// 牌山剩余张数.
    pub wall_remaining: usize,
    /// 已揭开的 dora 表牌.
    pub dora_indicators: Vec<Tile>,
    /// 最近 ~20 条事件 (用于渲染 last 行).
    pub events: Vec<GameEvent>,
}

/// 4 家公开信息. 自家手牌细节看 [`GameStateView::my_hand`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerView {
    pub seat: Seat,
    pub nickname: String,
    pub score: i32,
    /// 他家手牌张数 (▒▒ × N). 自家忽略此字段.
    pub hand_count: usize,
    pub melds: Vec<Meld>,
    pub river: Vec<Tile>,
    pub riichi: bool,
    /// 立直时弃出的牌在 river 里的索引 (用于横置渲染).
    pub riichi_river_idx: Option<usize>,
}

// ============================================================================
// 结算视图
// ============================================================================

/// 一局结算结果 (和牌 / 流局).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundResultView {
    /// 简化 MVP: 直接把渲染需要的字段传过去, 后续 phase 再细化.
    pub message: String,
    /// 局后各家分数.
    pub scores: [i32; 4],
}

/// 整庄结束.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameOverView {
    pub rankings: [Ranking; 4],
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rules::{GameRules, LengthRule};

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_string(value).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn client_msg_join_round_trip() {
        let msg = ClientMsg::Join {
            nickname: "Alice".into(),
            reconnect_token: Some(Uuid::nil()),
        };
        let back = round_trip(&msg);
        assert!(matches!(back, ClientMsg::Join { .. }));
    }

    #[test]
    fn client_msg_action_round_trip() {
        let msg = ClientMsg::Action(NetAction::Discard(TileSpec {
            kind: TileIndex(13),
        }));
        let back = round_trip(&msg);
        assert!(matches!(back, ClientMsg::Action(NetAction::Discard(_))));
    }

    #[test]
    fn server_msg_welcome_round_trip() {
        let config = GameRules {
            length: LengthRule::Tonpuusen,
            ..Default::default()
        };
        let msg = ServerMsg::Welcome {
            player_id: 1,
            reconnect_token: Uuid::nil(),
            room: Box::new(RoomView {
                room_id: "test-room".into(),
                host_id: 1,
                config,
                players: vec![PlayerSlot {
                    id: 1,
                    nickname: "host".into(),
                    ready: true,
                    seat: None,
                    is_ai: false,
                    is_host: true,
                    connected: true,
                }],
                state: RoomLifecycle::Lobby,
                mode: crate::net::p2p::RoomMode::Standard,
            }),
        };
        let back = round_trip(&msg);
        match back {
            ServerMsg::Welcome {
                player_id, room, ..
            } => {
                assert_eq!(player_id, 1);
                assert_eq!(room.players.len(), 1);
                assert!(matches!(room.config.length, LengthRule::Tonpuusen));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn room_lifecycle_serialize_snake_case() {
        let s = serde_json::to_string(&RoomLifecycle::InGame).unwrap();
        assert_eq!(s, "\"in_game\"");
    }

    #[test]
    fn net_action_pass_round_trip() {
        let msg = NetAction::Pass;
        let back = round_trip(&msg);
        assert!(matches!(back, NetAction::Pass));
    }

    #[test]
    fn client_msg_join_with_token_round_trip() {
        let token = Uuid::new_v4();
        let msg = ClientMsg::Join {
            nickname: "Alice".into(),
            reconnect_token: Some(token),
        };
        let back = round_trip(&msg);
        match back {
            ClientMsg::Join {
                nickname,
                reconnect_token,
            } => {
                assert_eq!(nickname, "Alice");
                assert_eq!(reconnect_token, Some(token));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn client_msg_ready_round_trip() {
        let msg = ClientMsg::Ready { ready: true };
        let back = round_trip(&msg);
        match back {
            ClientMsg::Ready { ready } => assert!(ready),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn client_msg_pong_round_trip() {
        let msg = ClientMsg::Pong { id: 42 };
        let back = round_trip(&msg);
        match back {
            ClientMsg::Pong { id } => assert_eq!(id, 42),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn client_msg_uses_snake_case_tag() {
        let msg = ClientMsg::StartGame;
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"start_game\""));
    }

    #[test]
    fn client_msg_back_to_room_continue_distinguishable() {
        let a = ClientMsg::BackToRoom;
        let b = ClientMsg::ContinueGame;
        let sa = serde_json::to_string(&a).unwrap();
        let sb = serde_json::to_string(&b).unwrap();
        assert_ne!(sa, sb);
    }

    #[test]
    fn net_action_kan_variants_round_trip() {
        let m1 = NetAction::Ankan(TileIndex(0));
        let m2 = NetAction::Shouminkan(TileIndex(33));
        let m3 = NetAction::Minkan;
        let b1 = round_trip(&m1);
        let b2 = round_trip(&m2);
        let b3 = round_trip(&m3);
        match b1 {
            NetAction::Ankan(k) => assert_eq!(k, TileIndex(0)),
            _ => panic!(),
        }
        match b2 {
            NetAction::Shouminkan(k) => assert_eq!(k, TileIndex(33)),
            _ => panic!(),
        }
        assert!(matches!(b3, NetAction::Minkan));
    }

    #[test]
    fn net_action_chi_with_index_round_trip() {
        let msg = NetAction::Chi(2);
        let back = round_trip(&msg);
        match back {
            NetAction::Chi(i) => assert_eq!(i, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn room_lifecycle_all_variants_serde() {
        for v in [
            RoomLifecycle::Lobby,
            RoomLifecycle::InGame,
            RoomLifecycle::GameEnd,
        ] {
            let s = serde_json::to_string(&v).unwrap();
            let back: RoomLifecycle = serde_json::from_str(&s).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn parse_tile_spec_variants() {
        assert_eq!(parse_tile_spec("5p").unwrap().kind, TileIndex(13));
        assert_eq!(parse_tile_spec("p5").unwrap().kind, TileIndex(13));
        assert_eq!(parse_tile_spec("9m").unwrap().kind, TileIndex(8));
        assert_eq!(parse_tile_spec("1s").unwrap().kind, TileIndex(18));
        assert_eq!(parse_tile_spec("中").unwrap().kind, TileIndex::CHUN);
        assert_eq!(parse_tile_spec("发").unwrap().kind, TileIndex::HATSU);
        assert!(parse_tile_spec("0p").is_none());
        assert!(parse_tile_spec("xx").is_none());
    }
}
