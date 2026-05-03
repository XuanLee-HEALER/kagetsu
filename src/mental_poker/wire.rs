//! P2P mental poker wire 协议 (M5.B.1).
//!
//! ZeroTrust 信任模型: 4 方对等通信, 不走房主中转. 本模块定义 P2P 协议消息
//! enum + 各原语的 byte-friendly 转换. 网络层 (libp2p request-response /
//! gossipsub) 拿 [`MentalPokerMsg`] 序列化为 cbor 发送; 收到后反序列化 +
//! 用 [`from_*_bytes`] 转回 runtime struct (Ciphertext / DleqProof 等).
//!
//! ## 编码分层
//! - **wire enum**: serde derive (cbor friendly), 各 variant 含 byte field
//! - **byte field**: ark-serialize compressed 形式 (~ 48 字节 / G1 point,
//!   32 字节 / Fr scalar)
//! - **解耦原因**: ark types (Curve / Scalar) 无 serde derive, 但有 ark-
//!   serialize. 用 bytes 桥接 — wire 层只 cbor, 应用层调 [`encode_*`] /
//!   [`decode_*`] 进出原语.
//!
//! ## 传输路径建议 (网络层在 M5.B.3+ 集成)
//! - **broadcast** (gossipsub `tui-majo/mp/{room_id}/v1`):
//!   KeyShare / ShuffleRound / RevealShare / Discard / Call / ConcealedKan / Win
//! - **request-response** (libp2p `/tui-majo/mp/v1`):
//!   DrawShareRequest → DrawShareResponse (X 主动请求其他人 share)
//! - **direct message** (request-response):
//!   ConcealedKanReveal (公开 announcement 后私发给 monitor)

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::cut_and_choose::{Opening, ShuffleProof};
use super::dleq::DleqProof;
use super::elgamal::{Ciphertext, PublicKey};
use super::protocol_draw::DecryptionShare;
use super::protocol_state::{CallType, ConcealedKanRecord};
use super::protocol_win::WinType;
use super::schnorr::DlogProof;
use super::shuffle::Permutation;
use super::{Curve, Scalar};

#[derive(Debug, Error)]
pub enum WireError {
    #[error("ark deserialize 失败: {0}")]
    ArkDecode(String),
    #[error("无效的 wire 格式: {0}")]
    Invalid(String),
}

// ============================================================================
// 各原语的 byte 转换帮手
// ============================================================================

fn ark_to_bytes<T: CanonicalSerialize>(t: &T) -> Vec<u8> {
    let mut buf = Vec::with_capacity(t.compressed_size());
    t.serialize_compressed(&mut buf)
        .expect("ark serialize never fails for valid struct");
    buf
}

fn ark_from_bytes<T: CanonicalDeserialize>(bytes: &[u8]) -> Result<T, WireError> {
    T::deserialize_compressed(bytes).map_err(|e| WireError::ArkDecode(e.to_string()))
}

pub fn encode_ciphertext(c: &Ciphertext) -> Vec<u8> {
    ark_to_bytes(c)
}
pub fn decode_ciphertext(b: &[u8]) -> Result<Ciphertext, WireError> {
    ark_from_bytes(b)
}

pub fn encode_pk(pk: &PublicKey) -> Vec<u8> {
    ark_to_bytes(pk)
}
pub fn decode_pk(b: &[u8]) -> Result<PublicKey, WireError> {
    ark_from_bytes(b)
}

pub fn encode_curve(p: &Curve) -> Vec<u8> {
    ark_to_bytes(p)
}
pub fn decode_curve(b: &[u8]) -> Result<Curve, WireError> {
    ark_from_bytes(b)
}

pub fn encode_scalar(s: &Scalar) -> Vec<u8> {
    ark_to_bytes(s)
}
pub fn decode_scalar(b: &[u8]) -> Result<Scalar, WireError> {
    ark_from_bytes(b)
}

pub fn encode_dlog_proof(p: &DlogProof) -> Vec<u8> {
    ark_to_bytes(p)
}
pub fn decode_dlog_proof(b: &[u8]) -> Result<DlogProof, WireError> {
    ark_from_bytes(b)
}

pub fn encode_dleq_proof(p: &DleqProof) -> Vec<u8> {
    ark_to_bytes(p)
}
pub fn decode_dleq_proof(b: &[u8]) -> Result<DleqProof, WireError> {
    ark_from_bytes(b)
}

pub fn encode_share(s: &DecryptionShare) -> Vec<u8> {
    ark_to_bytes(s)
}
pub fn decode_share(b: &[u8]) -> Result<DecryptionShare, WireError> {
    ark_from_bytes(b)
}

/// Vec<Ciphertext> 编码: length-prefix u32 + N × 96 bytes (compressed G1 × 2).
pub fn encode_ciphertext_vec(v: &[Ciphertext]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + v.len() * 96);
    buf.extend_from_slice(&(v.len() as u32).to_be_bytes());
    for c in v {
        buf.extend_from_slice(&encode_ciphertext(c));
    }
    buf
}

pub fn decode_ciphertext_vec(b: &[u8]) -> Result<Vec<Ciphertext>, WireError> {
    if b.len() < 4 {
        return Err(WireError::Invalid("ciphertext_vec 缺长度前缀".into()));
    }
    let n = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let body = &b[4..];
    // 每 ciphertext 96 字节 (2 个 compressed G1)
    if !body.len().is_multiple_of(96) || body.len() / 96 != n {
        return Err(WireError::Invalid(format!(
            "ciphertext_vec 长度不匹配: n={n}, body_len={}",
            body.len()
        )));
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let start = i * 96;
        out.push(decode_ciphertext(&body[start..start + 96])?);
    }
    Ok(out)
}

/// 序列化 ShuffleProof 为单个 byte blob (cnc proof 较大, 不暴露内部结构给 wire enum).
pub fn encode_shuffle_proof(p: &ShuffleProof) -> Result<Vec<u8>, WireError> {
    // intermediates: K × N × Ciphertext
    // openings: K × Opening (PreShuffle | PostShuffle)
    let mut buf = Vec::new();
    let k = p.intermediates.len();
    buf.extend_from_slice(&(k as u32).to_be_bytes());
    if k == 0 {
        return Ok(buf);
    }
    let n = p.intermediates[0].len();
    buf.extend_from_slice(&(n as u32).to_be_bytes());
    for inter in &p.intermediates {
        if inter.len() != n {
            return Err(WireError::Invalid("intermediates 长度不一致".into()));
        }
        for c in inter {
            buf.extend_from_slice(&encode_ciphertext(c));
        }
    }
    if p.openings.len() != k {
        return Err(WireError::Invalid("openings.len 跟 K 不匹配".into()));
    }
    for o in &p.openings {
        match o {
            Opening::PreShuffle { sigma, s } => {
                buf.push(0u8);
                encode_permutation_into(sigma, &mut buf);
                encode_scalar_vec_into(s, &mut buf);
            }
            Opening::PostShuffle { tau, t } => {
                buf.push(1u8);
                encode_permutation_into(tau, &mut buf);
                encode_scalar_vec_into(t, &mut buf);
            }
        }
    }
    Ok(buf)
}

pub fn decode_shuffle_proof(b: &[u8]) -> Result<ShuffleProof, WireError> {
    if b.len() < 4 {
        return Err(WireError::Invalid("shuffle_proof 太短".into()));
    }
    let k = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    if k == 0 {
        return Ok(ShuffleProof {
            intermediates: vec![],
            openings: vec![],
        });
    }
    if b.len() < 8 {
        return Err(WireError::Invalid("shuffle_proof 缺 N".into()));
    }
    let n = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let mut cur = 8;
    let mut intermediates = Vec::with_capacity(k);
    for _ in 0..k {
        let mut inter = Vec::with_capacity(n);
        for _ in 0..n {
            if cur + 96 > b.len() {
                return Err(WireError::Invalid("intermediates 数据短".into()));
            }
            inter.push(decode_ciphertext(&b[cur..cur + 96])?);
            cur += 96;
        }
        intermediates.push(inter);
    }
    let mut openings = Vec::with_capacity(k);
    for _ in 0..k {
        if cur >= b.len() {
            return Err(WireError::Invalid("opening 缺 tag".into()));
        }
        let tag = b[cur];
        cur += 1;
        let perm = decode_permutation_from(b, &mut cur)?;
        let scalars = decode_scalar_vec_from(b, &mut cur)?;
        match tag {
            0 => openings.push(Opening::PreShuffle {
                sigma: perm,
                s: scalars,
            }),
            1 => openings.push(Opening::PostShuffle {
                tau: perm,
                t: scalars,
            }),
            _ => return Err(WireError::Invalid(format!("未知 opening tag {tag}"))),
        }
    }
    Ok(ShuffleProof {
        intermediates,
        openings,
    })
}

fn encode_permutation_into(p: &Permutation, buf: &mut Vec<u8>) {
    let s = p.as_slice();
    buf.extend_from_slice(&(s.len() as u32).to_be_bytes());
    for v in s {
        buf.extend_from_slice(&(*v as u32).to_be_bytes());
    }
}

fn decode_permutation_from(b: &[u8], cur: &mut usize) -> Result<Permutation, WireError> {
    if *cur + 4 > b.len() {
        return Err(WireError::Invalid("permutation len 缺".into()));
    }
    let n = u32::from_be_bytes([b[*cur], b[*cur + 1], b[*cur + 2], b[*cur + 3]]) as usize;
    *cur += 4;
    if *cur + n * 4 > b.len() {
        return Err(WireError::Invalid("permutation body 缺".into()));
    }
    let mut indices = Vec::with_capacity(n);
    for _ in 0..n {
        let v = u32::from_be_bytes([b[*cur], b[*cur + 1], b[*cur + 2], b[*cur + 3]]) as usize;
        *cur += 4;
        indices.push(v);
    }
    Ok(Permutation::from_raw(indices))
}

fn encode_scalar_vec_into(v: &[Scalar], buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(v.len() as u32).to_be_bytes());
    for s in v {
        buf.extend_from_slice(&encode_scalar(s));
    }
}

fn decode_scalar_vec_from(b: &[u8], cur: &mut usize) -> Result<Vec<Scalar>, WireError> {
    if *cur + 4 > b.len() {
        return Err(WireError::Invalid("scalar_vec len 缺".into()));
    }
    let n = u32::from_be_bytes([b[*cur], b[*cur + 1], b[*cur + 2], b[*cur + 3]]) as usize;
    *cur += 4;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        // Fr compressed = 32 字节
        if *cur + 32 > b.len() {
            return Err(WireError::Invalid("scalar 数据短".into()));
        }
        out.push(decode_scalar(&b[*cur..*cur + 32])?);
        *cur += 32;
    }
    Ok(out)
}

// ============================================================================
// wire enum: P2P 协议消息
// ============================================================================

/// 跟 [`CallType`] 对应的 wire 表示.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireCallType {
    Chi,
    Pon,
    Kan,
}

impl From<CallType> for WireCallType {
    fn from(c: CallType) -> Self {
        match c {
            CallType::Chi => WireCallType::Chi,
            CallType::Pon => WireCallType::Pon,
            CallType::Kan => WireCallType::Kan,
        }
    }
}

impl From<WireCallType> for CallType {
    fn from(c: WireCallType) -> Self {
        match c {
            WireCallType::Chi => CallType::Chi,
            WireCallType::Pon => CallType::Pon,
            WireCallType::Kan => CallType::Kan,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireWinType {
    Tsumo,
    Ron { from_player: u32 },
}

impl From<WinType> for WireWinType {
    fn from(w: WinType) -> Self {
        match w {
            WinType::Tsumo => WireWinType::Tsumo,
            WinType::Ron { from_player } => WireWinType::Ron {
                from_player: from_player as u32,
            },
        }
    }
}

impl From<WireWinType> for WinType {
    fn from(w: WireWinType) -> Self {
        match w {
            WireWinType::Tsumo => WinType::Tsumo,
            WireWinType::Ron { from_player } => WinType::Ron {
                from_player: from_player as usize,
            },
        }
    }
}

/// P2P mental poker 协议消息 (4 方对等通信).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MentalPokerMsg {
    /// 协议 0 (开局 keygen): 玩家广播 (peer_id, pk, schnorr_proof).
    KeyShare {
        peer_id: Vec<u8>,
        pk: Vec<u8>,
        proof: Vec<u8>,
    },
    /// 协议 1: 玩家提交 shuffle round 输出 + cnc proof.
    ShuffleRound {
        round_idx: u32,
        new_deck: Vec<u8>,
        proof: Vec<u8>,
    },
    /// 协议 2 摸牌 — 摸牌方 X 请求 share.
    DrawShareRequest {
        request_id: Uuid,
        ct: Vec<u8>,
        deck_index: u32,
    },
    /// 协议 2 摸牌 — 玩家 i 回 X.
    DrawShareResponse {
        request_id: Uuid,
        share: Vec<u8>,
        proof: Vec<u8>,
    },
    /// 协议 2 摸牌 — 摸牌方收齐 + combine 后广播"我占有 deck[i]"以更新各方
    /// HandState (协议 4-7 require deck_index in drawn before discard/meld).
    /// 不含 plaintext (零信任安全): 只有摸牌方知 plaintext.
    DrawAnnouncement { player: u32, deck_index: u32 },
    /// 协议 3 公开揭示 — 4 方都广播.
    RevealShare {
        ct: Vec<u8>,
        share: Vec<u8>,
        proof: Vec<u8>,
    },
    /// 协议 4 弃牌.
    Discard {
        player: u32,
        deck_index: u32,
        plaintext: Vec<u8>,
    },
    /// 协议 5 鸣牌 (吃/碰/明杠).
    Call {
        player: u32,
        call_type: WireCallType,
        deck_indices: Vec<u32>,
        plaintexts: Vec<Vec<u8>>,
        from_player: u32,
        from_position_in_meld: u32,
    },
    /// 协议 6 暗杠公开 announcement (跟其它玩家说"我暗杠了 4 张, monitor 是 m").
    ConcealedKanAnnounce {
        player: u32,
        deck_indices: [u32; 4],
        monitor_player: u32,
    },
    /// 协议 6 私发给 monitor 的 reveal: 4 张 plaintext.
    ConcealedKanReveal { plaintexts: [Vec<u8>; 4] },
    /// 协议 7 和牌广播.
    Win {
        player: u32,
        win_type: WireWinType,
        hand_indices: Vec<u32>,
        hand_plaintexts: Vec<Vec<u8>>,
        winning_tile_index: u32,
        dora_plaintexts: Vec<Vec<u8>>,
        uradoor_plaintexts: Option<Vec<Vec<u8>>>,
    },
    /// 加杠 (M6.B Shouminkan) — 把已有 Pon meld 升级为 Kan.
    /// player 公开广播: 我把 deck[new_deck_index] (plaintext = same as Pon)
    /// 加进我的 melds[target_meld_idx] (Pon → Kan).
    Shouminkan {
        player: u32,
        target_meld_idx: u32,
        new_deck_index: u32,
        new_plaintext: Vec<u8>,
    },
}

impl MentalPokerMsg {
    /// helper: 给 ConcealedKanRecord 转 announce variant.
    pub fn from_concealed_kan(player: usize, k: &ConcealedKanRecord) -> Self {
        MentalPokerMsg::ConcealedKanAnnounce {
            player: player as u32,
            deck_indices: [
                k.deck_indices[0] as u32,
                k.deck_indices[1] as u32,
                k.deck_indices[2] as u32,
                k.deck_indices[3] as u32,
            ],
            monitor_player: k.monitor_player as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::Curve;
    use crate::mental_poker::elgamal::{keygen, mask};
    use crate::mental_poker::schnorr;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    #[test]
    fn ciphertext_byte_roundtrip() {
        let rng = &mut test_rng();
        let (_, pk) = keygen(rng);
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &pk, &m);
        let bytes = encode_ciphertext(&ct);
        // BLS12-381 G1 compressed = 48 字节, 2 个 = 96
        assert_eq!(bytes.len(), 96);
        let back = decode_ciphertext(&bytes).unwrap();
        assert_eq!(ct, back);
    }

    #[test]
    fn dlog_proof_byte_roundtrip() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let proof = schnorr::prove(rng, &sk, &pk, b"test");
        let bytes = encode_dlog_proof(&proof);
        let back = decode_dlog_proof(&bytes).unwrap();
        assert_eq!(proof, back);
    }

    #[test]
    fn dleq_proof_byte_roundtrip() {
        use crate::mental_poker::dleq;
        use ark_ec::PrimeGroup;
        let rng = &mut test_rng();
        let x = Scalar::rand(rng);
        let g1 = Curve::generator();
        let g2 = Curve::rand(rng);
        let h1 = g1 * x;
        let h2 = g2 * x;
        let proof = dleq::prove(rng, &x, &g1, &h1, &g2, &h2, b"ctx");
        let bytes = encode_dleq_proof(&proof);
        let back = decode_dleq_proof(&bytes).unwrap();
        assert_eq!(proof, back);
    }

    #[test]
    fn ciphertext_vec_byte_roundtrip() {
        let rng = &mut test_rng();
        let (_, pk) = keygen(rng);
        let v: Vec<Ciphertext> = (0..5)
            .map(|_| {
                let m = Curve::rand(rng);
                mask(rng, &pk, &m).0
            })
            .collect();
        let bytes = encode_ciphertext_vec(&v);
        // 4 (length) + 5 × 96 = 484
        assert_eq!(bytes.len(), 4 + 5 * 96);
        let back = decode_ciphertext_vec(&bytes).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn shuffle_proof_byte_roundtrip() {
        use crate::mental_poker::cut_and_choose;
        use crate::mental_poker::shuffle::shuffle_and_remask;
        let rng = &mut test_rng();
        let (_, pk) = keygen(rng);
        let n = 8usize;
        let deck: Vec<Ciphertext> = (0..n)
            .map(|_| {
                let m = Curve::rand(rng);
                mask(rng, &pk, &m).0
            })
            .collect();
        let (out, pi, r) = shuffle_and_remask(rng, &pk, &deck);
        let proof = cut_and_choose::prove(rng, &pk, &deck, &out, &pi, &r, 8);

        let bytes = encode_shuffle_proof(&proof).unwrap();
        let back = decode_shuffle_proof(&bytes).unwrap();
        assert_eq!(back.intermediates.len(), proof.intermediates.len());
        assert_eq!(back.openings.len(), proof.openings.len());
        for (a, b) in proof.intermediates.iter().zip(back.intermediates.iter()) {
            assert_eq!(a, b);
        }
        // verify back proof 仍正确 (sanity: roundtrip 没破坏 cnc proof)
        assert!(cut_and_choose::verify(&pk, &deck, &out, &back));
    }

    #[test]
    fn wire_msg_keyshare_serde_roundtrip() {
        let msg = MentalPokerMsg::KeyShare {
            peer_id: b"alice".to_vec(),
            pk: vec![1, 2, 3, 4],
            proof: vec![5, 6, 7],
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"key_share\""));
        let back: MentalPokerMsg = serde_json::from_str(&s).unwrap();
        match back {
            MentalPokerMsg::KeyShare { peer_id, pk, proof } => {
                assert_eq!(peer_id, b"alice");
                assert_eq!(pk, vec![1, 2, 3, 4]);
                assert_eq!(proof, vec![5, 6, 7]);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn wire_msg_draw_request_uuid_preserved() {
        let id = Uuid::new_v4();
        let msg = MentalPokerMsg::DrawShareRequest {
            request_id: id,
            ct: vec![1, 2],
            deck_index: 7,
        };
        let s = serde_json::to_string(&msg).unwrap();
        let back: MentalPokerMsg = serde_json::from_str(&s).unwrap();
        match back {
            MentalPokerMsg::DrawShareRequest {
                request_id,
                deck_index,
                ..
            } => {
                assert_eq!(request_id, id);
                assert_eq!(deck_index, 7);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn wire_msg_call_full_roundtrip() {
        let msg = MentalPokerMsg::Call {
            player: 0,
            call_type: WireCallType::Pon,
            deck_indices: vec![1, 2, 50],
            plaintexts: vec![vec![1; 48], vec![2; 48], vec![3; 48]],
            from_player: 1,
            from_position_in_meld: 2,
        };
        let s = serde_json::to_string(&msg).unwrap();
        let back: MentalPokerMsg = serde_json::from_str(&s).unwrap();
        match back {
            MentalPokerMsg::Call {
                player,
                call_type,
                from_player,
                deck_indices,
                ..
            } => {
                assert_eq!(player, 0);
                assert_eq!(call_type, WireCallType::Pon);
                assert_eq!(from_player, 1);
                assert_eq!(deck_indices, vec![1, 2, 50]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn call_type_bidirectional_conversion() {
        for c in [CallType::Chi, CallType::Pon, CallType::Kan] {
            let w: WireCallType = c.into();
            let back: CallType = w.into();
            assert_eq!(c, back);
        }
    }

    #[test]
    fn win_type_bidirectional_conversion() {
        let t = WinType::Tsumo;
        let back: WinType = WireWinType::from(t).into();
        assert!(matches!(back, WinType::Tsumo));

        let r = WinType::Ron { from_player: 3 };
        let back: WinType = WireWinType::from(r).into();
        match back {
            WinType::Ron { from_player } => assert_eq!(from_player, 3),
            _ => panic!(),
        }
    }

    #[test]
    fn corrupt_bytes_decode_fail() {
        // Ciphertext 期 96 字节, 给 4 字节 → ArkDecode 错误
        let r = decode_ciphertext(&[0u8; 4]);
        assert!(matches!(r, Err(WireError::ArkDecode(_))));
        // ciphertext_vec 长度声明 N=2 但 body 只 96 → Invalid
        let mut bad = (2u32).to_be_bytes().to_vec();
        bad.extend_from_slice(&[0u8; 96]); // 只 1 个 ciphertext
        let r = decode_ciphertext_vec(&bad);
        assert!(matches!(r, Err(WireError::Invalid(_))));
    }

    #[test]
    fn empty_shuffle_proof_roundtrip() {
        let p = ShuffleProof {
            intermediates: vec![],
            openings: vec![],
        };
        let bytes = encode_shuffle_proof(&p).unwrap();
        let back = decode_shuffle_proof(&bytes).unwrap();
        assert!(back.intermediates.is_empty());
        assert!(back.openings.is_empty());
    }
}
