//! WebSocket server (房主端) + remote client bridge (加入者端).
//!
//! ## 房主端
//! [`spawn_ws_server`] 接受一个 [`RoomHandle`] 与可选监听端口, 在 tokio
//! runtime 起一个 axum server, 路径 `/ws` 处理 WebSocket 升级. 每个 ws 连接
//! spawn 一个 task, 协议:
//! 1. client 第一条消息必须是 `ClientMsg::Join`, server 用它跑 RoomActor join 流程
//! 2. 之后每条 client → server 的 `ClientMsg` 都封成 [`RoomCmd::PlayerMsg`]
//! 3. RoomActor 推过来的 `ServerMsg` 全部通过 ws.send 出去
//!
//! ## 加入者端
//! [`join_remote`] connect 到 `ws://host:port/ws`, 发 Join, 等 Welcome,
//! 构造一个 [`NetSession`] 给 UI 用.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use futures_util::SinkExt;
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as TgMessage;

use crate::net::protocol::{ClientMsg, ServerMsg};
use crate::net::room::{JoinError, RoomCmd, RoomHandle};
use crate::net::session::NetSession;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("绑定 TCP 失败: {0}")]
    Bind(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteJoinError {
    #[error("连接 ws 失败: {0}")]
    Connect(String),
    #[error("协议错误: {0}")]
    Protocol(String),
    #[error("被 server 拒绝: {0}")]
    Refused(String),
}

// ============================================================================
// 房主: 启动 axum WS server
// ============================================================================

/// Spawn 一个 axum WS server, 返回真实绑定地址 (port=0 时让 OS 选).
///
/// `bind` 是绑定地址: 生产 LAN 模式用 `"0.0.0.0"` (任意网卡), 测试用 `"127.0.0.1"`
/// (loopback, 不触发 Windows 防火墙弹窗). `handle` 会被 clone 给每个 ws 连接 task.
/// 如果 RoomActor drop, 各连接 task 自然会读到 channel close → 退出.
pub async fn spawn_ws_server(
    handle: RoomHandle,
    bind: &str,
    port: u16,
) -> Result<SocketAddr, ServerError> {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(Arc::new(handle));
    let listener = TcpListener::bind((bind, port)).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("axum server 错误: {e}");
        }
    });
    Ok(addr)
}

/// LAN 模式绑定地址 — 任意网卡可达.
pub const LAN_BIND: &str = "0.0.0.0";

/// 测试 / loopback 模式绑定地址 — 不触发 Windows 防火墙.
pub const LOOPBACK_BIND: &str = "127.0.0.1";

async fn ws_handler(ws: WebSocketUpgrade, State(handle): State<Arc<RoomHandle>>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, handle))
}

async fn handle_socket(mut socket: WebSocket, handle: Arc<RoomHandle>) {
    // 1) 等 client 第一条 Join 消息.
    let join_msg = match socket.recv().await {
        Some(Ok(Message::Text(s))) => match serde_json::from_str::<ClientMsg>(s.as_str()) {
            Ok(m) => m,
            Err(e) => {
                let err = ServerMsg::Error {
                    message: format!("JSON 解析失败: {e}"),
                };
                let _ = socket
                    .send(Message::Text(
                        serde_json::to_string(&err).unwrap_or_default().into(),
                    ))
                    .await;
                return;
            }
        },
        _ => return,
    };
    let (nickname, reconnect_token) = match join_msg {
        ClientMsg::Join {
            nickname,
            reconnect_token,
        } => (nickname, reconnect_token),
        _ => {
            let err = ServerMsg::Error {
                message: "首条消息必须是 Join".into(),
            };
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&err).unwrap_or_default().into(),
                ))
                .await;
            return;
        }
    };

    // 2) 跑 RoomActor join 流程.
    let (s2c_tx, mut s2c_rx) = mpsc::unbounded_channel::<ServerMsg>();
    let (ack_tx, ack_rx) = oneshot::channel();
    if handle
        .tx
        .send(RoomCmd::Join {
            nickname,
            reconnect_token,
            sender: s2c_tx,
            ack: ack_tx,
        })
        .is_err()
    {
        return;
    }
    let join = match ack_rx.await {
        Ok(Ok(j)) => j,
        Ok(Err(JoinError::RoomFull)) => {
            send_error_msg(&mut socket, "房间已满").await;
            return;
        }
        Ok(Err(JoinError::AlreadyInGame)) => {
            send_error_msg(&mut socket, "房间已开局").await;
            return;
        }
        Ok(Err(JoinError::InvalidReconnectToken)) => {
            send_error_msg(&mut socket, "重连 token 无效").await;
            return;
        }
        Err(_) => return,
    };

    let player_id = join.player_id;

    // 3) 单 task 同时处理读写, 用 select! 避免 split 的 BiLock 死锁.
    loop {
        tokio::select! {
            outgoing = s2c_rx.recv() => {
                let Some(msg) = outgoing else { break; };
                let s = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(s.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(frame)) = incoming else { break; };
                match frame {
                    Message::Text(s) => match serde_json::from_str::<ClientMsg>(s.as_str()) {
                        Ok(msg) => {
                            if handle.tx.send(RoomCmd::PlayerMsg { player_id, msg }).is_err() {
                                break;
                            }
                        }
                        Err(e) => tracing::warn!("ws 收到无法解析的 JSON: {e}"),
                    },
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }

    let _ = handle.tx.send(RoomCmd::Disconnect { player_id });
}

async fn send_error_msg(socket: &mut WebSocket, msg: &str) {
    let s = serde_json::to_string(&ServerMsg::Error {
        message: msg.into(),
    })
    .unwrap_or_default();
    let _ = socket.send(Message::Text(s.into())).await;
}

// ============================================================================
// 加入者: connect ws + bridge
// ============================================================================

/// connect 到房主 server, 发送 Join, 等 Welcome, 返回构造好的 [`NetSession`].
///
/// `addr` 形如 `192.168.1.5:34567` (无 `ws://` 前缀, 函数内自加).
pub async fn join_remote(addr: &str, nickname: String) -> Result<NetSession, RemoteJoinError> {
    let url = format!("ws://{}/ws", addr);
    let (mut ws_stream, _resp) = connect_async(url)
        .await
        .map_err(|e| RemoteJoinError::Connect(e.to_string()))?;

    // 发 Join (整个 stream own 在 task 内, 不用 split, 避免 BiLock 死锁).
    let join_payload = ClientMsg::Join {
        nickname: nickname.clone(),
        reconnect_token: None,
    };
    let join_text = serde_json::to_string(&join_payload)
        .map_err(|e| RemoteJoinError::Protocol(format!("序列化失败: {e}")))?;
    ws_stream
        .send(TgMessage::text(join_text))
        .await
        .map_err(|e| RemoteJoinError::Connect(format!("发 Join 失败: {e}")))?;

    // 等 Welcome (或 Error).
    let first = ws_stream
        .next()
        .await
        .ok_or_else(|| RemoteJoinError::Protocol("server 未回应".into()))?
        .map_err(|e| RemoteJoinError::Protocol(format!("读 frame 失败: {e}")))?;
    let first_text = match first {
        TgMessage::Text(s) => s.to_string(),
        _ => return Err(RemoteJoinError::Protocol("非文本 frame".into())),
    };
    let first_msg: ServerMsg = serde_json::from_str(&first_text)
        .map_err(|e| RemoteJoinError::Protocol(format!("server JSON 错误: {e}")))?;
    let (player_id, token) = match first_msg {
        ServerMsg::Welcome {
            player_id,
            reconnect_token,
            room: _,
        } => (player_id, reconnect_token),
        ServerMsg::Error { message } => return Err(RemoteJoinError::Refused(message)),
        other => {
            return Err(RemoteJoinError::Protocol(format!(
                "首条消息非 Welcome: {:?}",
                std::mem::discriminant(&other)
            )));
        }
    };

    // 用 mpsc 桥接 UI ↔ ws.
    let (out_tx_ui, mut out_rx_ws) = mpsc::unbounded_channel::<ClientMsg>();
    let (in_tx_ws, in_rx_ui) = mpsc::unbounded_channel::<ServerMsg>();

    // 把 Welcome 投递到 UI inbox (Welcome 已被消费, 重新发).
    let welcome_again = ServerMsg::Welcome {
        player_id,
        reconnect_token: token,
        room: Box::new(crate::net::protocol::RoomView {
            room_id: format!("remote-{addr}"),
            host_id: 0,
            config: crate::config::GameConfig::default(),
            players: vec![],
            state: crate::net::protocol::RoomLifecycle::Lobby,
        }),
    };
    let _ = in_tx_ws.send(welcome_again);

    // 单 task 同时处理读写, 用 select! 避免 split 的 BiLock 死锁.
    tokio::spawn(async move {
        loop {
            tokio::select! {
                outgoing = out_rx_ws.recv() => {
                    let Some(msg) = outgoing else { break; };
                    let s = match serde_json::to_string(&msg) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("ClientMsg serialize 失败: {e}");
                            continue;
                        }
                    };
                    if ws_stream.send(TgMessage::text(s)).await.is_err() {
                        break;
                    }
                }
                incoming = ws_stream.next() => {
                    let Some(frame) = incoming else { break; };
                    let frame = match frame {
                        Ok(f) => f,
                        Err(_) => break,
                    };
                    let text = match frame {
                        TgMessage::Text(s) => s.to_string(),
                        TgMessage::Close(_) => break,
                        TgMessage::Ping(p) => {
                            let _ = ws_stream.send(TgMessage::Pong(p)).await;
                            continue;
                        }
                        _ => continue,
                    };
                    match serde_json::from_str::<ServerMsg>(&text) {
                        Ok(msg) => {
                            if in_tx_ws.send(msg).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("收到无法解析 ServerMsg: {e}");
                        }
                    }
                }
            }
        }
    });

    Ok(NetSession::from_channels(
        player_id, token, out_tx_ui, in_rx_ui,
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;
    use crate::net::room::spawn_room;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn end_to_end_two_clients_join_via_ws() {
        let handle = spawn_room("Host".into(), GameConfig::default());
        let addr = spawn_ws_server(handle.clone(), LOOPBACK_BIND, 0)
            .await
            .expect("bind");
        let connect_addr = format!("127.0.0.1:{}", addr.port());

        // 两个加入者
        let mut s1 = join_remote(&connect_addr, "Alice".into())
            .await
            .expect("s1 join");
        let s2 = join_remote(&connect_addr, "Bob".into())
            .await
            .expect("s2 join");

        assert!(s1.player_id != 0);
        assert!(s2.player_id != 0);
        assert_ne!(s1.player_id, s2.player_id);

        // s1 是 host (player_id=1) 默认 ready, s2 需要 Ready(true)
        s2.send(ClientMsg::Ready { ready: true });

        // 轮询直到看到 ≥2 ready 的 RoomUpdate, 最多 3 秒
        let mut max_ready = 0usize;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline && max_ready < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            while let Some(msg) = s1.try_recv() {
                if let ServerMsg::RoomUpdate(view) = &msg {
                    let n = view.players.iter().filter(|p| p.ready).count();
                    if n > max_ready {
                        max_ready = n;
                    }
                }
            }
        }
        assert!(max_ready >= 2, "s1 应该看到 ≥2 准备的 RoomUpdate");
    }
}
