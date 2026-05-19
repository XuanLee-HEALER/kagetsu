// Result screens — single hand result + match end standings.

function HandResultScreen() {
  return (
    <div data-screen-label="Hand result · 和了"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)", color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        position: "relative",
        overflow: "hidden",
      }}>

      {/* protective gradient at bottom — the prayer-flag fade */}
      <div style={{
        position: "absolute", left: 0, right: 0, bottom: 0, height: 200,
        background: "linear-gradient(180deg, transparent 0%, var(--bg-deepest) 100%)",
        pointerEvents: "none",
      }} />

      <header style={{
        padding: "40px 64px 28px",
        display: "flex", alignItems: "baseline", justifyContent: "space-between",
      }}>
        <div>
          <Eyebrow>Hand · 和了 · 東 2 局</Eyebrow>
          <h1 style={{
            font: "var(--t-h1)",
            letterSpacing: "var(--tracking-tight)",
            marginTop: 10,
            color: PIG.gser,
          }}>立直 · 平和 · 一発</h1>
          <p style={{
            font: "var(--t-lede)",
            color: "var(--fg-secondary)", marginTop: 8,
            fontStyle: "italic",
          }}>
            7 翻 30 符 · 跳満 · 你自摸了七筒
          </p>
        </div>
        <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
          <KeyBadge k="N" tone="primary" label="下一局" />
          <KeyBadge k="L" label="日志" size="sm" />
        </div>
      </header>

      <main style={{ padding: "0 64px", display: "flex", flexDirection: "column", gap: 32 }}>
        {/* winning hand + dora */}
        <section>
          <Eyebrow style={{ marginBottom: 14 }}>Winning hand · 和了形</Eyebrow>
          <div style={{ display: "flex", gap: 18, alignItems: "flex-end" }}>
            <div style={{ display: "flex", gap: 4 }}>
              <Tile t="m1" size="lg" /><Tile t="m2" size="lg" /><Tile t="m3" size="lg" />
            </div>
            <div style={{ display: "flex", gap: 4 }}>
              <Tile t="p3" size="lg" /><Tile t="p4" size="lg" /><Tile t="p5r" size="lg" /><Tile t="p6" size="lg" /><Tile t="p7" size="lg" state="draw" />
            </div>
            <div style={{ display: "flex", gap: 4 }}>
              <Tile t="s2" size="lg" /><Tile t="s3" size="lg" /><Tile t="s4" size="lg" />
            </div>
            <div style={{ display: "flex", gap: 4 }}>
              <Tile t="z1" size="lg" /><Tile t="z1" size="lg" />
            </div>
          </div>
          <div style={{ marginTop: 14, display: "flex", gap: 28, color: "var(--fg-tertiary)", fontSize: 13 }}>
            <span><span style={{ color: PIG.gser }}>★</span> 自摸 — Tsumo</span>
            <span>14 张 · 标准型</span>
            <span style={{ color: PIG.gser }}>七筒 · 待ち牌</span>
          </div>
        </section>

        {/* yaku breakdown */}
        <section style={{ display: "grid", gridTemplateColumns: "1fr 360px", gap: 48 }}>
          <div>
            <Eyebrow style={{ marginBottom: 14 }}>Yaku · 役</Eyebrow>
            <div style={{ display: "flex", flexDirection: "column", gap: 1 }}>
              <YakuRow name="立直 · Riichi" han={1} />
              <YakuRow name="平和 · Pinfu" han={1} />
              <YakuRow name="一発 · Ippatsu" han={1} accent />
              <YakuRow name="門前清自摸和 · Menzen tsumo" han={1} />
              <YakuRow name="一盃口 · Iipeiko" han={1} />
              <YakuRow name="ドラ · Dora" han={1} />
              <YakuRow name="赤ドラ · Akadora" han={1} />
            </div>
            <div style={{
              display: "flex", justifyContent: "space-between",
              padding: "16px 0", marginTop: 8,
              borderTop: "1px solid var(--border-default)",
              fontSize: "var(--text-lg)",
            }}>
              <span style={{ color: "var(--fg-secondary)" }}>30 符 · 7 翻</span>
              <span style={{
                fontFamily: "var(--font-serif)", fontSize: "var(--text-3xl)",
                color: PIG.gser, letterSpacing: "var(--tracking-tight)",
              }}>跳満 · Haneman</span>
            </div>
          </div>

          <div>
            <Eyebrow style={{ marginBottom: 14 }}>Score · 点数移动</Eyebrow>
            <div style={{
              background: "var(--bg-deep)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-lg)",
              padding: 20,
            }}>
              <div style={{ fontSize: "var(--text-3xl)", fontFamily: "var(--font-mono)", color: PIG.gser, fontWeight: 600 }}>
                +12,000
              </div>
              <div style={{ color: "var(--fg-tertiary)", fontSize: 13, marginTop: 4 }}>
                自摸 · 跳満 · 親 = 4,000 all
              </div>

              <div style={{ marginTop: 18, display: "flex", flexDirection: "column", gap: 8 }}>
                <DeltaRow seat="東 · 自家" delta="+12,000" before={26500} after={38500} you />
                <DeltaRow seat="南 · 下家" delta="-4,000" before={24000} after={20000} />
                <DeltaRow seat="西 · 对家" delta="-4,000" before={24500} after={20500} />
                <DeltaRow seat="北 · 上家" delta="-4,000" before={25000} after={21000} />
              </div>

              <div style={{ marginTop: 16, padding: "10px 12px", background: "var(--bg-surface0)", borderRadius: "var(--radius-md)" }}>
                <Row label="Honba · 本場" value="1 → 2 (連庄)" />
                <Row label="Kyotaku · 供託" value="1 → 0" />
              </div>
            </div>
          </div>
        </section>

        {/* ura dora reveal */}
        <section style={{ display: "flex", gap: 28, alignItems: "center" }}>
          <Eyebrow>Ura dora · 裏宝</Eyebrow>
          <div style={{ display: "flex", gap: 4 }}>
            <Tile t="m6" size="sm" />
          </div>
          <span style={{ color: "var(--fg-tertiary)", fontSize: 13 }}>
            指示 m6 → ドラ 七萬 · 命中 0 张
          </span>
        </section>
      </main>
    </div>
  );
}

function YakuRow({ name, han, accent }) {
  return (
    <div style={{
      display: "flex", justifyContent: "space-between", alignItems: "center",
      padding: "10px 0", borderBottom: "1px solid var(--border-subtle)",
    }}>
      <span style={{
        fontSize: 14, color: accent ? PIG.gser : "var(--fg-primary)",
      }}>{name}</span>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 14,
        color: accent ? PIG.gser : "var(--fg-secondary)",
      }}>{han} 翻</span>
    </div>
  );
}

function DeltaRow({ seat, delta, before, after, you }) {
  const positive = delta.startsWith("+");
  return (
    <div style={{
      display: "grid", gridTemplateColumns: "1fr auto auto",
      alignItems: "center", gap: 14,
      padding: "4px 0",
    }}>
      <span style={{ color: you ? PIG.gser : "var(--fg-secondary)", fontSize: 13 }}>{seat}</span>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 14,
        color: positive ? PIG.gser : PIG.mtshal, fontWeight: 600,
      }}>{delta}</span>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 12,
        color: "var(--fg-tertiary)",
        minWidth: 90, textAlign: "right",
      }}>
        {before.toLocaleString()} → {after.toLocaleString()}
      </span>
    </div>
  );
}

// ── Match end ──────────────────────────────────────────────────────
function MatchEndScreen() {
  const results = [
    { rank: 1, seat: "西", name: "对家", base: 39500, uma: "+30", final: 42500 },
    { rank: 2, seat: "東", name: "你", base: 28500, uma: "+10", final: 29500, you: true },
    { rank: 3, seat: "北", name: "上家", base: 18500, uma: "-10", final: 17500 },
    { rank: 4, seat: "南", name: "下家", base: 13500, uma: "-30", final: 10500 },
  ];

  return (
    <div data-screen-label="Match end · 終局"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)", color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        position: "relative", overflow: "hidden",
      }}>

      <header style={{
        padding: "56px 64px 36px",
      }}>
        <Eyebrow>Match end · 終局 · 半庄战</Eyebrow>
        <h1 style={{
          font: "var(--t-h1)",
          letterSpacing: "var(--tracking-tight)",
          marginTop: 12,
        }}>
          <span style={{ color: PIG.gser }}>对家</span>
          <span style={{ color: "var(--fg-secondary)", margin: "0 14px", fontWeight: 300 }}>·</span>
          <span style={{ fontStyle: "italic", color: "var(--fg-secondary)", fontFamily: "var(--font-serif)" }}>winner</span>
        </h1>
        <p style={{ marginTop: 8, color: "var(--fg-tertiary)" }}>
          25,000 起点 · 30,000 终点 · uma 20-10-(-10)-(-20) · 8 局 1 时 14 分
        </p>
      </header>

      <main style={{
        padding: "0 64px",
        display: "grid", gridTemplateColumns: "1fr 1fr",
        gap: 56,
      }}>
        {/* standings */}
        <section>
          <Eyebrow style={{ marginBottom: 16 }}>Standings · 顺位</Eyebrow>
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {results.map((r) => (
              <StandingRow key={r.rank} r={r} />
            ))}
          </div>
        </section>

        {/* stats */}
        <section>
          <Eyebrow style={{ marginBottom: 16 }}>Your match · 你的本场</Eyebrow>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <BigStat label="和了率" value="33%" sub="3 / 9 局" />
            <BigStat label="放铳率" value="11%" sub="1 / 9 局" />
            <BigStat label="副露率" value="22%" sub="2 / 9 局" />
            <BigStat label="平均打点" value="6,800" sub="跳满 ×1 · 满貫 ×1" mono />
            <BigStat label="立直" value="3 回" sub="2 / 3 命中" />
            <BigStat label="役満" value="0 回" sub="—" muted />
          </div>
        </section>
      </main>

      <footer style={{
        position: "absolute", bottom: 0, left: 0, right: 0,
        padding: "20px 64px",
        background: "var(--bg-deepest)",
        borderTop: "1px solid var(--border-subtle)",
        display: "flex", alignItems: "center", justifyContent: "space-between",
      }}>
        <div style={{ display: "flex", gap: 14 }}>
          <KeyBadge k="N" tone="primary" label="再来一局" />
          <KeyBadge k="S" label="保存牌谱" size="sm" />
          <KeyBadge k="R" label="重放" size="sm" />
        </div>
        <KeyBadge k="Esc" label="主菜单" size="sm" />
      </footer>
    </div>
  );
}

function StandingRow({ r }) {
  const colors = ["", PIG.gser, PIG.dri_med, PIG.dul_ba, "var(--fg-disabled)"];
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: "auto auto 1fr auto auto",
      alignItems: "center", gap: 18,
      padding: "16px 22px",
      background: r.you ? "var(--accent-soft)" : "var(--bg-surface0)",
      border: `1px solid ${r.you ? "var(--border-focus)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-lg)",
    }}>
      <span style={{
        fontFamily: "var(--font-serif)",
        fontSize: "var(--text-4xl)", fontWeight: 300,
        color: colors[r.rank],
        lineHeight: 1, minWidth: 40,
      }}>{r.rank}</span>
      <span style={{
        fontFamily: "var(--font-serif)", fontSize: "var(--text-xl)",
        color: r.you ? PIG.gser : "var(--fg-primary)",
      }}>{r.seat}</span>
      <div>
        <div style={{ fontSize: "var(--text-md)", color: "var(--fg-primary)" }}>{r.name}</div>
        <div style={{ fontSize: 11, color: "var(--fg-tertiary)", marginTop: 2 }}>
          base {r.base.toLocaleString()} · uma {r.uma}
        </div>
      </div>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: "var(--text-xl)",
        color: r.you ? PIG.gser : "var(--fg-primary)", fontWeight: 600,
      }}>{r.final.toLocaleString()}</span>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: 12,
        color: r.rank <= 2 ? PIG.ljangkhu : PIG.mtshal, fontWeight: 600,
      }}>{r.uma}</span>
    </div>
  );
}

function BigStat({ label, value, sub, mono, muted }) {
  return (
    <div style={{
      padding: 18,
      background: "var(--bg-surface0)",
      border: "1px solid var(--border-default)",
      borderRadius: "var(--radius-lg)",
      opacity: muted ? 0.5 : 1,
    }}>
      <Eyebrow style={{ fontSize: 10 }}>{label}</Eyebrow>
      <div style={{
        marginTop: 8,
        fontFamily: mono ? "var(--font-mono)" : "var(--font-serif)",
        fontSize: "var(--text-3xl)", fontWeight: mono ? 600 : 400,
        letterSpacing: "var(--tracking-tight)",
        color: "var(--fg-primary)",
        lineHeight: 1,
      }}>{value}</div>
      <div style={{ fontSize: 12, color: "var(--fg-tertiary)", marginTop: 6 }}>{sub}</div>
    </div>
  );
}

window.HandResultScreen = HandResultScreen;
window.MatchEndScreen = MatchEndScreen;
