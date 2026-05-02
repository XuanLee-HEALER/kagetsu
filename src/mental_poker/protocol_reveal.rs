//! 协议 3: 公开揭示 — M4.E.
//!
//! ## 用例
//! - **dora indicator** 揭示 (开局 / 杠 / 立直)
//! - **立直暗杠开杠** 揭示
//! - **自摸 / 荣和** 时全手牌揭示给其他玩家审计
//!
//! ## 协议
//! N 个持 sk_i 的玩家全部 broadcast (share_i, dleq_proof_i):
//! 1. 每人独立计算 share_i = sk_i · c.c1 + DLEQ proof
//! 2. 每人 broadcast (share_i, proof_i, peer_id_i)
//! 3. 任何接收方 (含玩家自己) 验证全部 N 个 proof
//! 4. 任何接收方 combine N 个 share 恢复 plaintext = c.c2 - sum(share_i)
//! 5. 任何接收方反查 Tile mapping 还原具体 Tile
//!
//! ## 跟协议 2 (摸牌) 的区别
//! - 协议 2: **仅一方** (摸牌者 X) 拿到 plaintext, X 收齐 N-1 share 后用自己 sk_X
//!   算第 N 个; 其他人**只 broadcast share 给 X**, 不自己 combine.
//! - 协议 3: **所有人** broadcast share, 所有人都 combine.
//!
//! 底层原语 [`compute_share`] / [`verify_share`] / [`combine_shares`] 复用自
//! [`crate::mental_poker::protocol_draw`] (协议 2). 本模块提供高层 multi-party
//! API + e2e helper.
//!
//! ## 安全性
//! - 任一方拒绝合作 / 发错 share (DLEQ verify 失败) → 揭示流程中止, 该方暴露.
//! - 全部 N 个 honest share 才能成功揭示 — 即使 N-1 共谋, 缺最后 1 个就揭不出.
//!   这是 mental poker 的根本性质 (单方拒绝阻断协议).

use ark_std::rand::Rng;
use thiserror::Error;

use super::dleq::DleqProof;
use super::elgamal::{Ciphertext, PublicKey, SecretKey};
use super::protocol_draw::{
    self, combine_shares as draw_combine_shares, compute_share as draw_compute_share,
    verify_share as draw_verify_share, DecryptionShare,
};
use super::Curve;

/// 一个玩家提交的 reveal 包: (share, dleq_proof). 跟 [`MemberInfo`] 配对组成
/// 完整可验证的 reveal contribution.
#[derive(Debug, Clone, Copy)]
pub struct RevealShare {
    pub share: DecryptionShare,
    pub proof: DleqProof,
}

/// 揭示协议的 multi-party 公开成员信息 (任何人都用同一份 verify).
#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub peer_id: Vec<u8>,
    pub pk: PublicKey,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RevealError {
    #[error("成员数 {got} 跟 share 数 {expected} 不一致")]
    SizeMismatch { got: usize, expected: usize },
    #[error("成员 {index} 的 share DLEQ 验证失败 (peer_id={peer_id})")]
    InvalidShare { index: usize, peer_id: String },
}

/// 玩家 i 准备自己的 reveal 包 — share + DLEQ proof. 只是 [`compute_share`]
/// 的语义化薄封装.
pub fn prepare_share<R: Rng + ?Sized>(
    rng: &mut R,
    sk: &SecretKey,
    pk: &PublicKey,
    ct: &Ciphertext,
    peer_id: &[u8],
) -> RevealShare {
    let (share, proof) = draw_compute_share(rng, sk, pk, ct, peer_id);
    RevealShare { share, proof }
}

/// 验证单个 reveal 包. (peer_pk 应跟 ctx 一致, caller 负责.)
pub fn verify_one(
    pk: &PublicKey,
    ct: &Ciphertext,
    contribution: &RevealShare,
    peer_id: &[u8],
) -> bool {
    draw_verify_share(pk, ct, &contribution.share, &contribution.proof, peer_id)
}

/// 公开揭示完整流程: 验证全部 N 个 share, combine 恢复明文.
///
/// 顺序敏感: members[i] 跟 contributions[i] 必须对应. 失败定位: error 含
/// 第一个 invalid share 的 index 和 peer_id, 调用方可踢人.
///
/// 成功返回明文 (Curve point). 反查 Tile 是 application 层职责.
pub fn public_reveal(
    members: &[MemberInfo],
    ct: &Ciphertext,
    contributions: &[RevealShare],
) -> Result<Curve, RevealError> {
    if members.len() != contributions.len() {
        return Err(RevealError::SizeMismatch {
            got: contributions.len(),
            expected: members.len(),
        });
    }

    for (i, (m, c)) in members.iter().zip(contributions.iter()).enumerate() {
        if !verify_one(&m.pk, ct, c, &m.peer_id) {
            return Err(RevealError::InvalidShare {
                index: i,
                peer_id: hex_short(&m.peer_id),
            });
        }
    }

    let shares: Vec<DecryptionShare> = contributions.iter().map(|c| c.share).collect();
    Ok(draw_combine_shares(ct, &shares))
}

fn hex_short(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut out = String::with_capacity(take * 2);
    for b in &bytes[..take] {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// 重新 export protocol_draw 原语作 alias, 让 protocol 3 调用方一站式 import.
pub use protocol_draw::DecryptionShare as Share;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::elgamal::{keygen, mask};
    use crate::mental_poker::joint_key::aggregate;
    use crate::mental_poker::schnorr;
    use crate::mental_poker::Curve;
    use ark_ec::PrimeGroup;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    /// 工具: 4 玩家 setup, 返回 (sks, pks, members, jpk).
    fn setup_4_players() -> (
        Vec<SecretKey>,
        Vec<PublicKey>,
        Vec<MemberInfo>,
        crate::mental_poker::joint_key::JointPublicKey,
    ) {
        let rng = &mut test_rng();
        let mut sks = Vec::new();
        let mut pks = Vec::new();
        let mut members = Vec::new();
        let mut entries = Vec::new();
        for i in 0..4 {
            let peer_id = format!("p{i}").into_bytes();
            let (sk, pk) = keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            pks.push(pk);
            members.push(MemberInfo {
                peer_id: peer_id.clone(),
                pk,
            });
            entries.push((peer_id, pk, proof));
        }
        let jpk = aggregate(&entries).unwrap();
        (sks, pks, members, jpk)
    }

    /// 标准 4 方公开揭示: dora indicator 场景.
    #[test]
    fn protocol_3_public_reveal_4_players() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();

        // 加密 1 张牌作 dora indicator.
        let dora_tile = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &dora_tile);

        // 4 人各自 prepare share.
        let contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();

        // 任一玩家 (e.g. 自己) public_reveal — 全部都该拿到 dora_tile.
        let recovered = public_reveal(&members, &ct, &contributions).expect("honest 4-party");
        assert_eq!(recovered, dora_tile);
    }

    /// cheating: 一个玩家发错 share, 揭示失败 + 指出具体哪个玩家.
    #[test]
    fn protocol_3_cheating_player_localized() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        let mut contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();

        // cheater = 玩家 2: 篡改 share value
        contributions[2].share.0 += Curve::generator();

        let err = public_reveal(&members, &ct, &contributions).expect_err("cheater detected");
        match err {
            RevealError::InvalidShare { index, .. } => assert_eq!(index, 2),
            other => panic!("期望 InvalidShare, 收到 {other:?}"),
        }
    }

    /// 错乱 contributions / members 配对 → DLEQ ctx 不一致 → 失败.
    #[test]
    fn protocol_3_misaligned_contributions_fail() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        let mut contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();

        // 交换 contributions[0] 和 contributions[1] (peer_id 不匹配 share 来源)
        contributions.swap(0, 1);

        let err = public_reveal(&members, &ct, &contributions).expect_err("misaligned");
        // 第一个 mismatch 的 index 应 = 0
        match err {
            RevealError::InvalidShare { index, .. } => assert!(index == 0 || index == 1),
            other => panic!("期望 InvalidShare, 收到 {other:?}"),
        }
    }

    /// 缺一个 contribution: SizeMismatch 错误.
    #[test]
    fn protocol_3_missing_contribution() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        let mut contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();
        contributions.pop(); // 只剩 3 个

        let err = public_reveal(&members, &ct, &contributions).expect_err("size mismatch");
        assert!(matches!(
            err,
            RevealError::SizeMismatch { got: 3, expected: 4 }
        ));
    }

    /// 多张牌 (e.g. 多张 dora) 批量揭示, 都正确.
    #[test]
    fn protocol_3_batch_reveal() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();

        let n_dora = 5;
        let tiles: Vec<Curve> = (0..n_dora).map(|_| Curve::rand(rng)).collect();
        let cts: Vec<Ciphertext> = tiles.iter().map(|t| mask(rng, &jpk.as_pk(), t).0).collect();

        for (tile, ct) in tiles.iter().zip(cts.iter()) {
            let contributions: Vec<RevealShare> = (0..4)
                .map(|i| prepare_share(rng, &sks[i], &pks[i], ct, &members[i].peer_id))
                .collect();
            let recovered =
                public_reveal(&members, ct, &contributions).expect("each ct reveals");
            assert_eq!(recovered, *tile);
        }
    }

    /// 协议 2 (摸牌) 和协议 3 (公开揭示) 在同一 ct 上得到相同 plaintext (sanity).
    /// 验证两个协议复用底层原语没出错.
    #[test]
    fn protocol_2_and_3_agree_on_plaintext() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        // 协议 2 视角: 收 4 个 share, combine 拿明文.
        let shares_p2: Vec<DecryptionShare> = (0..4)
            .map(|i| {
                let (s, _) = draw_compute_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id);
                s
            })
            .collect();
        let m_p2 = draw_combine_shares(&ct, &shares_p2);

        // 协议 3 视角: 全部 prepare + verify + combine.
        let contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();
        let m_p3 = public_reveal(&members, &ct, &contributions).unwrap();

        assert_eq!(m_p2, m_p3);
        assert_eq!(m_p2, m);
    }

    /// fake DLEQ proof (用别人 sk 算 share + proof) → caught.
    #[test]
    fn protocol_3_fake_proof_caught() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        let mut contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();

        // attacker (在 i=1) 用别人 sk_2 算 share 但宣称是自己提交
        // 即 share = sk_2 · c1, peer_id = "p1" (members[1])
        let (fake_share, fake_proof) =
            draw_compute_share(rng, &sks[2], &pks[2], &ct, &members[1].peer_id);
        contributions[1] = RevealShare {
            share: fake_share,
            proof: fake_proof,
        };

        // members[1].pk = pks[1] (玩家 1 的真 pk), 不是 pks[2]
        // DLEQ((G, pks[1]), (c1, fake_share)) — fake_share = sk_2 · c1, 不满足 sk_1 关系
        let err = public_reveal(&members, &ct, &contributions).expect_err("attacker caught");
        assert!(matches!(err, RevealError::InvalidShare { index: 1, .. }));
    }

    /// 全部 4 玩家 honest 都能独立 verify (不同接收者得同一结果).
    #[test]
    fn protocol_3_all_receivers_agree() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        let contributions: Vec<RevealShare> = (0..4)
            .map(|i| prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id))
            .collect();

        // 每个玩家独立 public_reveal 应得同一 plaintext.
        let mut results = Vec::new();
        for _ in 0..4 {
            results.push(public_reveal(&members, &ct, &contributions).unwrap());
        }
        for r in &results {
            assert_eq!(*r, m);
        }
    }
}
