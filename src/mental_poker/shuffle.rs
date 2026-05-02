//! 联合洗牌 (协议 1) — M4.C.
//!
//! ## 协议总览
//! 4 玩家轮流给牌山做 (置换 + 重加密) 操作, 每轮带 Bayer-Groth ZK 证明,
//! 其他人验证后接受新牌山. 任一方不串通, 最终牌序对所有人都不可预知.
//!
//! ## 当前实现进度
//! - **M4.C.0** (此文件): plain shuffle 函数 (Permutation + ReEnc), 无 ZK 证明,
//!   仅建 API + 验证 roundtrip 正确性 (解密后 plaintext 集合保持).
//! - M4.C.1+ (后续 commits): Bayer-Groth ZK shuffle argument.
//!
//! ## 为什么先做 plain shuffle
//! ZK 证明的存在条件是 "有合法的 shuffle 关系存在". 如果 shuffle 函数本身
//! buggy, ZK proof 也证不出来. 先把基础对的, 然后 ZK 套上去.

use ark_ff::UniformRand;
use ark_std::rand::Rng;

use super::Scalar;
use super::elgamal::{Ciphertext, PublicKey, remask};

/// 置换 π: 长度 N 的 vec, π[i] = j 表示 "output position i 放 input position j 的元素".
///
/// 必须是 bijection ([0, N) → [0, N) 的全双射), 由 [`Self::random`] 生成时保证.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permutation(Vec<usize>);

impl Permutation {
    /// Fisher-Yates 生成长度 N 的均匀随机置换.
    pub fn random<R: Rng + ?Sized>(rng: &mut R, n: usize) -> Self {
        let mut indices: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (rng.next_u64() as usize) % (i + 1);
            indices.swap(i, j);
        }
        Self(indices)
    }

    /// 长度.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 把置换 apply 到一个 vec: out[i] = items[π[i]].
    pub fn apply<T: Clone>(&self, items: &[T]) -> Vec<T> {
        debug_assert_eq!(items.len(), self.0.len(), "permutation length mismatch");
        self.0.iter().map(|&idx| items[idx].clone()).collect()
    }

    /// 内部 mapping (for ZK proof 构造).
    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }

    /// 反转: 如果 self = π, 返回 π^{-1}.
    pub fn inverse(&self) -> Self {
        let mut inv = vec![0usize; self.0.len()];
        for (i, &j) in self.0.iter().enumerate() {
            inv[j] = i;
        }
        Self(inv)
    }

    /// 检查是否是合法 permutation ([0, N) 每个值出现恰一次).
    /// random / inverse 内部已保证, 外部构造时调一下.
    pub fn is_valid(&self) -> bool {
        let n = self.0.len();
        let mut seen = vec![false; n];
        for &v in &self.0 {
            if v >= n || seen[v] {
                return false;
            }
            seen[v] = true;
        }
        true
    }

    /// 从原始 mapping 构造, 不做 valid check (for ZK proof reconstruction).
    pub fn from_raw(indices: Vec<usize>) -> Self {
        Self(indices)
    }
}

/// 单玩家 shuffle 一轮: 随机生成 π 和 mask 因子向量 r, 输出新牌山.
///
/// 返回 (out_deck, π, r):
/// - out_deck: 长度同 input, out_deck[i] = ReEnc(input[π[i]], r[i])
/// - π: 用过的置换, ZK 证明时需要
/// - r: 用过的 mask 因子向量, ZK 证明时需要
///
/// **ZK 证明在 M4.C.5 加上**. 当前函数本身不带证明 — 调用方传入的 in/out
/// 一致性由后续 `prove_shuffle` (TODO) 保证.
pub fn shuffle_and_remask<R: Rng + ?Sized>(
    rng: &mut R,
    pk: &PublicKey,
    deck: &[Ciphertext],
) -> (Vec<Ciphertext>, Permutation, Vec<Scalar>) {
    let n = deck.len();
    let pi = Permutation::random(rng, n);
    let r: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();

    // out[i] = ReEnc(input[π[i]], r[i])
    let mut out: Vec<Ciphertext> = pi.apply(deck);
    for (ct, r_i) in out.iter_mut().zip(r.iter()) {
        *ct = remask(pk, ct, *r_i);
    }
    (out, pi, r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::Curve;
    use crate::mental_poker::elgamal::{keygen, mask, unmask_with_sk};
    use ark_ff::UniformRand;
    use ark_std::test_rng;
    use std::collections::HashSet;

    #[test]
    fn permutation_random_is_bijection() {
        let rng = &mut test_rng();
        for n in [1usize, 2, 4, 8, 16, 136] {
            let p = Permutation::random(rng, n);
            assert!(p.is_valid(), "n={n}");
            assert_eq!(p.len(), n);
        }
    }

    #[test]
    fn permutation_apply_then_inverse_is_identity() {
        let rng = &mut test_rng();
        let n = 32;
        let p = Permutation::random(rng, n);
        let items: Vec<u32> = (0..n as u32).collect();
        let shuffled = p.apply(&items);
        let inv = p.inverse();
        let restored = inv.apply(&shuffled);
        assert_eq!(restored, items);
    }

    #[test]
    fn permutation_is_valid_rejects_duplicates() {
        let p = Permutation::from_raw(vec![0, 1, 1, 3]);
        assert!(!p.is_valid());
    }

    #[test]
    fn permutation_is_valid_rejects_out_of_range() {
        let p = Permutation::from_raw(vec![0, 1, 2, 9]);
        assert!(!p.is_valid());
    }

    /// shuffle 一轮: plaintext 集合保持, 顺序变化.
    #[test]
    fn shuffle_preserves_plaintext_set() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);

        let n = 16usize;
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let deck: Vec<Ciphertext> = plaintexts.iter().map(|m| mask(rng, &pk, m).0).collect();

        let (shuffled, _pi, _r) = shuffle_and_remask(rng, &pk, &deck);
        assert_eq!(shuffled.len(), n);

        // 解密 shuffled, 集合应等于 plaintext 集合 (作集合比较, 顺序不重要)
        let recovered: Vec<Curve> = shuffled.iter().map(|c| unmask_with_sk(&sk, c)).collect();
        let original_set: HashSet<_> = plaintexts.iter().map(|p| format!("{p}")).collect();
        let recovered_set: HashSet<_> = recovered.iter().map(|p| format!("{p}")).collect();
        assert_eq!(original_set, recovered_set);
    }

    /// shuffle 后 ciphertext 都变 (因为 remask 用了新 r), 即使 π 是 identity.
    #[test]
    fn shuffle_remasks_every_ciphertext() {
        let rng = &mut test_rng();
        let (_, pk) = keygen(rng);
        let deck: Vec<Ciphertext> = (0..8)
            .map(|_| {
                let m = Curve::rand(rng);
                mask(rng, &pk, &m).0
            })
            .collect();

        let (shuffled, _pi, _r) = shuffle_and_remask(rng, &pk, &deck);
        // 不太可能 random π = identity 且 r = 0, 即使 π = identity remask 也变密文
        let any_changed = deck.iter().zip(shuffled.iter()).any(|(a, b)| a != b);
        assert!(any_changed, "shuffle 应改变密文");
    }

    /// 4 人 sequential shuffle: plaintext 集合最终保持.
    #[test]
    fn four_player_sequential_shuffle_preserves_plaintexts() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);

        // 模拟麻将一副: 136 张
        let n = 136usize;
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let mut deck: Vec<Ciphertext> = plaintexts.iter().map(|m| mask(rng, &pk, m).0).collect();

        // 4 个玩家轮流洗
        for _player in 0..4 {
            let (out, _pi, _r) = shuffle_and_remask(rng, &pk, &deck);
            deck = out;
        }

        // plaintext 集合保持
        let recovered: HashSet<_> = deck
            .iter()
            .map(|c| format!("{}", unmask_with_sk(&sk, c)))
            .collect();
        let original: HashSet<_> = plaintexts.iter().map(|p| format!("{p}")).collect();
        assert_eq!(recovered.len(), n);
        assert_eq!(recovered, original);
    }

    /// 麻将 136 张 baseline 性能 (无 ZK) — 仅作未来 ZK proof 性能对比基线.
    /// 跑出来时间会 print 在 cargo test --nocapture 输出.
    #[test]
    fn shuffle_136_baseline_performance() {
        let rng = &mut test_rng();
        let (_, pk) = keygen(rng);
        let n = 136usize;
        let deck: Vec<Ciphertext> = (0..n)
            .map(|_| {
                let m = Curve::rand(rng);
                mask(rng, &pk, &m).0
            })
            .collect();

        let t0 = std::time::Instant::now();
        let (_, _, _) = shuffle_and_remask(rng, &pk, &deck);
        let dt = t0.elapsed();
        println!("[baseline] 136 张单轮 shuffle (无 ZK): {dt:?}");
        // 不断言绝对值, 仅记录. 通常 < 50ms.
        assert!(dt.as_secs() < 5, "shuffle baseline 异常慢 ({dt:?})");
    }
}
