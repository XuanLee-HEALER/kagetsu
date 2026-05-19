// ZeroTrust game screen — variant of GameScreen showing mental poker protocol state.
// Different keys, protocol indicator, joint shuffle status.

function ZeroTrustGameScreen() {
  const g = GAME;
  const me = g.players[0], shimo = g.players[1], toi = g.players[2], kami = g.players[3];

  return (
    <div data-screen-label="ZeroTrust · game"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)",
        color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        display: "grid",
        gridTemplateColumns: "1fr 340px",
        gridTemplateRows: "56px 1fr 156px",
        gridTemplateAreas: `"top top" "table side" "hand side"`,
        position: "relative",
      }}>

      {/* top status bar — adds protocol pill */}
      <header style={{
        gridArea: "top",
        background: "var(--bg-deepest)",
        borderBottom: "1px solid var(--border-subtle)",
        display: "grid", gridTemplateColumns: "1fr auto 1fr",
        alignItems: "center", padding: "0 32px",
      }}>
        <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <DorjeMark size={20} />
            <span style={{
              font: "var(--t-eyebrow)", letterSpacing: "var(--tracking-widest)",
              textTransform: "uppercase", color: "var(--fg-tertiary)",
            }}>tui-majo</span>
          </div>
          <ZTBadge />
          <ProtocolPill step={2} label="Draw · 摸牌" />
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 14, justifyContent: "center" }}>
          <span style={{
            font: "var(--t-eyebrow)", letterSpacing: "var(--tracking-widest)",
            textTransform: "uppercase", color: "var(--fg-tertiary)",
          }}>東 2 局 · 1 本場 · 巡 8</span>
        </div>

        <div style={{ display: "flex", gap: 16, alignItems: "center", justifyContent: "flex-end" }}>
          <span style={{ color: "var(--fg-tertiary)", fontSize: 12, fontFamily: "var(--font-mono)" }}>
            Sako-Killian · K=80
          </span>
          <span style={{ width: 1, height: 24, background: "var(--border-default)" }} />
          <KeyBadge k="P" label="Protocol" size="sm" />
          <KeyBadge k="Esc" label="Leave" size="sm" />
        </div>
      </header>

      <section style={{ gridArea: "table", position: "relative", overflow: "hidden" }}>
        <Table players={g.players} dora={g.dora[0]} wall={g.wall} junme={g.junme} round={g.round} />

        {/* protocol overlay tint — barely visible blue, signals ZT */}
        <div style={{
          position: "absolute", inset: 0,
          background: `radial-gradient(circle at center, rgba(91,138,184,0.04) 0%, transparent 60%)`,
          pointerEvents: "none",
        }} />
      </section>

      <section style={{
        gridArea: "hand",
        background: "var(--bg-deep)",
        borderTop: "1px solid var(--border-subtle)",
        padding: "20px 32px 16px",
        display: "flex", flexDirection: "column", gap: 12,
      }}>
        <ZTHandStrip me={me} />
      </section>

      <aside style={{
        gridArea: "side",
        background: "var(--bg-deep)",
        borderLeft: "1px solid var(--border-subtle)",
        padding: "20px 20px 16px",
        overflowY: "auto",
        display: "flex", flexDirection: "column", gap: 20,
      }}>
        <ZTSidePanel game={g} />
      </aside>
    </div>
  );
}

function ZTBadge() {
  return (
    <div style={{
      display: "inline-flex", alignItems: "center", gap: 8,
      padding: "4px 10px 4px 8px",
      background: `linear-gradient(90deg, rgba(91,138,184,0.16) 0%, rgba(46,72,120,0.12) 100%)`,
      border: `1px solid ${PIG.ngonpo}`,
      borderRadius: "var(--radius-pill)",
      color: PIG.ngonpo, fontSize: 11, fontWeight: 600,
      letterSpacing: "var(--tracking-wide)",
      textTransform: "uppercase",
    }}>
      {/* simplified shield/lock icon */}
      <svg width="11" height="13" viewBox="0 0 11 13" fill="none" stroke="currentColor" strokeWidth="1.4">
        <path d="M5.5 1 L9.5 2.5 V6.5 C9.5 9 7.5 11 5.5 12 C3.5 11 1.5 9 1.5 6.5 V2.5 Z" />
      </svg>
      ZeroTrust
    </div>
  );
}

function ProtocolPill({ step, label }) {
  const total = 8;
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 10,
      padding: "5px 12px",
      background: "var(--bg-surface0)",
      border: "1px solid var(--border-default)",
      borderRadius: "var(--radius-pill)",
    }}>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 11,
        color: PIG.gser, fontWeight: 600,
      }}>P{step}/{total - 1}</span>
      <span style={{ color: "var(--fg-secondary)", fontSize: 12 }}>{label}</span>
      <div style={{ display: "flex", gap: 2 }}>
        {Array(8).fill(0).map((_, i) => (
          <span key={i} style={{
            width: 4, height: 4, borderRadius: "50%",
            background: i <= step ? PIG.gser : "var(--bg-surface2)",
          }} />
        ))}
      </div>
    </div>
  );
}

function ZTHandStrip({ me }) {
  return (
    <>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 20, justifyContent: "center" }}>
        <TileRow tiles={me.hand} size="md" gap={3} selected={6} />
        <div style={{ width: 1, height: 52, background: "var(--border-default)" }} />
        <Tile t={me.draw} size="md" state="draw" />
      </div>

      <div style={{
        display: "flex", alignItems: "center", justifyContent: "space-between",
        gap: 20, marginTop: 6,
      }}>
        <div style={{ display: "flex", alignItems: "center", gap: 16, flexWrap: "wrap" }}>
          <KeyBadge k="D" tone="primary" label="摸下一张" />
          <KeyBadge k="Enter" label="弃 cursor 牌" />
          <KeyBadge k="←→" label="移动 cursor" />
          <span style={{ width: 1, height: 18, background: "var(--border-default)" }} />
          <KeyBadge k="R" label="揭示宝牌" />
          <KeyBadge k="C" label="吃" /><KeyBadge k="P" label="碰" />
          <KeyBadge k="K" label="明杠" /><KeyBadge k="A" label="暗杠" />
          <KeyBadge k="X" label="加杠" />
          <span style={{ width: 1, height: 18, background: "var(--border-default)" }} />
          <KeyBadge k="I" tone="danger" label="立直" />
          <KeyBadge k="T" tone="ok" label="自摸" />
          <KeyBadge k="N" tone="ok" label="荣和" />
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{
            font: "var(--t-eyebrow)", letterSpacing: "var(--tracking-widest)",
            textTransform: "uppercase",
            color: PIG.ngonpo,
            background: "rgba(91,138,184,0.16)",
            padding: "4px 12px",
            borderRadius: "var(--radius-sm)",
            fontWeight: 600, fontSize: 11,
          }}>P2P</span>
          <KeyBadge k="L" label="Leave" size="sm" />
        </div>
      </div>
    </>
  );
}

function ZTSidePanel({ game }) {
  return (
    <>
      {/* peers + crypto */}
      <section>
        <Eyebrow style={{ marginBottom: 10 }}>Peers · 4 nodes</Eyebrow>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <PeerRow seat="東" name="你" status="ok" pk="12D3Koo...HJ9k" rtt="—" you />
          <PeerRow seat="南" name="下家" status="ok" pk="12D3Koo...x2Pq" rtt="14ms" />
          <PeerRow seat="西" name="对家" status="ok" pk="12D3Koo...mZ8r" rtt="22ms" />
          <PeerRow seat="北" name="上家" status="ok" pk="12D3Koo...vL1n" rtt="9ms" />
        </div>
      </section>

      <Hr />

      {/* protocol progress */}
      <section>
        <Eyebrow style={{ marginBottom: 10 }}>Protocol · 协议进度</Eyebrow>
        <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
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

      {/* crypto stats */}
      <section>
        <Eyebrow style={{ marginBottom: 10 }}>Crypto · 加密</Eyebrow>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <Row label="Curve" value="BLS12-381 G1" mono />
          <Row label="RNG" value="ChaCha20" mono />
          <Row label="ZK" value="Fiat-Shamir" mono />
          <Row label="DLEQ proofs" value={<span style={{ color: PIG.ljangkhu }}>320 valid</span>} mono />
          <Row label="Shuffle K" value="80 · 完整" mono />
          <Row label="Verify time" value="~10s · once" mono />
        </div>
      </section>

      <Hr />

      {/* event log */}
      <section style={{ flex: 1 }}>
        <Eyebrow style={{ marginBottom: 10 }}>Events · 协议日志</Eyebrow>
        <div style={{
          display: "flex", flexDirection: "column", gap: 4,
          fontFamily: "var(--font-mono)", fontSize: 11,
          lineHeight: 1.6,
        }}>
          <Evt t="08:14:22" who="net" msg="gossip · peers=4" tone="dim" />
          <Evt t="08:14:23" who="P0" msg="JointKey computed" tone="ok" />
          <Evt t="08:14:31" who="P1" msg="CnC shuffle ok · K=80" tone="ok" />
          <Evt t="08:14:31" who="P3" msg="Dora revealed · m5" />
          <Evt t="08:14:32" who="me" msg="DrawShare → 4 peers" tone="info" />
          <Evt t="08:14:32" who="P2" msg="Decrypt: 6 share · t=3" />
          <Evt t="08:14:32" who="me" msg="Hand[13] = p7" tone="ok" />
        </div>
      </section>
    </>
  );
}

function PeerRow({ seat, name, status, pk, rtt, you }) {
  const ok = status === "ok";
  return (
    <div style={{
      display: "grid", gridTemplateColumns: "auto auto 1fr auto",
      alignItems: "center", gap: 10,
      padding: "6px 10px",
      background: you ? "var(--accent-soft)" : "transparent",
      border: `1px solid ${you ? "var(--border-focus)" : "var(--border-subtle)"}`,
      borderRadius: "var(--radius-md)",
    }}>
      <span style={{
        width: 7, height: 7, borderRadius: "50%",
        background: ok ? PIG.ljangkhu : PIG.mtshal,
        boxShadow: ok ? `0 0 6px ${PIG.ljangkhu}` : null,
      }} />
      <span style={{
        fontFamily: "var(--font-serif)", fontSize: "var(--text-md)",
        color: you ? PIG.gser : "var(--fg-primary)",
      }}>{seat}</span>
      <div>
        <div style={{ fontSize: 12, color: "var(--fg-primary)" }}>{name}</div>
        <div style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--fg-tertiary)" }}>{pk}</div>
      </div>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 11,
        color: "var(--fg-tertiary)",
      }}>{rtt}</span>
    </div>
  );
}

function ProtoStep({ n, name, status }) {
  const color = status === "done" ? PIG.ljangkhu : status === "active" ? PIG.gser : "var(--fg-disabled)";
  const glyph = status === "done" ? "●" : status === "active" ? "◆" : "○";
  return (
    <div style={{
      display: "grid", gridTemplateColumns: "auto auto 1fr",
      alignItems: "center", gap: 8,
      padding: "4px 6px",
      background: status === "active" ? "var(--accent-soft)" : "transparent",
      borderRadius: "var(--radius-sm)",
    }}>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 10,
        color: "var(--fg-disabled)",
      }}>P{n}</span>
      <span style={{ color, fontSize: 10 }}>{glyph}</span>
      <span style={{
        fontSize: 12,
        color: status === "idle" ? "var(--fg-disabled)" : status === "active" ? PIG.gser : "var(--fg-secondary)",
        fontWeight: status === "active" ? 500 : 400,
      }}>{name}</span>
    </div>
  );
}

function Evt({ t, who, msg, tone }) {
  const color = tone === "ok" ? PIG.ljangkhu : tone === "info" ? PIG.ngonpo : tone === "dim" ? "var(--fg-disabled)" : "var(--fg-secondary)";
  return (
    <div style={{ display: "grid", gridTemplateColumns: "auto auto 1fr", gap: 8 }}>
      <span style={{ color: "var(--fg-disabled)" }}>{t}</span>
      <span style={{ color: PIG.dri_med, fontWeight: 600, minWidth: 22 }}>{who}</span>
      <span style={{ color }}>{msg}</span>
    </div>
  );
}

window.ZeroTrustGameScreen = ZeroTrustGameScreen;
