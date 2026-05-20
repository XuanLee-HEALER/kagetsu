<script>
  // Side panel for ZeroTrust — peers · protocol · crypto · event log.
  import Eyebrow from '../../core/Eyebrow.svelte';
  import Hr from '../../core/Hr.svelte';
  import Row from '../../core/Row.svelte';
  import { PIG } from '../../core/pigments.js';
  import PeerRow from './PeerRow.svelte';
  import ProtoStep from './ProtoStep.svelte';
  import Evt from './Evt.svelte';

  export let game;
</script>

<!-- peers + crypto -->
<section>
  <Eyebrow style="margin-bottom: 10px;">Peers · 4 nodes</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 6px;">
    <PeerRow seat="東" name="你" status="ok" pk="12D3Koo...HJ9k" rtt="—" you />
    <PeerRow seat="南" name="下家" status="ok" pk="12D3Koo...x2Pq" rtt="14ms" />
    <PeerRow seat="西" name="对家" status="ok" pk="12D3Koo...mZ8r" rtt="22ms" />
    <PeerRow seat="北" name="上家" status="ok" pk="12D3Koo...vL1n" rtt="9ms" />
  </div>
</section>

<Hr />

<!-- protocol progress -->
<section>
  <Eyebrow style="margin-bottom: 10px;">Protocol · 协议进度</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 3px;">
    <ProtoStep n={0} name="JointKey · 联合公钥" status="done" />
    <ProtoStep n={1} name="Shuffle · Sako-Killian K=80" status="done" />
    <ProtoStep n={2} name="Draw · threshold decrypt" status="active" />
    <ProtoStep n={3} name="Reveal · dora indicator" status="done" />
    <ProtoStep n={4} name="Discard · broadcast" status="idle" />
    <ProtoStep n={5} name="Call · chi/pon/kan" status="idle" />
    <ProtoStep n={6} name="Concealed kan · 暗杠验证" status="idle" />
    <ProtoStep n={7} name="Win · ownership 证明" status="idle" />
  </div>
</section>

<Hr />

<!-- crypto stats -->
<section>
  <Eyebrow style="margin-bottom: 10px;">Crypto · 加密</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 8px;">
    <Row label="Curve" value="BLS12-381 G1" mono />
    <Row label="RNG" value="ChaCha20" mono />
    <Row label="ZK" value="Fiat-Shamir" mono />
    <Row label="DLEQ proofs" mono>
      <span style="color: {PIG.ljangkhu};">320 valid</span>
    </Row>
    <Row label="Shuffle K" value="80 · 完整" mono />
    <Row label="Verify time" value="~10s · once" mono />
  </div>
</section>

<Hr />

<!-- event log -->
<section style="flex: 1;">
  <Eyebrow style="margin-bottom: 10px;">Events · 协议日志</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 4px; font-family: var(--font-mono); font-size: 11px; line-height: 1.6;">
    <Evt t="08:14:22" who="net" msg="gossip · peers=4" tone="dim" />
    <Evt t="08:14:23" who="P0" msg="JointKey computed" tone="ok" />
    <Evt t="08:14:31" who="P1" msg="CnC shuffle ok · K=80" tone="ok" />
    <Evt t="08:14:31" who="P3" msg="Dora revealed · m5" />
    <Evt t="08:14:32" who="me" msg="DrawShare → 4 peers" tone="info" />
    <Evt t="08:14:32" who="P2" msg="Decrypt: 6 share · t=3" />
    <Evt t="08:14:32" who="me" msg="Hand[13] = p7" tone="ok" />
  </div>
</section>
