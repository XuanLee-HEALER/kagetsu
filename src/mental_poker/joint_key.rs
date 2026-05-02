//! 联合公钥 (M4.A.3): 4 玩家的 pk_i 经 Schnorr 验证后聚合成 PK = sum(pk_i).
//!
//! ## 为什么必须 verify Schnorr proof
//! "Rogue key attack": 如果只把 pk 直接相加, 攻击者可以发布
//! `pk_attacker = G - sum(其他 pk)`, 让 PK = G, sk = 1 (攻击者私下计算).
//! 攻击者就掌握了 PK 对应的 sk, 可以单方解密任何密文 (整桌零信任崩塌).
//!
//! 防御: 每个玩家广播 pk_i 时同时广播 Schnorr DLOG proof of knowledge of sk_i.
//! 验证通过才视为合法 pk_i, sum 之前先 reject 非法的.
//!
//! ## API 形态
//! [`aggregate`] 接 N 个 (peer_id, pk, schnorr_proof) 三元组, 全部验证通过 → 返回
//! [`JointPublicKey`]; 任一失败 → 返回 [`AggregateError`] 指出 offending peer index.
//!
//! ctx (proof binding) 用 peer_id 字节, 防止跨玩家重放他人 proof.

use thiserror::Error;

use super::elgamal::PublicKey;
use super::schnorr::{self, DlogProof};
use super::Curve;

/// 联合公钥. 4 个玩家的 pk_i 之和.
///
/// 加密一张牌时用 [`Self::as_pk`] 拿一个 [`PublicKey`] 接口.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JointPublicKey(pub Curve);

impl JointPublicKey {
    /// 借用为 [`PublicKey`] 接口 (e.g. 给 elgamal::mask 用).
    pub fn as_pk(&self) -> PublicKey {
        PublicKey(self.0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AggregateError {
    #[error("玩家 {index} 的 Schnorr DLOG 证明无效 (peer_id={peer_id_hex})")]
    InvalidProof { index: usize, peer_id_hex: String },

    #[error("成员数 {got} 不在合法范围内 (必须 ≥ 2)")]
    InsufficientMembers { got: usize },
}

/// 聚合 N 个玩家的公钥. 顺序 deterministic — caller 必须按一致顺序传入
/// (e.g. 按 peer_id 字节序排序), 否则各玩家算出的 PK 一致但顺序敏感的下游
/// 协议 (如 shuffle 顺序) 会乱.
///
/// 每个 entry = (peer_id_bytes 作 ctx, pk, schnorr_proof).
pub fn aggregate(
    members: &[(Vec<u8>, PublicKey, DlogProof)],
) -> Result<JointPublicKey, AggregateError> {
    if members.len() < 2 {
        return Err(AggregateError::InsufficientMembers {
            got: members.len(),
        });
    }

    // 1. 每人验证 schnorr proof.
    for (i, (peer_id, pk, proof)) in members.iter().enumerate() {
        if !schnorr::verify(pk, proof, peer_id) {
            return Err(AggregateError::InvalidProof {
                index: i,
                peer_id_hex: hex_short(peer_id),
            });
        }
    }

    // 2. sum.
    let aggregate = members.iter().map(|(_, pk, _)| pk.0).sum::<Curve>();
    Ok(JointPublicKey(aggregate))
}

/// peer_id 较短显示 (前 8 字节 hex), 给 error 消息用.
fn hex_short(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut out = String::with_capacity(take * 2);
    for b in &bytes[..take] {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::elgamal::{self, keygen, SecretKey};
    use crate::mental_poker::schnorr;
    use crate::mental_poker::Scalar;
    use ark_ec::PrimeGroup;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    fn make_member<R: ark_std::rand::Rng>(
        rng: &mut R,
        peer_id: &[u8],
    ) -> (SecretKey, (Vec<u8>, PublicKey, DlogProof)) {
        let (sk, pk) = keygen(rng);
        let proof = schnorr::prove(rng, &sk, &pk, peer_id);
        (sk, (peer_id.to_vec(), pk, proof))
    }

    /// 4 玩家 honest 聚合: PK = sum(pk_i), 用任一 sum(sk_i) 反向验证 sk
    /// 不应该跟 PK 直接对应单独某一方 (即整桌没人能独自解密).
    #[test]
    fn aggregate_4_honest_players() {
        let rng = &mut test_rng();
        let (sk_a, m_a) = make_member(rng, b"alice");
        let (sk_b, m_b) = make_member(rng, b"bob");
        let (sk_c, m_c) = make_member(rng, b"carol");
        let (sk_d, m_d) = make_member(rng, b"dave");
        let members = vec![m_a, m_b, m_c, m_d];

        let jpk = aggregate(&members).expect("4 honest");
        // PK 应等于 sum(sk_i) · G
        let total_sk = sk_a.0 + sk_b.0 + sk_c.0 + sk_d.0;
        let g = Curve::generator();
        assert_eq!(jpk.0, g * total_sk);
    }

    /// 单方 sk 不能解密 联合 PK 加密的密文 (核心安全性).
    #[test]
    fn single_sk_cannot_decrypt_joint_ciphertext() {
        let rng = &mut test_rng();
        let (_, m_a) = make_member(rng, b"alice");
        let (sk_b, m_b) = make_member(rng, b"bob");
        let members = vec![m_a, m_b];
        let jpk = aggregate(&members).expect("2 honest");

        // 用联合 PK 加密 message
        let message = Curve::rand(rng);
        let (ct, _) = elgamal::mask(rng, &jpk.as_pk(), &message);

        // 单方 sk_b 解密失败 (拿到的不是 message)
        let recovered = elgamal::unmask_with_sk(&sk_b, &ct);
        assert_ne!(recovered, message);
    }

    /// 篡改某玩家的 schnorr proof → aggregate 拒绝并指出 index.
    #[test]
    fn tampered_proof_rejected() {
        let rng = &mut test_rng();
        let (_, m_a) = make_member(rng, b"alice");
        let (_, mut m_b) = make_member(rng, b"bob");
        let (_, m_c) = make_member(rng, b"carol");
        let (_, m_d) = make_member(rng, b"dave");

        // 篡改 bob 的 proof
        m_b.2.z += Scalar::from(1u64);
        let members = vec![m_a, m_b, m_c, m_d];

        let err = aggregate(&members).expect_err("应被拒绝");
        match err {
            AggregateError::InvalidProof { index, .. } => assert_eq!(index, 1),
            other => panic!("期望 InvalidProof, 收到 {other:?}"),
        }
    }

    /// 太少成员 (<2) 拒绝聚合.
    #[test]
    fn insufficient_members_rejected() {
        let rng = &mut test_rng();
        let (_, m) = make_member(rng, b"alone");
        assert!(matches!(
            aggregate(&[m]),
            Err(AggregateError::InsufficientMembers { got: 1 })
        ));
        assert!(matches!(
            aggregate(&[]),
            Err(AggregateError::InsufficientMembers { got: 0 })
        ));
    }

    /// rogue key attack 防御: 攻击者 publish pk_attacker = -sum(其他真 pk),
    /// 但他没有 schnorr proof of knowledge of 对应 sk → aggregate 拒绝.
    #[test]
    fn rogue_key_attack_blocked() {
        let rng = &mut test_rng();
        let (_, m_a) = make_member(rng, b"alice");
        let (_, m_b) = make_member(rng, b"bob");
        let (_, m_c) = make_member(rng, b"carol");

        // 攻击者: 想让 PK = G (sk = 1), 所以 pk_attacker = G - pk_a - pk_b - pk_c.
        let g = Curve::generator();
        let rogue_pk = PublicKey(g - m_a.1.0 - m_b.1.0 - m_c.1.0);
        // 他不知道对应 sk, 伪造一个空 proof
        let fake_proof = DlogProof {
            a: g,
            z: Scalar::from(0u64),
        };
        let m_rogue = (b"attacker".to_vec(), rogue_pk, fake_proof);

        let members = vec![m_a, m_b, m_c, m_rogue];
        assert!(matches!(
            aggregate(&members),
            Err(AggregateError::InvalidProof { index: 3, .. })
        ));
    }

    /// 不同顺序入参 → 同一组人聚合出**同一 PK** (加法可交换), 但下游协议
    /// (shuffle 顺序) 必须用一致顺序. 这里只 sanity check 加法.
    #[test]
    fn aggregate_is_order_independent_for_pk() {
        let rng = &mut test_rng();
        let (_, m_a) = make_member(rng, b"alice");
        let (_, m_b) = make_member(rng, b"bob");
        let (_, m_c) = make_member(rng, b"carol");

        let jpk1 = aggregate(&[m_a.clone(), m_b.clone(), m_c.clone()]).unwrap();
        let jpk2 = aggregate(&[m_c, m_a, m_b]).unwrap();
        assert_eq!(jpk1, jpk2);
    }
}
