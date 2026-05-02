//! 一次性诊断: 起一个 host swarm 连 claw, 看 reservation 协议是否触发.
//! 跑: RUST_LOG=info,libp2p_relay=debug,libp2p_swarm=info \
//!     cargo test --release --test probe_claw_relay -- --ignored --nocapture
//!
//! 关键: listen_on(/p2p-circuit) 必须等 identify received from relay 之后,
//! 否则 libp2p 立即 ListenerClosed (libp2p 0.56 行为).

use std::time::Duration;

use futures_util::StreamExt;
use libp2p::{Multiaddr, identify, multiaddr::Protocol, swarm::SwarmEvent};
use tui_majo::net::p2p::behaviour::P2pBehaviourEvent;
use tui_majo::net::p2p::swarm::{build_swarm, new_keypair};

const CLAW_QUIC: &str =
    "/ip4/47.84.49.170/udp/4001/quic-v1/p2p/12D3KooWDCnWrarKrN7aeVarDn2NpJwz7XdPWDXvgnHEvmARubYP";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn probe_relay_reservation() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "info,libp2p_relay=debug,libp2p_swarm=info,libp2p_dcutr=debug",
                )
            }),
        )
        .try_init();

    let kp = new_keypair();
    let mut swarm = build_swarm(kp, "probe/0".into()).expect("build swarm");

    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .expect("listen quic");

    let claw: Multiaddr = CLAW_QUIC.parse().unwrap();
    let claw_peer = claw
        .iter()
        .find_map(|p| match p {
            Protocol::P2p(id) => Some(id),
            _ => None,
        })
        .expect("claw multiaddr 含 /p2p/<peer-id>");

    swarm.dial(claw.clone()).expect("dial claw");
    println!("[probe] dialed {claw}, waiting for identify before listen_on(circuit)...");

    let mut listened = false;
    let _ = tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            let event = swarm.select_next_some().await;
            // 关键: 等 identify::Received from claw, 才 listen_on(/p2p-circuit)
            if !listened
                && let SwarmEvent::Behaviour(P2pBehaviourEvent::Identify(
                    identify::Event::Received { peer_id, .. },
                )) = &event
                && *peer_id == claw_peer
            {
                let circuit = claw.clone().with(Protocol::P2pCircuit);
                match swarm.listen_on(circuit.clone()) {
                    Ok(_) => {
                        println!("[probe] >>> listen_on circuit OK (identify received): {circuit}");
                        listened = true;
                    }
                    Err(e) => println!("[probe] !!! listen circuit ERR: {e}"),
                }
            }
            println!("[event] {event:?}");
        }
    })
    .await;
    println!("[probe] 35s 到, 退出");
}
