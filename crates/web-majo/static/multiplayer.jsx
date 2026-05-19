// Multiplayer screens — LAN lobby + Room waiting.

function LobbyScreen() {
  return (
    <div data-screen-label="Lobby · 大廳"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)", color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        display: "grid",
        gridTemplateRows: "72px 1fr 64px",
      }}>

      <header style={{
        background: "var(--bg-deepest)",
        borderBottom: "1px solid var(--border-subtle)",
        display: "flex", alignItems: "center", padding: "0 32px", gap: 16,
      }}>
        <DorjeMark size={20} />
        <div>
          <Eyebrow>Local network · 局域网</Eyebrow>
          <div style={{
            fontFamily: "var(--font-serif)", fontSize: "var(--text-lg)",
            letterSpacing: "var(--tracking-tight)", marginTop: 2,
          }}>大厅</div>
        </div>
        <div style={{ marginLeft: "auto", display: "flex", gap: 18, alignItems: "center" }}>
          <DiscoveryPill />
          <KeyBadge k="R" label="刷新" size="sm" />
          <KeyBadge k="C" tone="primary" label="创建房间" size="sm" />
        </div>
      </header>

      <main style={{
        display: "grid", gridTemplateColumns: "1.5fr 1fr",
        gap: 32, padding: "32px 48px", overflow: "hidden",
      }}>
        {/* room list */}
        <section style={{ display: "flex", flexDirection: "column", gap: 16, overflow: "hidden" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
            <Eyebrow>Rooms · 房间 (3)</Eyebrow>
            <span style={{ color: "var(--fg-tertiary)", fontSize: 12, fontFamily: "var(--font-mono)" }}>
              mDNS · gossipsub · 5 秒刷新
            </span>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 10, overflowY: "auto", paddingRight: 6 }}>
            <RoomRow active host="murakami@thinkpad" mode="Standard" players={2} max={4}
              addr="10.0.0.42:4321" rules="半庄 · 食断 · 赤宝" />
            <RoomRow host="tanaka@air" mode="ZeroTrust" players={3} max={4}
              addr="10.0.0.18:5142" rules="半庄 · 头跳" highlight="zt" />
            <RoomRow host="okuda@imac" mode="Standard" players={1} max={4}
              addr="10.0.0.91:4099" rules="东风 · 一发 · 里宝" />
          </div>

          {/* manual fallback */}
          <div style={{
            marginTop: 4,
            padding: "14px 18px",
            background: "var(--bg-surface0)",
            border: "1px solid var(--border-default)",
            borderRadius: "var(--radius-lg)",
          }}>
            <Eyebrow style={{ marginBottom: 10 }}>Manual · multiaddr</Eyebrow>
            <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
              <input readOnly value="/ip4/10.0.0.42/udp/4321/quic-v1/p2p/12D3KooWHJ..."
                style={{
                  flex: 1,
                  height: 36, padding: "0 12px",
                  background: "var(--bg-base)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-md)",
                  color: "var(--fg-primary)",
                  fontFamily: "var(--font-mono)", fontSize: 12,
                }} />
              <Button label="Join" />
            </div>
            <div style={{ color: "var(--fg-tertiary)", fontSize: 12, marginTop: 8 }}>
              mDNS 失效或跨子网时使用 · QUIC over TCP fallback
            </div>
          </div>
        </section>

        {/* side: details / status */}
        <aside style={{
          background: "var(--bg-deep)",
          border: "1px solid var(--border-subtle)",
          borderRadius: "var(--radius-lg)",
          padding: "20px 22px",
          display: "flex", flexDirection: "column", gap: 18,
        }}>
          <section>
            <Eyebrow style={{ marginBottom: 10 }}>Selected · 选中房间</Eyebrow>
            <div style={{
              fontFamily: "var(--font-serif)", fontSize: "var(--text-2xl)",
              letterSpacing: "var(--tracking-tight)", color: PIG.gser,
            }}>murakami</div>
            <div style={{ color: "var(--fg-tertiary)", fontSize: 13, marginTop: 4 }}>
              @thinkpad · 10.0.0.42
            </div>
          </section>

          <Hr />

          <section style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <Row label="Mode · 模式" value={<span style={{ color: PIG.gser }}>Standard</span>} />
            <Row label="Rules · 规则" value="半庄 · 食断 · 赤宝" />
            <Row label="Players · 玩家" value="2 / 4" mono />
            <Row label="AI fill · 补满" value="空座位补 AI" />
            <Row label="Timer · 计时" value="30 秒 / 步" mono />
            <Row label="Seed · 种子" value="random" mono />
          </section>

          <Hr />

          <section>
            <Eyebrow style={{ marginBottom: 10 }}>Players · 在房</Eyebrow>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <PlayerBadge seat="東" name="murakami" host />
              <PlayerBadge seat="南" name="tanaka" />
              <PlayerBadge seat="西" empty />
              <PlayerBadge seat="北" empty />
            </div>
          </section>

          <div style={{ marginTop: "auto", display: "flex", gap: 10 }}>
            <Button label="Spectate · 観戦" />
            <Button label="Join · 入る" primary kb="Enter" />
          </div>
        </aside>
      </main>

      <footer style={{
        borderTop: "1px solid var(--border-subtle)",
        background: "var(--bg-deepest)",
        display: "flex", alignItems: "center",
        justifyContent: "space-between",
        padding: "0 32px",
      }}>
        <div style={{ display: "flex", gap: 14 }}>
          <KeyBadge k="↑↓" label="选" size="sm" />
          <KeyBadge k="Enter" label="加入" size="sm" />
          <KeyBadge k="C" label="创建" size="sm" />
          <KeyBadge k="S" label="観戦" size="sm" />
        </div>
        <div style={{ display: "flex", gap: 14 }}>
          <KeyBadge k=":" label="命令" size="sm" />
          <KeyBadge k="Esc" label="返回" size="sm" />
        </div>
      </footer>
    </div>
  );
}

function DiscoveryPill() {
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 8,
      padding: "4px 12px",
      background: "var(--status-success-bg)",
      border: `1px solid ${PIG.ljangkhu}`,
      borderRadius: "var(--radius-pill)",
      fontSize: 12, color: PIG.ljangkhu, fontWeight: 500,
    }}>
      <span style={{
        width: 6, height: 6, borderRadius: "50%",
        background: PIG.ljangkhu,
        boxShadow: `0 0 8px ${PIG.ljangkhu}`,
      }} />
      mDNS active · 3 peers
    </div>
  );
}

function RoomRow({ active, host, mode, players, max, addr, rules, highlight }) {
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: "auto 1fr auto auto auto",
      gap: 16, alignItems: "center",
      padding: "14px 18px",
      background: active ? "var(--accent-soft)" : "var(--bg-surface0)",
      border: `1px solid ${active ? "var(--border-focus)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-lg)",
    }}>
      <div style={{
        width: 36, height: 36, borderRadius: "var(--radius-md)",
        background: highlight === "zt" ? "rgba(91,138,184,0.18)" : "var(--bg-surface1)",
        border: `1px solid ${highlight === "zt" ? PIG.ngonpo : "var(--border-default)"}`,
        display: "flex", alignItems: "center", justifyContent: "center",
        color: highlight === "zt" ? PIG.ngonpo : "var(--fg-secondary)",
        fontFamily: "var(--font-mono)", fontSize: 10, fontWeight: 600,
      }}>
        {mode === "ZeroTrust" ? "ZT" : "P2P"}
      </div>
      <div>
        <div style={{
          fontFamily: "var(--font-serif)", fontSize: "var(--text-lg)",
          color: active ? PIG.gser : "var(--fg-primary)",
          letterSpacing: "var(--tracking-tight)", lineHeight: 1.2,
        }}>{host}</div>
        <div style={{ color: "var(--fg-tertiary)", fontSize: 12, fontFamily: "var(--font-mono)", marginTop: 2 }}>{addr}</div>
      </div>
      <div style={{ color: "var(--fg-secondary)", fontSize: 13 }}>{rules}</div>
      <div style={{
        color: highlight === "zt" ? PIG.ngonpo : mode === "Standard" ? "var(--fg-secondary)" : PIG.gser,
        fontSize: 13, fontWeight: 500,
      }}>{mode}</div>
      <div style={{
        fontFamily: "var(--font-mono)", fontSize: "var(--text-md)",
        color: players === max ? "var(--fg-disabled)" : "var(--fg-primary)",
      }}>
        {players}/{max}
      </div>
    </div>
  );
}

function PlayerBadge({ seat, name, host, empty }) {
  return (
    <div style={{
      display: "grid", gridTemplateColumns: "auto 1fr auto",
      alignItems: "center", gap: 12,
      padding: "8px 12px",
      background: empty ? "transparent" : "var(--bg-surface0)",
      border: `1px solid ${empty ? "var(--border-subtle)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-md)",
      borderStyle: empty ? "dashed" : "solid",
    }}>
      <span style={{
        fontFamily: "var(--font-serif)", fontSize: "var(--text-md)",
        color: empty ? "var(--fg-disabled)" : "var(--fg-primary)",
      }}>{seat}</span>
      <span style={{
        color: empty ? "var(--fg-disabled)" : "var(--fg-primary)",
        fontSize: 13, fontStyle: empty ? "italic" : "normal",
      }}>{empty ? "Empty · 等待" : name}</span>
      {host ? <span style={{
        fontSize: 10, color: PIG.gser, fontWeight: 600,
        padding: "1px 6px", border: `1px solid ${PIG.gser}`, borderRadius: 2,
      }}>HOST</span> : null}
    </div>
  );
}

// ── Room waiting screen ───────────────────────────────────────────
function RoomScreen() {
  return (
    <div data-screen-label="Room · 房間"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)", color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        display: "flex", flexDirection: "column",
      }}>
      <header style={{
        height: 72,
        background: "var(--bg-deepest)",
        borderBottom: "1px solid var(--border-subtle)",
        display: "flex", alignItems: "center", padding: "0 32px", gap: 16,
      }}>
        <DorjeMark size={20} />
        <div>
          <Eyebrow>Room · 房间</Eyebrow>
          <div style={{ fontFamily: "var(--font-serif)", fontSize: "var(--text-lg)", marginTop: 2 }}>
            murakami@thinkpad
          </div>
        </div>
        <div style={{ marginLeft: "auto", display: "flex", gap: 16, alignItems: "center" }}>
          <DiscoveryPill />
          <KeyBadge k="L" label="离开" size="sm" />
        </div>
      </header>

      <main style={{
        flex: 1, display: "grid",
        gridTemplateColumns: "1fr 360px",
        gap: 0, overflow: "hidden",
      }}>
        <section style={{ padding: "48px 64px", display: "flex", flexDirection: "column", gap: 32 }}>
          <div>
            <Eyebrow>Seats · 座位</Eyebrow>
            <h2 style={{
              font: "var(--t-h3)", letterSpacing: "var(--tracking-tight)",
              marginTop: 6,
            }}>等待玩家就坐</h2>
            <p style={{ color: "var(--fg-secondary)", maxWidth: 520, marginTop: 8 }}>
              当前 3/4 玩家。房主可点击空座位填入 AI，或继续等待。
            </p>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
            <SeatCard seat="東" name="murakami" host you />
            <SeatCard seat="南" name="tanaka" />
            <SeatCard seat="西" name="okuda" />
            <SeatCard seat="北" empty />
          </div>
        </section>

        <aside style={{
          background: "var(--bg-deep)",
          borderLeft: "1px solid var(--border-subtle)",
          padding: "32px 24px", overflowY: "auto",
          display: "flex", flexDirection: "column", gap: 22,
        }}>
          <section>
            <Eyebrow style={{ marginBottom: 10 }}>Rules · 规则</Eyebrow>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <Row label="Format" value="半庄战" />
              <Row label="Multi-ron" value="头跳" />
              <Row label="食断 · 赤宝 · 一发" value={<span style={{ color: PIG.ljangkhu }}>开</span>} />
              <Row label="里宝牌" value={<span style={{ color: PIG.ljangkhu }}>开</span>} />
              <Row label="Uma" value="20-10-(-10)-(-20)" mono />
              <Row label="Start · Target" value="25,000 · 30,000" mono />
              <Row label="Timer" value="30 秒" mono />
            </div>
          </section>

          <Hr />

          <section>
            <Eyebrow style={{ marginBottom: 10 }}>Chat · 聊天</Eyebrow>
            <div style={{
              fontSize: 12, color: "var(--fg-tertiary)",
              display: "flex", flexDirection: "column", gap: 6,
              fontFamily: "var(--font-mono)",
            }}>
              <div><span style={{ color: PIG.gser }}>murakami</span> joined</div>
              <div><span style={{ color: "var(--fg-secondary)" }}>tanaka</span> joined</div>
              <div><span style={{ color: "var(--fg-secondary)" }}>okuda</span> joined</div>
              <div><span style={{ color: PIG.ljangkhu }}>system</span> waiting 1 more...</div>
            </div>
          </section>

          <div style={{ marginTop: "auto", display: "flex", gap: 10, flexDirection: "column" }}>
            <Button label="Fill empty with AI · 补 AI" />
            <Button label="Start · 開始" primary kb="Enter" />
          </div>
        </aside>
      </main>
    </div>
  );
}

function SeatCard({ seat, name, host, you, empty }) {
  return (
    <div style={{
      padding: 24,
      background: you ? "var(--accent-soft)" : empty ? "transparent" : "var(--bg-surface0)",
      border: `1px ${empty ? "dashed" : "solid"} ${you ? "var(--border-focus)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-lg)",
      display: "flex", alignItems: "center", gap: 18,
      minHeight: 100,
    }}>
      <div style={{
        width: 56, height: 56, borderRadius: "var(--radius-md)",
        background: empty ? "transparent" : "var(--bg-base)",
        border: `1px ${empty ? "dashed" : "solid"} ${you ? PIG.gser : "var(--border-default)"}`,
        display: "flex", alignItems: "center", justifyContent: "center",
        fontFamily: "var(--font-serif)", fontSize: "var(--text-2xl)",
        color: you ? PIG.gser : empty ? "var(--fg-disabled)" : "var(--fg-primary)",
      }}>{seat}</div>
      <div style={{ flex: 1 }}>
        <div style={{
          fontSize: "var(--text-md)",
          color: empty ? "var(--fg-disabled)" : "var(--fg-primary)",
          fontStyle: empty ? "italic" : "normal",
        }}>{empty ? "Empty · 等待加入" : name}</div>
        <div style={{ color: "var(--fg-tertiary)", fontSize: 12, marginTop: 4, display: "flex", gap: 8 }}>
          {host ? <span style={{ color: PIG.gser, fontWeight: 600 }}>HOST</span> : null}
          {you ? <span>YOU</span> : null}
          {empty ? <span>or fill AI</span> : <span style={{ color: PIG.ljangkhu }}>● ready</span>}
        </div>
      </div>
    </div>
  );
}

window.LobbyScreen = LobbyScreen;
window.RoomScreen = RoomScreen;
