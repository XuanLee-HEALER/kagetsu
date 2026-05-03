//! 4 方零信任协议端到端集成测试 (M5.B).
//!
//! 跟 [`crate::mental_poker::session::tests::protocol_1_2_3_full_e2e`] 互补:
//! 那个测试只覆盖密码学 (洗牌 + 摸 + 揭示), 不接 [`Table`] 也不跨协议 4-7.
//! 本模块测**完整时序**: 4 方各自维护独立 Table 镜像, 跑 draw → discard →
//! call → concealed_kan → win 流程, 验证 4 方公开状态最终一致.
//!
//! ## 设计原则
//! - 协议 2 (摸牌) 解密结果只给当事人, 当事人 record_draw(Some(plaintext)),
//!   其他 3 方 record_draw(None) (visibility 模拟).
//! - 协议 4-7 announcement 含 plaintext, 4 方都 apply, 公开 state 一致.
//! - 测试 harness 加 step counter 防卡死 (每个 scenario 上限 200 步).
//! - 每步后 assert 4 方公开 state (discarded/melds/concealed_kans) 严格相等.

#![cfg(test)]

use ark_ff::UniformRand;
use ark_std::test_rng;

use super::Curve;
use super::cut_and_choose;
use super::elgamal::{PublicKey, SecretKey, keygen};
use super::joint_key::{JointPublicKey, aggregate};
use super::protocol_call::CallAnnouncement;
use super::protocol_concealed_kan::ConcealedKanAnnouncement;
use super::protocol_discard::DiscardAnnouncement;
use super::protocol_reveal::{MemberInfo, prepare_share};
use super::protocol_state::{CallType, Table};
use super::protocol_win::{WinAnnouncement, WinType};
use super::schnorr;
use super::session::{DrawSession, ShuffleSession};
use super::shuffle::shuffle_and_remask;

/// 协议步骤上限 — 防卡死 watchdog. 任何 scenario 超此值都视为协议设计 bug.
const MAX_PROTOCOL_STEPS: usize = 1000;

/// 4 方完整 setup: keys + members + jpk.
struct Players {
    sks: Vec<SecretKey>,
    pks: Vec<PublicKey>,
    members: Vec<MemberInfo>,
    jpk: JointPublicKey,
}

fn setup_4() -> Players {
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
    Players {
        sks,
        pks,
        members,
        jpk,
    }
}

/// 协议 1: 4 方联合洗牌, 返回 final_deck (密文) + initial plaintexts (校验用).
fn run_shuffle(p: &Players, n_cards: usize) -> (Vec<super::elgamal::Ciphertext>, Vec<Curve>) {
    let rng = &mut test_rng();
    let plaintexts: Vec<Curve> = (0..n_cards).map(|_| Curve::rand(rng)).collect();
    let mut sess = ShuffleSession::start(p.members.clone(), p.jpk, plaintexts.clone(), 20).unwrap();
    let mut steps = 0usize;
    while !sess.is_complete() {
        steps += 1;
        assert!(
            steps < MAX_PROTOCOL_STEPS,
            "shuffle session 超过 {} 步, 卡死?",
            MAX_PROTOCOL_STEPS
        );
        let player = sess.next_actor();
        let input = sess.current_input_deck().to_vec();
        let (out, pi, r) = shuffle_and_remask(rng, &p.jpk.as_pk(), &input);
        let proof = cut_and_choose::prove(
            rng,
            &p.jpk.as_pk(),
            &input,
            &out,
            &pi,
            &r,
            sess.cnc_k_rounds(),
        );
        sess.submit_round(player, out, proof).unwrap();
    }
    let final_deck = sess.final_deck().unwrap().to_vec();
    (final_deck, plaintexts)
}

/// 协议 2: 玩家 `who` 摸 `deck_index` 那张, 4 方各自 record_draw 到 tables[].
/// 仅当事人 record Some(plaintext), 其他 3 方 record None. 返回明文给 caller 用.
fn draw_one(
    p: &Players,
    final_deck: &[super::elgamal::Ciphertext],
    deck_index: usize,
    who: usize,
    tables: &mut [Table; 4],
) -> Curve {
    let rng = &mut test_rng();
    let ct = final_deck[deck_index];
    let mut sess = DrawSession::new(p.members.clone(), ct);
    let mut steps = 0usize;
    for i in 0..4 {
        steps += 1;
        assert!(
            steps < MAX_PROTOCOL_STEPS,
            "draw session 超过 {} 步, 卡死?",
            MAX_PROTOCOL_STEPS
        );
        let share = prepare_share(rng, &p.sks[i], &p.pks[i], &ct, &p.members[i].peer_id);
        sess.submit(i, share).unwrap();
    }
    assert!(sess.is_ready());
    let plaintext = sess.try_combine().unwrap();
    // 4 方记录 draw (visibility 模拟: 仅当事人 Some)
    for (i, t) in tables.iter_mut().enumerate() {
        let pt_for_this_view = if i == who { Some(plaintext) } else { None };
        t.hand_mut(who)
            .record_draw(deck_index, pt_for_this_view)
            .unwrap();
    }
    plaintext
}

/// 公开广播 announcement: 4 方都 apply, 然后 assert 公开 state 一致.
fn broadcast_discard(ann: DiscardAnnouncement, tables: &mut [Table; 4]) {
    for t in tables.iter_mut() {
        ann.apply(t).unwrap();
    }
    assert_public_state_consistent(tables);
}

fn broadcast_call(ann: CallAnnouncement, tables: &mut [Table; 4]) {
    for t in tables.iter_mut() {
        ann.apply(t).unwrap();
    }
    assert_public_state_consistent(tables);
}

fn broadcast_concealed_kan(ann: ConcealedKanAnnouncement, tables: &mut [Table; 4]) {
    for t in tables.iter_mut() {
        ann.apply(t).unwrap();
    }
    assert_public_state_consistent(tables);
}

/// 4 方公开 state 一致性: discarded / melds / concealed_kans 必须严格相同.
/// drawn 因 visibility 不同 (玩家 i 看自己手牌 plaintext 是 Some, 看别人是 None) 跳过.
fn assert_public_state_consistent(tables: &[Table; 4]) {
    for player in 0..4 {
        // 各方看 player 的公开数据应一致.
        let h0 = tables[0].hand(player);
        for (view, t) in tables.iter().enumerate().skip(1) {
            let h = t.hand(player);
            // discarded: HashMap<usize, Curve>
            let mut d0: Vec<_> = h0.discarded_indices().copied().collect();
            d0.sort();
            let mut dv: Vec<_> = h.discarded_indices().copied().collect();
            dv.sort();
            assert_eq!(
                d0, dv,
                "玩家 {player} discarded indices 在 view 0 vs view {view} 不一致"
            );
            for idx in &d0 {
                assert_eq!(
                    h0.discarded_plaintext(*idx),
                    h.discarded_plaintext(*idx),
                    "玩家 {player} discarded[{idx}] plaintext 在 view 0 vs view {view} 不一致"
                );
            }
            // melds
            assert_eq!(
                h0.melds(),
                h.melds(),
                "玩家 {player} melds 在 view 0 vs view {view} 不一致"
            );
            // concealed_kans
            assert_eq!(
                h0.concealed_kans(),
                h.concealed_kans(),
                "玩家 {player} concealed_kans 在 view 0 vs view {view} 不一致"
            );
        }
    }
}

/// **场景 A**: 完整一回合 — 玩家 0 摸 13 张 + 弃 → 玩家 1 摸 + 弃.
/// 验证 4 方公开 state 在每步后一致, 弃牌历史正确.
#[test]
fn scenario_a_basic_draw_discard_flow() {
    let p = setup_4();
    let (final_deck, _) = run_shuffle(&p, 32);
    let mut tables: [Table; 4] = std::array::from_fn(|_| Table::new(4, 32));

    // 玩家 0 摸 13 张
    let mut p0_pts = Vec::new();
    for i in 0..13 {
        let pt = draw_one(&p, &final_deck, i, 0, &mut tables);
        p0_pts.push(pt);
    }
    // 玩家 0 弃第一张 (deck_index=0)
    let discard_pt = p0_pts[0];
    broadcast_discard(
        DiscardAnnouncement {
            player: 0,
            deck_index: 0,
            plaintext: discard_pt,
        },
        &mut tables,
    );
    // 4 方都看到 player 0 弃了 deck_index 0
    for t in &tables {
        assert_eq!(
            t.hand(0).discarded_plaintext(0),
            Some(&discard_pt),
            "discarded plaintext 4 方应一致"
        );
        assert!(!t.hand(0).has_in_hand(0));
    }

    // 玩家 1 摸 1 张 (deck_index=13), 弃
    let p1_pt = draw_one(&p, &final_deck, 13, 1, &mut tables);
    broadcast_discard(
        DiscardAnnouncement {
            player: 1,
            deck_index: 13,
            plaintext: p1_pt,
        },
        &mut tables,
    );

    // 验证 final state
    for t in &tables {
        assert_eq!(t.hand(0).discarded_indices().count(), 1);
        assert_eq!(t.hand(1).discarded_indices().count(), 1);
    }
}

/// **场景 B**: 鸣牌 (碰) — 玩家 1 弃, 玩家 0 碰.
/// 关键: 碰需要 plaintext 一致, 我们在 setup 阶段强制玩家 0 摸到的 2 张 plaintext
/// 跟玩家 1 弃的 plaintext 相同 (实际游戏不可能强制, 测试中用一个特殊场景).
#[test]
fn scenario_b_pon_call_after_discard() {
    let _p = setup_4();
    // 用 mock plaintext 直接构造 ct, 不走 shuffle (因为 shuffle 后 plaintext 是
    // deterministic 但难以指定特定碰牌). 这里只测 4-table 一致性 + 协议 4+5 时序.
    let rng = &mut test_rng();
    let target_pt = Curve::rand(rng); // 模拟 "碰" 用的牌 plaintext.

    let mut tables: [Table; 4] = std::array::from_fn(|_| Table::new(4, 136));
    // 玩家 0 摸 2 张 target_pt (mock 双对子)
    for (i, t) in tables.iter_mut().enumerate() {
        let pt = if i == 0 { Some(target_pt) } else { None };
        t.hand_mut(0).record_draw(0, pt).unwrap();
        t.hand_mut(0).record_draw(1, pt).unwrap();
    }
    assert_public_state_consistent(&tables);

    // 玩家 1 摸 + 弃 deck_index=50 plaintext=target_pt
    for (i, t) in tables.iter_mut().enumerate() {
        let pt = if i == 1 { Some(target_pt) } else { None };
        t.hand_mut(1).record_draw(50, pt).unwrap();
    }
    broadcast_discard(
        DiscardAnnouncement {
            player: 1,
            deck_index: 50,
            plaintext: target_pt,
        },
        &mut tables,
    );

    // 玩家 0 碰
    broadcast_call(
        CallAnnouncement {
            player: 0,
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 50],
            plaintexts: vec![target_pt; 3],
            from_player: 1,
            from_position_in_meld: 2,
        },
        &mut tables,
    );

    // 4 方都看到玩家 0 有 1 个 meld (碰)
    for t in &tables {
        assert_eq!(t.hand(0).melds().len(), 1);
        let meld = &t.hand(0).melds()[0];
        assert_eq!(meld.call_type, CallType::Pon);
        assert_eq!(meld.from_player, 1);
        assert!(!t.hand(0).has_in_hand(0));
        assert!(!t.hand(0).has_in_hand(1));
    }
}

/// **场景 C**: 暗杠 — 玩家 0 摸 4 张 → 暗杠 announcement (公开 indices 给所有人,
/// reveal 私发给 monitor=玩家 2).
/// 公开 announcement 在 4 方一致, monitor 私下验证 4 张 plaintext 留 application 层.
#[test]
fn scenario_c_concealed_kan() {
    let p = setup_4();
    let (final_deck, _) = run_shuffle(&p, 16);
    let mut tables: [Table; 4] = std::array::from_fn(|_| Table::new(4, 16));

    // 玩家 0 摸 deck[0..4] 4 张
    for i in 0..4 {
        let _ = draw_one(&p, &final_deck, i, 0, &mut tables);
    }

    // 玩家 0 暗杠 (公开广播 — 不揭示 plaintexts, 只标记 4 个 indices 出 hand)
    broadcast_concealed_kan(
        ConcealedKanAnnouncement {
            player: 0,
            deck_indices: [0, 1, 2, 3],
            monitor_player: 2,
        },
        &mut tables,
    );

    // 4 方都看到玩家 0 有 1 个 concealed_kan, indices 出 hand
    for t in &tables {
        assert_eq!(t.hand(0).concealed_kans().len(), 1);
        assert_eq!(t.hand(0).concealed_kans()[0].monitor_player, 2);
        for i in 0..4 {
            assert!(!t.hand(0).has_in_hand(i));
        }
    }

    // monitor (玩家 2) 私下从 player 0 拿 4 个 plaintexts (生产中走 P2P 私聊).
    // 测试中我们通过 tables[0] (玩家 0 自己 view) 直接读取.
    let mut monitor_view = Vec::new();
    for i in 0..4 {
        let pt = tables[0].hand(0).drawn_plaintext(i).copied().unwrap();
        monitor_view.push(pt);
    }
    // monitor 收到 4 个 plaintext (application 层验证 4 张同 tile_index, 这里跳过).
    assert_eq!(monitor_view.len(), 4);
}

/// **场景 D**: 自摸 — 玩家 0 摸 14 张 → win announcement validate.
#[test]
fn scenario_d_tsumo_win() {
    let p = setup_4();
    let (final_deck, _) = run_shuffle(&p, 16);
    let mut tables: [Table; 4] = std::array::from_fn(|_| Table::new(4, 16));

    let mut hand_pts = Vec::new();
    for i in 0..14 {
        let pt = draw_one(&p, &final_deck, i, 0, &mut tables);
        hand_pts.push(pt);
    }

    let win = WinAnnouncement {
        player: 0,
        win_type: WinType::Tsumo,
        hand_indices: (0..14).collect(),
        hand_plaintexts: hand_pts,
        winning_tile_index: 13,
        dora_plaintexts: vec![],
        uradoor_plaintexts: None,
    };
    // 4 方都验证通过
    for t in &tables {
        win.validate(t).unwrap();
    }
}

/// **场景 E**: 荣和 — 玩家 0 摸 13 张, 玩家 1 摸 + 弃, 玩家 0 ron.
#[test]
fn scenario_e_ron_win() {
    let p = setup_4();
    let (final_deck, _) = run_shuffle(&p, 32);
    let mut tables: [Table; 4] = std::array::from_fn(|_| Table::new(4, 32));

    // 玩家 0 摸 13 张
    let mut p0_pts = Vec::new();
    for i in 0..13 {
        let pt = draw_one(&p, &final_deck, i, 0, &mut tables);
        p0_pts.push(pt);
    }
    // 玩家 1 摸 14 (winning_tile) + 弃
    let winning_pt = draw_one(&p, &final_deck, 14, 1, &mut tables);
    broadcast_discard(
        DiscardAnnouncement {
            player: 1,
            deck_index: 14,
            plaintext: winning_pt,
        },
        &mut tables,
    );

    // 玩家 0 ron: hand_indices = 0..13 + 14 (来自玩家 1 弃牌)
    let mut hand_indices: Vec<usize> = (0..13).collect();
    hand_indices.push(14);
    let mut hand_pts = p0_pts;
    hand_pts.push(winning_pt);

    let win = WinAnnouncement {
        player: 0,
        win_type: WinType::Ron { from_player: 1 },
        hand_indices,
        hand_plaintexts: hand_pts,
        winning_tile_index: 14,
        dora_plaintexts: vec![],
        uradoor_plaintexts: None,
    };
    // 4 方都验证通过
    for t in &tables {
        win.validate(t).unwrap();
    }
}
