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

use crate::config::GameConfig;
use crate::game::{GameEvent, Phase, RoundWind};
use crate::domain::meld::{Meld, Seat};
use crate::score::Ranking;
use crate::domain::tile::{Tile, TileIndex};
use crate::ui::screens::game::TileSpec;

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
    UpdateConfig(GameConfig),
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
    pub config: GameConfig,
    pub players: Vec<PlayerSlot>,
    pub state: RoomLifecycle,
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
    pub riichi_sticks: u8,
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
    use crate::config::{GameConfig, LengthRule};

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
        let config = GameConfig {
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
}
