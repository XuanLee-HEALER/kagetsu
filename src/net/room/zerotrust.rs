//! room/zerotrust — RoomActor 拆分模块: ZeroTrust 模式开局 (mental poker MpStart 分发)
//!
//! impl block 直接读 super::RoomActor 私有字段 (Rust 子 module 可访问 parent 私有).

use crate::engine::domain::meld::Seat;
use crate::net::protocol::{
    RoomLifecycle, ServerMsg,
};
use super::*;

impl RoomActor {
    pub(super) fn start_zerotrust_game(&mut self) {
        // 分配座位 (东南西北 = own_index 0..3, 跟 Standard 一致).
        let seats = [Seat::East, Seat::South, Seat::West, Seat::North];
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.seat = Some(seats[i]);
        }

        // M5.D.2: 用真 libp2p PeerId 字节填 all_peer_ids.
        // host_swarm_task 在 Join 时通过 RoomCmd::AssociatePeer 注入加入者 PeerId,
        // spawn_p2p_listener 通过 RoomCmd::SetLocalPeerId 注入 host 自己的.
        // 任一 slot 缺 PeerId 时拒绝开局, 让 caller (UI) 重试.
        let mut all_peer_ids: Vec<Vec<u8>> = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            match self.player_peers.get(&slot.id) {
                Some(p) => all_peer_ids.push(p.clone()),
                None => {
                    self.send_error(
                        self.slots[0].id,
                        &format!(
                            "ZeroTrust: slot {} (id={}) 缺 libp2p PeerId 关联, 等 P2P 层 ready 再开局",
                            slot.nickname, slot.id
                        ),
                    );
                    return;
                }
            }
        }

        // session_label = SHA-256(room_id || sorted_peer_ids) — 4 方独立算应一致.
        let session_label = compute_session_label(&self.room_id, &all_peer_ids);

        // 牌山大小 + cnc K 从 GameRules 派生 (生产 = 136 / 80, 测试可缩).
        let deck_size: u32 = 136;
        let cnc_k_rounds: u32 = 80;

        // 给每个真人玩家发 MpStart, own_index = slot index.
        for (idx, slot) in self.slots.iter().enumerate() {
            if let Some(sender) = &slot.sender {
                let _ = sender.send(ServerMsg::MpStart {
                    all_peer_ids: all_peer_ids.clone(),
                    own_index: idx as u32,
                    session_label: session_label.clone(),
                    deck_size,
                    cnc_k_rounds,
                });
            }
        }

        self.state = RoomLifecycle::InGame;
        self.broadcast_room_update();
    }
}
