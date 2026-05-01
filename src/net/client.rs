//! Transport 抽象 + LocalTransport (房主自己用同进程 channel).
//!
//! `NetTransport<Out, In>` 表示一个双向消息通道:
//! - Client 端用 `NetTransport<ClientMsg, ServerMsg>`: 发 client msg, 收 server msg
//! - Server 端用 `NetTransport<ServerMsg, ClientMsg>`: 反过来
//!
//! Phase 2 只实现 [`LocalTransport`] (mpsc 同进程 channel), 房主自己用 — 不
//! 走网络 socket. Phase 5 会加 `WsTransport` 给跨网络 client 用.

use thiserror::Error;
use tokio::sync::mpsc::{self, error::TryRecvError};

use crate::net::protocol::{ClientMsg, ServerMsg};

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport disconnected")]
    Disconnected,
}

/// 双向异步消息通道. `Out` 发出去, `In` 收进来.
pub trait NetTransport<Out, In>: Send + 'static
where
    Out: Send + 'static,
    In: Send + 'static,
{
    /// 发一条消息. 通道关闭返回 [`TransportError::Disconnected`].
    fn send(&mut self, msg: Out) -> Result<(), TransportError>;

    /// 非阻塞收消息. `Ok(None)` = 通道空但未关闭, `Err(Disconnected)` = 关闭.
    fn try_recv(&mut self) -> Result<Option<In>, TransportError>;

    /// 通道是否还活着 (对端没断).
    fn is_connected(&self) -> bool;
}

/// Client 端 transport (alias).
pub type ClientTransport = Box<dyn NetTransport<ClientMsg, ServerMsg>>;
/// Server 端 transport (alias).
pub type ServerTransport = Box<dyn NetTransport<ServerMsg, ClientMsg>>;

// ============================================================================
// LocalTransport (mpsc 同进程, 房主自己用)
// ============================================================================

/// 同进程 mpsc transport. 房主既是 server 又是 client, 用这个连接两端避免
/// 走网络 socket.
pub struct LocalTransport<Out, In>
where
    Out: Send + 'static,
    In: Send + 'static,
{
    tx: mpsc::UnboundedSender<Out>,
    rx: mpsc::UnboundedReceiver<In>,
}

impl<Out, In> NetTransport<Out, In> for LocalTransport<Out, In>
where
    Out: Send + 'static,
    In: Send + 'static,
{
    fn send(&mut self, msg: Out) -> Result<(), TransportError> {
        self.tx.send(msg).map_err(|_| TransportError::Disconnected)
    }

    fn try_recv(&mut self) -> Result<Option<In>, TransportError> {
        match self.rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(TransportError::Disconnected),
        }
    }

    fn is_connected(&self) -> bool {
        !self.tx.is_closed()
    }
}

/// 创建一对同进程 transport, 用于房主-自己 connection.
///
/// 返回 `(client, server)` — 各自看自己一侧.
pub fn local_pair() -> (
    LocalTransport<ClientMsg, ServerMsg>,
    LocalTransport<ServerMsg, ClientMsg>,
) {
    let (c2s_tx, c2s_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (s2c_tx, s2c_rx) = mpsc::unbounded_channel::<ServerMsg>();
    let client = LocalTransport {
        tx: c2s_tx,
        rx: s2c_rx,
    };
    let server = LocalTransport {
        tx: s2c_tx,
        rx: c2s_rx,
    };
    (client, server)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::{ClientMsg, ServerMsg};

    #[tokio::test]
    async fn local_pair_round_trip() {
        let (mut client, mut server) = local_pair();

        // client → server
        client.send(ClientMsg::Pong(42)).unwrap();
        // 在 same task 内同步 try_recv 即可 (unbounded_channel 立即可见)
        match server.try_recv().unwrap() {
            Some(ClientMsg::Pong(n)) => assert_eq!(n, 42),
            other => panic!("unexpected: {:?}", other),
        }

        // server → client
        server.send(ServerMsg::Ping(99)).unwrap();
        match client.try_recv().unwrap() {
            Some(ServerMsg::Ping(n)) => assert_eq!(n, 99),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[tokio::test]
    async fn try_recv_returns_none_when_empty() {
        let (_client, mut server) = local_pair();
        assert!(server.try_recv().unwrap().is_none());
    }

    #[tokio::test]
    async fn drop_one_side_disconnects() {
        let (client, mut server) = local_pair();
        drop(client);
        // server 收侧 (来自 client) → channel 已关
        assert!(matches!(
            server.try_recv(),
            Err(TransportError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn disconnected_send_returns_err() {
        let (mut client, server) = local_pair();
        drop(server);
        let r = client.send(ClientMsg::Pong(1));
        assert!(matches!(r, Err(TransportError::Disconnected)));
    }

    #[tokio::test]
    async fn is_connected_reflects_peer_drop() {
        let (client, server) = local_pair();
        assert!(client.is_connected());
        drop(server);
        assert!(!client.is_connected());
    }
}
