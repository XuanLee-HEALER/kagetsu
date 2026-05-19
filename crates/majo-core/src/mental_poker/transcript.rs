//! Fiat-Shamir transcript: 把交互式 Sigma 协议变非交互式.
//!
//! 用法: 证明者 / 验证者**独立**构造同样序列的 transcript, 各自调用
//! [`Transcript::challenge_scalar`] 得到相同的 challenge. 任意一方偏离
//! → 双方算出不同 challenge → 验证失败.
//!
//! 实现选 SHA-256 (sha2), 已被 libp2p 间接引入, 不增加版本碎片. 不用 merlin
//! 等 Strobe-based transcript 是因为协议长度小 (Schnorr / DLEQ ~ 4-5 个点),
//! SHA-256 足够 + zero new deps.
//!
//! ## 安全约束
//! - 每条信息 commit 前必须 prepend 一个 **domain separator label** 防止
//!   长度扩展 / 跨协议复用攻击 (e.g. label_bytes(b"schnorr-prove-step-1"))
//! - 每个 commit 前 prepend 4-byte big-endian length 防止 ambiguous parse
//!   (e.g. commit("ab") + commit("c") 不应跟 commit("abc") 同 hash)
//! - challenge 通过 `from_le_bytes_mod_order` 从 hash bytes 派生 Scalar; 落入
//!   group 阶上的均匀分布 (Fr ≈ 256-bit, SHA-256 输出 256-bit, 偏差可忽略)

use ark_ff::PrimeField;
use ark_serialize::CanonicalSerialize;
use sha2::{Digest, Sha256};

use super::{Curve, Scalar};

/// 累积要 hash 的 bytes 的 transcript. 不可重置, 一次性使用.
pub struct Transcript {
    hasher: Sha256,
}

impl Transcript {
    /// 起一个新 transcript, 用 `domain` 作 protocol-binding label
    /// (e.g. "tui-majo/mp/schnorr-dl/v1").
    pub fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update((domain.len() as u32).to_be_bytes());
        hasher.update(domain);
        Self { hasher }
    }

    /// commit 一个带 label 的 byte slice. label 防止跨字段拼接歧义.
    pub fn append_message(&mut self, label: &[u8], bytes: &[u8]) {
        self.hasher.update((label.len() as u32).to_be_bytes());
        self.hasher.update(label);
        self.hasher.update((bytes.len() as u32).to_be_bytes());
        self.hasher.update(bytes);
    }

    /// commit 一个曲线点 (用 ark canonical serialization).
    pub fn append_point(&mut self, label: &[u8], point: &Curve) {
        let mut buf = Vec::new();
        point
            .serialize_compressed(&mut buf)
            .expect("ark serialize curve point");
        self.append_message(label, &buf);
    }

    /// 派生一个 Fr challenge. 不消耗 transcript (可继续 append + 派生更多).
    /// 派生后内部加上 label "challenge" 以防同一 transcript 派生两个相同 c.
    pub fn challenge_scalar(&mut self, label: &[u8]) -> Scalar {
        self.append_message(b"challenge_label", label);
        let bytes = self.hasher.clone().finalize();
        // 把派生这一步也喂进 hasher, 让后续派生区分:
        self.hasher.update(b"challenge_consumed");
        self.hasher.update(bytes);
        Scalar::from_le_bytes_mod_order(&bytes)
    }

    /// 派生 `count` 个 challenge bits (布尔). 用于 cut-and-choose 协议.
    /// SHA-256 256 bit 输出, 跨 32 字节扩展时再 hash 一轮.
    pub fn challenge_bits(&mut self, label: &[u8], count: usize) -> Vec<bool> {
        self.append_message(b"challenge_bits_label", label);
        let mut bits = Vec::with_capacity(count);
        let mut counter: u32 = 0;
        while bits.len() < count {
            let mut h = self.hasher.clone();
            h.update(b"bits_chunk");
            h.update(counter.to_be_bytes());
            let chunk = h.finalize();
            for byte in chunk.iter() {
                for shift in 0..8 {
                    if bits.len() >= count {
                        break;
                    }
                    bits.push((byte >> shift) & 1 == 1);
                }
                if bits.len() >= count {
                    break;
                }
            }
            counter += 1;
        }
        // 把派生这一步喂进 hasher 防止后续 challenge 重复
        self.hasher.update(b"bits_consumed");
        self.hasher.update((count as u32).to_be_bytes());
        bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ec::PrimeGroup;

    #[test]
    fn same_input_same_challenge() {
        let mut t1 = Transcript::new(b"test");
        let mut t2 = Transcript::new(b"test");
        let p = Curve::generator();
        t1.append_point(b"P", &p);
        t2.append_point(b"P", &p);
        assert_eq!(t1.challenge_scalar(b"c"), t2.challenge_scalar(b"c"));
    }

    #[test]
    fn different_domain_different_challenge() {
        let mut t1 = Transcript::new(b"alpha");
        let mut t2 = Transcript::new(b"beta");
        let p = Curve::generator();
        t1.append_point(b"P", &p);
        t2.append_point(b"P", &p);
        assert_ne!(t1.challenge_scalar(b"c"), t2.challenge_scalar(b"c"));
    }

    #[test]
    fn different_message_different_challenge() {
        let mut t1 = Transcript::new(b"test");
        let mut t2 = Transcript::new(b"test");
        let g = Curve::generator();
        t1.append_point(b"P", &g);
        t2.append_point(b"P", &(g + g));
        assert_ne!(t1.challenge_scalar(b"c"), t2.challenge_scalar(b"c"));
    }

    /// 派生连续两个 challenge 不能相同 (防 sigma 双 c 漏洞).
    #[test]
    fn successive_challenges_differ() {
        let mut t = Transcript::new(b"test");
        t.append_point(b"P", &Curve::generator());
        let c1 = t.challenge_scalar(b"c1");
        let c2 = t.challenge_scalar(b"c2");
        assert_ne!(c1, c2);
    }

    /// label 区分: append_message 用不同 label 应导致不同 challenge.
    #[test]
    fn different_label_different_challenge() {
        let mut t1 = Transcript::new(b"test");
        let mut t2 = Transcript::new(b"test");
        t1.append_message(b"a", b"x");
        t2.append_message(b"b", b"x");
        assert_ne!(t1.challenge_scalar(b"c"), t2.challenge_scalar(b"c"));
    }
}
