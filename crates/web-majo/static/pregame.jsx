// Main menu / launcher + Pre-game config screens.

function MenuScreen() {
  return (
    <div data-screen-label="Menu · launcher"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)", color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        display: "grid",
        gridTemplateColumns: "1fr 480px 1fr",
        position: "relative", overflow: "hidden",
      }}>

      {/* left column — display title + lineage */}
      <div style={{
        display: "flex", flexDirection: "column", justifyContent: "center",
        padding: "80px 64px",
      }}>
        <Eyebrow>Welcome · 帰宅</Eyebrow>
        <h1 style={{
          font: "var(--t-h1)",
          letterSpacing: "var(--tracking-tight)",
          marginTop: 14, color: "var(--fg-primary)",
        }}>
          tui-majo
        </h1>
        <div style={{
          fontFamily: "var(--font-serif)", fontStyle: "italic",
          fontSize: "var(--text-lg)",
          color: "var(--fg-secondary)", marginTop: 8,
          maxWidth: 400,
        }}>
          A single dark surface for one quiet game of riichi.
        </div>

        <div style={{
          marginTop: 64,
          fontFamily: "var(--font-mono)", fontSize: 12,
          color: "var(--fg-tertiary)", lineHeight: 1.8,
        }}>
          <div>v2.1.0 · WRC 2022 rules</div>
          <div>local · libp2p · zerotrust</div>
          <div>403 unit tests · 99 replay verifications</div>
        </div>

        <div style={{ marginTop: 48, display: "flex", gap: 12, alignItems: "center" }}>
          <ThreeStripes />
          <span style={{
            font: "var(--t-eyebrow)",
            letterSpacing: "var(--tracking-widest)",
            textTransform: "uppercase",
            color: "var(--fg-tertiary)",
          }}>
            Sakya · 萨迦
          </span>
        </div>
      </div>

      {/* center column — menu */}
      <div style={{
        display: "flex", flexDirection: "column", justifyContent: "center",
        padding: "80px 0",
      }}>
        <MenuItem k="1" title="Single player" subtitle="单人对局 · AI 补满空座" hover />
        <MenuItem k="2" title="LAN game" subtitle="局域网 · libp2p mDNS 发现" />
        <MenuItem k="3" title="ZeroTrust" subtitle="P2P mental poker · 无需信任房主" />
        <MenuItem k="4" title="Replay" subtitle="重放天凤 mjlog" />
        <MenuItem k="5" title="Settings" subtitle="规则 · 主题 · 计时" />
        <div style={{ height: 24 }} />
        <MenuItem k="Q" title="Quit" subtitle="退出 · ご機嫌よう" muted />
      </div>

      {/* right column — empty, just to provide breathing room */}
      <div style={{
        position: "relative",
        display: "flex", flexDirection: "column", justifyContent: "flex-end",
        padding: "0 64px 64px",
      }}>
        <div style={{
          fontFamily: "var(--font-serif)", fontSize: "var(--text-base)",
          fontStyle: "italic", color: "var(--fg-tertiary)",
          maxWidth: 280, textAlign: "right",
          lineHeight: 1.7,
        }}>
          "Like a black thangka — gold lines of enlightened form emerging from the void."
        </div>
      </div>

      {/* bottom hint */}
      <div style={{
        position: "absolute", bottom: 24, left: "50%",
        transform: "translateX(-50%)",
        display: "flex", gap: 18, alignItems: "center",
      }}>
        <KeyBadge k="↑↓" size="sm" label="navigate" />
        <KeyBadge k="Enter" tone="primary" size="sm" label="select" />
        <KeyBadge k="1-5" size="sm" label="direct" />
      </div>
    </div>
  );
}

function MenuItem({ k, title, subtitle, hover, muted }) {
  return (
    <div style={{
      padding: "18px 32px",
      borderLeft: hover ? `3px solid ${PIG.gser}` : "3px solid transparent",
      background: hover ? "var(--accent-soft)" : "transparent",
      display: "grid", gridTemplateColumns: "auto 1fr",
      alignItems: "center", gap: 20,
      transition: "background 200ms var(--ease-out)",
      cursor: "default",
      opacity: muted ? 0.5 : 1,
    }}>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: "var(--text-lg)",
        color: hover ? PIG.gser : "var(--fg-tertiary)",
        fontWeight: 500,
        width: 28,
      }}>{k}</span>
      <div>
        <div style={{
          fontFamily: "var(--font-serif)", fontWeight: 400,
          fontSize: "var(--text-xl)", color: "var(--fg-primary)",
          letterSpacing: "var(--tracking-tight)",
          lineHeight: 1.2,
        }}>{title}</div>
        <div style={{
          fontSize: "var(--text-sm)",
          color: "var(--fg-tertiary)", marginTop: 4,
        }}>{subtitle}</div>
      </div>
    </div>
  );
}

function ThreeStripes() {
  return (
    <div style={{ display: "flex", height: 16, width: 40 }}>
      <div style={{ flex: 1, background: PIG.stripeRed }} />
      <div style={{ flex: 1, background: PIG.stripeWhite }} />
      <div style={{ flex: 1, background: PIG.stripeBlue }} />
    </div>
  );
}

// ── Config screen ───────────────────────────────────────────────────
function ConfigScreen() {
  return (
    <div data-screen-label="Config · 設定"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)", color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        display: "grid",
        gridTemplateColumns: "260px 1fr",
        gridTemplateRows: "72px 1fr 72px",
        gridTemplateAreas: `
          "head head"
          "nav  body"
          "foot foot"
        `,
      }}>

      <header style={{
        gridArea: "head",
        background: "var(--bg-deepest)",
        borderBottom: "1px solid var(--border-subtle)",
        display: "flex", alignItems: "center",
        padding: "0 32px", gap: 16,
      }}>
        <DorjeMark size={20} />
        <div>
          <Eyebrow>Pre-game · 设定</Eyebrow>
          <div style={{
            fontFamily: "var(--font-serif)",
            fontSize: "var(--text-lg)",
            letterSpacing: "var(--tracking-tight)",
            marginTop: 2,
          }}>对局配置</div>
        </div>
        <div style={{ marginLeft: "auto", display: "flex", gap: 10, alignItems: "center" }}>
          <KeyBadge k="Esc" label="返回" size="sm" />
        </div>
      </header>

      <nav style={{
        gridArea: "nav",
        background: "var(--bg-deep)",
        borderRight: "1px solid var(--border-subtle)",
        padding: "20px 0",
      }}>
        <NavItem n="01" label="赛制 · Format" active />
        <NavItem n="02" label="役种 · Yaku" />
        <NavItem n="03" label="点棒 · Points" />
        <NavItem n="04" label="Uma" />
        <NavItem n="05" label="计时 · Timer" />
        <NavItem n="06" label="主题 · Theme" />
        <NavItem n="07" label="种子 · Seed" />
      </nav>

      <main style={{
        gridArea: "body",
        overflowY: "auto",
        padding: "32px 64px",
      }}>
        <div style={{ maxWidth: 720 }}>
          <Eyebrow>Section 01</Eyebrow>
          <h2 style={{
            font: "var(--t-h3)",
            letterSpacing: "var(--tracking-tight)",
            marginTop: 8, marginBottom: 6,
          }}>赛制 · Format</h2>
          <p style={{ color: "var(--fg-secondary)", maxWidth: 540, marginBottom: 28 }}>
            选择庄数与终局规则。半庄战为标准 8 局 (东南各 4)，东风战仅 4 局。
          </p>

          <ConfigGroup label="Game length · 庄数">
            <Radio name="length" label="半庄战 · Hanchan" sub="东 + 南 · 8 局" active />
            <Radio name="length" label="东风战 · Tonpuusen" sub="仅东风 · 4 局" />
          </ConfigGroup>

          <ConfigGroup label="Multi-ron · 多家荣和">
            <Radio name="ron" label="头跳 · Atamahane" sub="按上家顺序仅头家获胜" active />
            <Radio name="ron" label="双家荣 · Double ron" sub="允许两家同时荣和" />
            <Radio name="ron" label="三家荣 · Triple ron" sub="三家荣和 = 流局 → 三家和" />
          </ConfigGroup>

          <ConfigGroup label="Special rules · 特殊規定">
            <Toggle label="食断 · Kuitan" sub="开放副露后断幺九成立" on />
            <Toggle label="赤宝牌 · Red five" sub="三色五各一张赤" on />
            <Toggle label="一发 · Ippatsu" sub="立直后一巡内和牌" on />
            <Toggle label="里宝牌 · Uradora" sub="立直和牌时翻里宝指示" on />
            <Toggle label="数役満 · Counted yakuman" sub="13+ 番按役満计算" />
            <Toggle label="W役満 · Double yakuman" sub="允许双重役満" />
          </ConfigGroup>

          <ConfigGroup label="End condition · 終局">
            <Toggle label="西入 · Westward extension" sub="到南四亲未达终局点 → 继续" />
            <Toggle label="击飞 · Tobi" sub="任一家点数 < 0 立即終局" on />
          </ConfigGroup>
        </div>
      </main>

      <footer style={{
        gridArea: "foot",
        borderTop: "1px solid var(--border-subtle)",
        background: "var(--bg-deepest)",
        display: "flex", alignItems: "center",
        justifyContent: "space-between",
        padding: "0 32px",
      }}>
        <div style={{ display: "flex", gap: 14 }}>
          <KeyBadge k="Tab" label="下一节" size="sm" />
          <KeyBadge k="←→" label="切换选项" size="sm" />
        </div>
        <div style={{ display: "flex", gap: 12 }}>
          <Button label="重置默认" />
          <Button label="开始对局" primary kb="Enter" />
        </div>
      </footer>
    </div>
  );
}

function NavItem({ n, label, active }) {
  return (
    <div style={{
      display: "grid", gridTemplateColumns: "auto 1fr",
      alignItems: "center", gap: 12,
      padding: "10px 20px",
      borderLeft: active ? `2px solid ${PIG.gser}` : "2px solid transparent",
      background: active ? "var(--accent-soft)" : "transparent",
      color: active ? PIG.gser : "var(--fg-secondary)",
      fontSize: 13,
    }}>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 11,
        color: active ? PIG.gser : "var(--fg-disabled)",
      }}>{n}</span>
      <span style={{ fontWeight: active ? 600 : 400 }}>{label}</span>
    </div>
  );
}

function ConfigGroup({ label, children }) {
  return (
    <div style={{ marginBottom: 28 }}>
      <Eyebrow style={{ marginBottom: 10 }}>{label}</Eyebrow>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {children}
      </div>
    </div>
  );
}

function Radio({ name, label, sub, active }) {
  return (
    <label style={{
      display: "grid", gridTemplateColumns: "auto 1fr",
      alignItems: "center", gap: 12,
      padding: "10px 14px",
      background: active ? "var(--accent-soft)" : "var(--bg-surface0)",
      border: `1px solid ${active ? "var(--border-focus)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-md)",
      cursor: "pointer",
    }}>
      <span style={{
        width: 16, height: 16, borderRadius: "50%",
        border: `1.5px solid ${active ? PIG.gser : "var(--fg-disabled)"}`,
        display: "flex", alignItems: "center", justifyContent: "center",
      }}>
        {active ? (
          <span style={{ width: 7, height: 7, borderRadius: "50%", background: PIG.gser }} />
        ) : null}
      </span>
      <div>
        <div style={{ color: "var(--fg-primary)", fontSize: 14, fontWeight: active ? 500 : 400 }}>{label}</div>
        {sub ? <div style={{ color: "var(--fg-tertiary)", fontSize: 12, marginTop: 2 }}>{sub}</div> : null}
      </div>
    </label>
  );
}

function Toggle({ label, sub, on }) {
  return (
    <label style={{
      display: "grid", gridTemplateColumns: "1fr auto",
      alignItems: "center", gap: 12,
      padding: "10px 14px",
      background: "var(--bg-surface0)",
      border: "1px solid var(--border-default)",
      borderRadius: "var(--radius-md)",
      cursor: "pointer",
    }}>
      <div>
        <div style={{ color: "var(--fg-primary)", fontSize: 14, fontWeight: on ? 500 : 400 }}>{label}</div>
        {sub ? <div style={{ color: "var(--fg-tertiary)", fontSize: 12, marginTop: 2 }}>{sub}</div> : null}
      </div>
      <span style={{
        width: 36, height: 20, borderRadius: 10,
        background: on ? PIG.gser : "var(--bg-surface2)",
        position: "relative",
        transition: "background 200ms",
      }}>
        <span style={{
          position: "absolute",
          top: 2, left: on ? 18 : 2,
          width: 16, height: 16, borderRadius: "50%",
          background: on ? "var(--fg-on-accent)" : "var(--fg-primary)",
          transition: "left 200ms",
        }} />
      </span>
    </label>
  );
}

function Button({ label, primary, kb }) {
  return (
    <button style={{
      height: 40, padding: "0 20px",
      background: primary ? PIG.gser : "var(--bg-surface0)",
      color: primary ? "var(--fg-on-accent)" : "var(--fg-primary)",
      border: `1px solid ${primary ? PIG.gser : "var(--border-default)"}`,
      borderRadius: "var(--radius-md)",
      fontSize: 14, fontWeight: 500,
      fontFamily: "var(--font-sans)",
      display: "flex", alignItems: "center", gap: 10,
      cursor: "pointer",
    }}>
      {label}
      {kb ? (
        <span style={{
          fontFamily: "var(--font-mono)", fontSize: 11,
          opacity: 0.7,
          padding: "1px 6px",
          background: primary ? "rgba(12,12,20,0.18)" : "var(--bg-surface1)",
          borderRadius: "var(--radius-xs)",
        }}>{kb}</span>
      ) : null}
    </button>
  );
}

window.MenuScreen = MenuScreen;
window.ConfigScreen = ConfigScreen;
