//! Tile ↔ Curve point 双向映射 (M5.B.0).
//!
//! ElGamal 加密的明文必须是 group 元素 (Curve point), 不能直接是整数. 协议 1
//! 加密初始 deck 时, 把 136 张 Tile 各编码为唯一 Curve point. 解密 (协议 2/3)
//! 后通过反查 BiMap 还原 Tile.
//!
//! ## 派生方式
//! [`from_label`] 用 SHA-256 + ChaCha20Rng 从 session label (e.g. room_id +
//! 开局 nonce) deterministic 派生 136 个独立 Curve point. 任意 prover/verifier
//! 用同 label 拿同一组 mapping. 不可预测 (PRF) 但 reproducible.
//!
//! ## 安全性
//! - 派生 RNG 不可逆 → attacker 不能从 Curve point 反推 Tile (除查 BiMap)
//! - SHA-256 输出充分熵 → 136 个 random points collision 概率 ≈ 2^{-256}
//! - **不依赖 Tile 内部 ID 到 Curve 的关系** — 各方独立派生时同 label 同结果
//!
//! ## 跟 Wall 的关系
//! Wall (engine/wall.rs) 在 Standard 模式下 own 136 个 Tile 实例 (含 id /
//! kind / red). ZeroTrust 模式下 Wall 不 own Tile, 而是 own Vec<Ciphertext>;
//! Card mapping 把 Tile 索引 (0..136) ↔ Curve point. 解密后通过 BiMap +
//! 已知 (索引→Tile 实例) 列表得到具体 Tile.

use std::collections::HashMap;

use ark_ff::UniformRand;
use ark_serialize::CanonicalSerialize;
use ark_std::rand::SeedableRng;
use ark_std::rand::rngs::StdRng;
use sha2::{Digest, Sha256};

use super::Curve;

const DOMAIN: &[u8] = b"kagetsu/mp/card-mapping/v1";
/// 一副牌的张数.
pub const DECK_SIZE: usize = 136;

/// Tile ↔ Curve point 双向映射.
///
/// 内部用 Vec<Curve> 正向 (tile_id → point), HashMap<bytes, tile_id> 反向.
/// 反向用 serialize_compressed 字节作 HashMap key (Curve 不实现 Hash).
#[derive(Debug, Clone)]
pub struct CardMapping {
    points: Vec<Curve>,
    point_to_id: HashMap<Vec<u8>, usize>,
}

impl CardMapping {
    /// Deterministic 派生: 任意人用同 (label, n) 拿到完全相同的 mapping.
    /// label 应包含房间 ID + 开局 nonce 让不同房间 / 不同局 mapping 独立.
    pub fn from_label(label: &[u8]) -> Self {
        Self::from_label_sized(label, DECK_SIZE)
    }

    /// 任意 size 版本 (供单测用小 N 加速).
    pub fn from_label_sized(label: &[u8], n: usize) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        hasher.update((label.len() as u32).to_be_bytes());
        hasher.update(label);
        hasher.update((n as u32).to_be_bytes());
        let seed_bytes: [u8; 32] = hasher.finalize().into();
        let mut rng = StdRng::from_seed(seed_bytes);

        let mut points = Vec::with_capacity(n);
        let mut point_to_id = HashMap::with_capacity(n);
        for id in 0..n {
            // 用 rejection sampling 避免 collision (理论概率 ~2^{-256}, 但保险).
            loop {
                let p = Curve::rand(&mut rng);
                let bytes = serialize_point(&p);
                if let std::collections::hash_map::Entry::Vacant(e) = point_to_id.entry(bytes) {
                    e.insert(id);
                    points.push(p);
                    break;
                }
            }
        }
        Self {
            points,
            point_to_id,
        }
    }

    /// 张数.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// 编码 tile_id (0..len) → Curve point.
    /// **Panic** 越界.
    pub fn encode(&self, tile_id: usize) -> Curve {
        self.points[tile_id]
    }

    /// 解码 Curve point → tile_id. 不在 mapping 返回 None.
    pub fn decode(&self, point: &Curve) -> Option<usize> {
        let bytes = serialize_point(point);
        self.point_to_id.get(&bytes).copied()
    }

    /// 全部 points 的 slice (协议 1 加密初始 deck 时用).
    pub fn points(&self) -> &[Curve] {
        &self.points
    }
}

fn serialize_point(p: &Curve) -> Vec<u8> {
    let mut buf = Vec::with_capacity(48);
    p.serialize_compressed(&mut buf)
        .expect("ark serialize Curve point");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    #[test]
    fn from_label_is_deterministic() {
        let m1 = CardMapping::from_label_sized(b"room-A/seed-1", 16);
        let m2 = CardMapping::from_label_sized(b"room-A/seed-1", 16);
        assert_eq!(m1.len(), 16);
        assert_eq!(m2.len(), 16);
        for i in 0..16 {
            assert_eq!(m1.encode(i), m2.encode(i));
        }
    }

    #[test]
    fn different_labels_yield_different_mapping() {
        let m1 = CardMapping::from_label_sized(b"room-A", 8);
        let m2 = CardMapping::from_label_sized(b"room-B", 8);
        assert_ne!(m1.encode(0), m2.encode(0));
    }

    #[test]
    fn different_n_yield_different_first_point() {
        let m1 = CardMapping::from_label_sized(b"room-X", 8);
        let m2 = CardMapping::from_label_sized(b"room-X", 16);
        // 不同 n 派生时 hash seed 含 n, 所以序列从一开始就不同
        assert_ne!(m1.encode(0), m2.encode(0));
    }

    #[test]
    fn encode_decode_roundtrip() {
        let m = CardMapping::from_label_sized(b"test", 32);
        for i in 0..32 {
            let p = m.encode(i);
            assert_eq!(m.decode(&p), Some(i));
        }
    }

    #[test]
    fn random_point_not_in_mapping_returns_none() {
        let m = CardMapping::from_label_sized(b"test", 8);
        let rng = &mut test_rng();
        // 极小概率撞上, 跑 5 次足以可靠
        for _ in 0..5 {
            let random = Curve::rand(rng);
            // mapping 里只 8 个点, 随机 Curve 撞上概率 ≈ 8/2^256
            assert!(m.decode(&random).is_none() || m.points().contains(&random));
        }
    }

    #[test]
    fn no_collisions_in_full_deck() {
        let m = CardMapping::from_label(b"full");
        assert_eq!(m.len(), DECK_SIZE);
        // 全部 136 个 point 应唯一
        let mut seen = std::collections::HashSet::new();
        for p in m.points() {
            let bytes = serialize_point(p);
            assert!(seen.insert(bytes), "collision detected");
        }
    }

    #[test]
    fn full_deck_all_decodable() {
        let m = CardMapping::from_label(b"decode-all");
        for i in 0..DECK_SIZE {
            let p = m.encode(i);
            assert_eq!(m.decode(&p), Some(i));
        }
    }

    /// 同 label 不同实例 mapping 内部 BiMap 也一致 (HashMap insertion order
    /// 不影响 lookup).
    #[test]
    fn deterministic_decode_path() {
        let m1 = CardMapping::from_label_sized(b"x", 16);
        let m2 = CardMapping::from_label_sized(b"x", 16);
        for i in 0..16 {
            let p = m1.encode(i);
            // m2.decode(p) 应也是 i (用同 label 派生, points 一致)
            assert_eq!(m2.decode(&p), Some(i));
        }
    }

    #[test]
    fn empty_mapping_is_handled() {
        let m = CardMapping::from_label_sized(b"empty", 0);
        assert_eq!(m.len(), 0);
        assert!(m.is_empty());
    }

    #[test]
    fn points_slice_matches_encode() {
        let m = CardMapping::from_label_sized(b"slice", 8);
        let points = m.points();
        assert_eq!(points.len(), 8);
        for (i, p) in points.iter().enumerate() {
            assert_eq!(*p, m.encode(i));
        }
    }
}
