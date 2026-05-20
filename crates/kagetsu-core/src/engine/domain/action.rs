//! 玩家可执行的动作 (Action).
//!
//! UI / AI 决策的中间表示. 调用方负责把 Action 翻译成
//! [`crate::engine::op::AtomicOp`] 喂给
//! [`crate::engine::round_state::round_apply`].

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::Tile;

/// 玩家可执行的动作 — UI / AI 决策的中间表示.
///
/// 与 [`crate::engine::op::AtomicOp`] 的区别:
///
/// | | `Action` | `AtomicOp` |
/// |---|----------|------------|
/// | 角色 | UI/AI 决策出参 | engine 入参 |
/// | 颗粒度 | 玩家视角动作 | 局内不可分算子 |
/// | 立直 | `Riichi(tile)` 一步 | `RiichiDeclare` + `Discard` 两步 |
/// | 鸣牌 ID | 含具体 Tile | 含 hand_tile_ids (id 唯一) |
///
/// driver 通常先收集 `Action` (从 UI 输入或 AI 决策), 再翻译成
/// 1 或 2 个 `AtomicOp` 喂给 [`crate::engine::round_state::round_apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 切牌 (打牌 / Dahai).
    Discard(Tile),
    /// 立直 (リーチ / Riichi) 宣告 + 切某张. UI 一步表达, engine 内部拆两步.
    Riichi(Tile),
    /// 碰 (ポン / Pon). `tiles` = 自手中与他家弃牌组成刻子的两张.
    Pon { tiles: [Tile; 2] },
    /// 吃 (チー / Chi). `tiles` = 自手中与上家弃牌组成顺子的两张.
    Chi { tiles: [Tile; 2] },
    /// 大明杠 (大明槓 / Minkan). 来源在 driver 处确定 (kind 从 last_discard 推).
    Minkan,
    /// 暗杠 (暗槓 / Ankan). 4 张同 kind 中任选 1 张代表 (kind 唯一定位).
    Ankan(Tile),
    /// 加杠 (加槓 / 小明槓 / Shouminkan). 自手第 4 张同 kind, 加进已有 Pon.
    Shouminkan(Tile),
    /// 自摸和了 (自摸 / Tsumo).
    Tsumo,
    /// 荣和 (栄和 / ロン / Ron). 参数 = 自家 Seat (`AtomicOp::Ron::who`).
    Ron(Seat),
    /// 九种九牌流局 (九種九牌 / Kyuushu Kyuuhai). 子家第一巡内手中 ≥ 9 种幺九牌
    /// 可宣此流局. 当前 engine *未实现*, variant 保留备用.
    KyuushuKyuuhai,
    /// 跳过 (パス / Pass). 鸣牌窗口 / 和了机会放弃.
    Pass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::tile::TileIndex;

    fn t(kind: u8, id: u16) -> Tile {
        Tile {
            kind: TileIndex(kind),
            red: false,
            id,
        }
    }

    #[test]
    fn action_equality_distinguishes_variants() {
        let d1 = Action::Discard(t(0, 0));
        let d2 = Action::Discard(t(0, 0));
        assert_eq!(d1, d2);
        let r = Action::Riichi(t(0, 0));
        assert_ne!(d1, r);
    }

    #[test]
    fn discard_distinguishes_tiles() {
        let a = Action::Discard(t(0, 0));
        let b = Action::Discard(t(1, 1));
        assert_ne!(a, b);
    }

    #[test]
    fn pon_with_different_tile_pairs_differ() {
        let p1 = Action::Pon {
            tiles: [t(0, 0), t(0, 1)],
        };
        let p2 = Action::Pon {
            tiles: [t(0, 2), t(0, 3)],
        };
        assert_ne!(p1, p2);
        // 同 tiles 同 id 相等
        let p3 = Action::Pon {
            tiles: [t(0, 0), t(0, 1)],
        };
        assert_eq!(p1, p3);
    }

    #[test]
    fn ron_records_target_seat() {
        let r1 = Action::Ron(crate::engine::domain::meld::Seat::West);
        let r2 = Action::Ron(crate::engine::domain::meld::Seat::North);
        assert_ne!(r1, r2);
    }

    #[test]
    fn pass_and_tsumo_are_unit() {
        assert_eq!(Action::Pass, Action::Pass);
        assert_eq!(Action::Tsumo, Action::Tsumo);
        assert_eq!(Action::KyuushuKyuuhai, Action::KyuushuKyuuhai);
        assert_eq!(Action::Minkan, Action::Minkan);
        assert_ne!(Action::Pass, Action::Tsumo);
    }

    #[test]
    fn ankan_carries_tile_kind() {
        let a1 = Action::Ankan(t(0, 0));
        let a2 = Action::Ankan(t(1, 0));
        assert_ne!(a1, a2);
    }

    #[test]
    fn action_clones_correctly() {
        let a = Action::Pon {
            tiles: [t(0, 0), t(0, 1)],
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
