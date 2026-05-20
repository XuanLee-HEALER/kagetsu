//! Pedersen vector commitment (M4.C.1).
//!
//! ## 用途
//! Bayer-Groth shuffle argument 的 base building block. prover 把秘密向量
//! `(a_1, ..., a_N)` (e.g. permutation 的 Hadamard product 因子) commit 到
//! 单个曲线点, 后续 sub-argument 在 commit 上做线性 / 乘积证明.
//!
//! ## 定义
//! 给定 N+1 个独立 generator (g_1, ..., g_N, h), commitment 为:
//!     Com((a_1, ..., a_N), r) = h^r · prod_i(g_i^{a_i})
//!
//! 性质:
//! - **hiding**: 给定 r 随机均匀, Com 在 group 中均匀分布, 不漏 a_i.
//! - **binding**: 改 (a_i, r) 到 (a_i', r') 得到同 Com 等价于求 g_i 之间的 DL
//!   关系, 在 DDH 假设下不可行.
//! - **同态**: Com(a, r1) + Com(b, r2) = Com(a + b, r1 + r2).
//!
//! ## generator 安全性
//! generators 通过 SHA-256 + ChaCha20 RNG 从 (domain, label, n) deterministic
//! 派生 — 任意人重新派生得到同一组. **没人知道** g_i 之间或 g_i 与 h 的
//! DL 关系 (派生过程不可逆), 所以满足 binding.
//!
//! ## 大小
//! Bayer-Groth shuffle (N=136 牌山) 用 m × n = 136 = 8 × 17 分解, 一组 ck
//! 实际只需要 max(m, n) + 1 个 generator. 但本模块作通用 API, n 由调用方决定.

use ark_ec::CurveGroup;
use ark_ff::UniformRand;
use ark_std::rand::SeedableRng;
use ark_std::rand::rngs::StdRng;
use sha2::{Digest, Sha256};

use super::{Curve, Scalar};

const DOMAIN: &[u8] = b"kagetsu/mp/pedersen-ck/v1";

/// Pedersen vector commitment key.
///
/// `generators[i]` 是承诺第 i 个分量用的曲线点, `blinding` 是承诺 randomness.
#[derive(Debug, Clone)]
pub struct CommitmentKey {
    pub generators: Vec<Curve>,
    pub blinding: Curve,
}

impl CommitmentKey {
    /// Deterministic 派生: 任意人用同 (label, n) 拿到完全相同的 ck.
    /// label 应包含协议版本 + sessionID 让不同房间 ck 独立.
    pub fn from_label(label: &[u8], n: usize) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        hasher.update((label.len() as u32).to_be_bytes());
        hasher.update(label);
        hasher.update((n as u32).to_be_bytes());
        let seed_bytes: [u8; 32] = hasher.finalize().into();
        let mut rng = StdRng::from_seed(seed_bytes);

        let generators: Vec<Curve> = (0..n).map(|_| Curve::rand(&mut rng)).collect();
        let blinding = Curve::rand(&mut rng);
        Self {
            generators,
            blinding,
        }
    }

    /// Vector 长度 (= generators.len()).
    pub fn n(&self) -> usize {
        self.generators.len()
    }

    /// Commit to scalar vector with blinding factor r.
    /// **Panic** 如果 values.len() != self.n().
    pub fn commit(&self, values: &[Scalar], r: Scalar) -> Curve {
        assert_eq!(
            values.len(),
            self.generators.len(),
            "vector length must equal commitment key size"
        );
        // multi-scalar multiplication: 比逐项加快几倍 (后续 perf 优化时切到 MSM)
        let mut acc = self.blinding * r;
        for (g_i, a_i) in self.generators.iter().zip(values.iter()) {
            acc += *g_i * *a_i;
        }
        acc
    }

    /// 给单个 scalar 加 blinding 的快捷方式 (Bayer-Groth 中常用).
    /// Com(a, r) = h^r · g_1^a (用第 0 个 generator).
    pub fn commit_single(&self, value: Scalar, r: Scalar) -> Curve {
        debug_assert!(!self.generators.is_empty());
        self.blinding * r + self.generators[0] * value
    }
}

/// 把 commit 化为 affine 形式 (序列化前用; 比较相等用 affine 形式更稳).
pub fn to_affine(c: &Curve) -> ark_bls12_381::G1Affine {
    c.into_affine()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_std::test_rng;

    /// deterministic: 同 label / n → 同 ck.
    #[test]
    fn from_label_is_deterministic() {
        let ck1 = CommitmentKey::from_label(b"test-room-A", 16);
        let ck2 = CommitmentKey::from_label(b"test-room-A", 16);
        assert_eq!(ck1.generators.len(), ck2.generators.len());
        for (a, b) in ck1.generators.iter().zip(ck2.generators.iter()) {
            assert_eq!(a, b);
        }
        assert_eq!(ck1.blinding, ck2.blinding);
    }

    /// 不同 label → 不同 ck.
    #[test]
    fn different_labels_yield_different_ck() {
        let ck1 = CommitmentKey::from_label(b"room-A", 8);
        let ck2 = CommitmentKey::from_label(b"room-B", 8);
        assert_ne!(ck1.generators[0], ck2.generators[0]);
        assert_ne!(ck1.blinding, ck2.blinding);
    }

    /// 不同 n → 不同 ck (避免 prefix 攻击).
    #[test]
    fn different_n_yield_different_first_generator() {
        let ck1 = CommitmentKey::from_label(b"room-X", 8);
        let ck2 = CommitmentKey::from_label(b"room-X", 16);
        assert_ne!(ck1.generators[0], ck2.generators[0]);
    }

    /// hiding: 同 values 不同 r → 不同 commit.
    #[test]
    fn commit_is_hiding_via_blinding() {
        let rng = &mut test_rng();
        let ck = CommitmentKey::from_label(b"test", 4);
        let values: Vec<Scalar> = vec![
            Scalar::from(1u64),
            Scalar::from(2u64),
            Scalar::from(3u64),
            Scalar::from(4u64),
        ];
        let r1 = Scalar::rand(rng);
        let r2 = Scalar::rand(rng);
        let c1 = ck.commit(&values, r1);
        let c2 = ck.commit(&values, r2);
        assert_ne!(c1, c2);
    }

    /// 不同 values 同 r → 不同 commit (binding sanity, 非密码学完整证明).
    #[test]
    fn commit_changes_with_values() {
        let ck = CommitmentKey::from_label(b"test", 3);
        let r = Scalar::from(7u64);
        let v1: Vec<Scalar> = vec![Scalar::from(1u64), Scalar::from(2u64), Scalar::from(3u64)];
        let v2: Vec<Scalar> = vec![Scalar::from(1u64), Scalar::from(2u64), Scalar::from(4u64)];
        assert_ne!(ck.commit(&v1, r), ck.commit(&v2, r));
    }

    /// 同态加: Com(a, r1) + Com(b, r2) = Com(a + b, r1 + r2).
    #[test]
    fn commit_is_additively_homomorphic() {
        let rng = &mut test_rng();
        let ck = CommitmentKey::from_label(b"test", 4);
        let a: Vec<Scalar> = (0..4).map(|_| Scalar::rand(rng)).collect();
        let b: Vec<Scalar> = (0..4).map(|_| Scalar::rand(rng)).collect();
        let r1 = Scalar::rand(rng);
        let r2 = Scalar::rand(rng);

        let c_a = ck.commit(&a, r1);
        let c_b = ck.commit(&b, r2);
        let c_sum_via_add = c_a + c_b;

        let sum: Vec<Scalar> = a.iter().zip(b.iter()).map(|(x, y)| *x + y).collect();
        let c_sum_direct = ck.commit(&sum, r1 + r2);

        assert_eq!(c_sum_via_add, c_sum_direct);
    }

    /// 标量乘: c · Com(a, r) = Com(c · a, c · r).
    #[test]
    fn commit_is_scalar_multiplicatively_homomorphic() {
        let rng = &mut test_rng();
        let ck = CommitmentKey::from_label(b"test", 4);
        let a: Vec<Scalar> = (0..4).map(|_| Scalar::rand(rng)).collect();
        let r = Scalar::rand(rng);
        let c = Scalar::rand(rng);

        let c_a = ck.commit(&a, r);
        let scaled = c_a * c;

        let scaled_a: Vec<Scalar> = a.iter().map(|x| *x * c).collect();
        let direct = ck.commit(&scaled_a, r * c);
        assert_eq!(scaled, direct);
    }

    /// commit_single 跟 commit-with-zeros-padded 一致.
    #[test]
    fn commit_single_matches_padded_vector() {
        let ck = CommitmentKey::from_label(b"test", 4);
        let v = Scalar::from(42u64);
        let r = Scalar::from(7u64);
        let single = ck.commit_single(v, r);

        let mut padded = vec![Scalar::from(0u64); 4];
        padded[0] = v;
        let via_vector = ck.commit(&padded, r);
        assert_eq!(single, via_vector);
    }

    /// vector 长度不匹配应 panic.
    #[test]
    #[should_panic(expected = "vector length must equal commitment key size")]
    fn commit_panics_on_size_mismatch() {
        let ck = CommitmentKey::from_label(b"test", 4);
        let too_short = vec![Scalar::from(1u64); 3];
        let _ = ck.commit(&too_short, Scalar::from(0u64));
    }
}
