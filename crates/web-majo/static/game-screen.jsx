// Main game screen — single player / Standard mode.
// Layout: top status bar · 4-player table · bottom hand · right info rail.

function GameScreen({ showActionModal = false, mode = "NORMAL", commandText = "discard p4", showRiichiStick = false }) {
  const g = GAME;
  const me = g.players[0], shimo = g.players[1], toi = g.players[2], kami = g.players[3];

  return (
    <div data-screen-label="Game · main"
      style={{
        width: 1440, height: 900,
        background: "var(--bg-base)",
        color: "var(--fg-primary)",
        fontFamily: "var(--font-sans)",
        position: "relative",
        display: "grid",
        gridTemplateColumns: "1fr 320px",
        gridTemplateRows: "56px 1fr 156px",
        gridTemplateAreas: `
          "top top"
          "table side"
          "hand side"
        `,
      }}>

      {/* ── top status bar ────────────────────────────────────── */}
      <header style={{
        gridArea: "top",
        background: "var(--bg-deepest)",
        borderBottom: "1px solid var(--border-subtle)",
        display: "grid", gridTemplateColumns: "1fr auto 1fr",
        alignItems: "center", padding: "0 32px",
        backdropFilter: "blur(20px) saturate(140%)",
      }}>
        <div style={{ display: "flex", gap: 24, alignItems: "center" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <DorjeMark size={20} />
            <span style={{
              font: "var(--t-eyebrow)", letterSpacing: "var(--tracking-widest)",
              textTransform: "uppercase", color: "var(--fg-tertiary)",
            }}>tui-majo</span>
          </div>
          <Divider />
          <Stat label="Round · 局" value={`${g.round.wind} ${g.round.num}`} />
          <Stat label="Honba · 本場" value={String(g.round.honba)} />
          <Stat label="Kyotaku · 供託" value={String(g.round.riichi)} tone={g.round.riichi > 0 ? "warning" : null} />
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 14, justifyContent: "center" }}>
          <Eyebrow>Dora · 宝牌</Eyebrow>
          <Tile t={g.dora[0]} size="xs" />
          <Tile t="?" size="xs" />
          <Tile t="?" size="xs" />
        </div>

        <div style={{ display: "flex", gap: 20, alignItems: "center", justifyContent: "flex-end" }}>
          <Stat label="Junme · 巡" value={String(g.junme)} />
          <Stat label="Wall · 山" value={String(g.wall)} tone={g.wall < 16 ? "warning" : null} />
          <Divider />
          <Stat label="" value="20:14" mono />
          <KeyBadge k="?" label="Help" size="sm" />
          <KeyBadge k="Esc" label="Menu" size="sm" />
        </div>
      </header>

      {/* ── table ─────────────────────────────────────────────── */}
      <section style={{
        gridArea: "table",
        position: "relative",
        overflow: "hidden",
      }}>
        <Table players={g.players} dora={g.dora[0]} wall={g.wall} junme={g.junme} round={g.round} />
      </section>

      {/* ── own hand strip ─────────────────────────────────────── */}
      <section style={{
        gridArea: "hand",
        background: "var(--bg-deep)",
        borderTop: "1px solid var(--border-subtle)",
        padding: "20px 32px 16px",
        display: "flex", flexDirection: "column", gap: 12,
      }}>
        <HandStrip me={me} mode={mode} commandText={commandText} />
      </section>

      {/* ── right rail ─────────────────────────────────────────── */}
      <aside style={{
        gridArea: "side",
        background: "var(--bg-deep)",
        borderLeft: "1px solid var(--border-subtle)",
        padding: "20px 20px 16px",
        overflowY: "auto",
        display: "flex", flexDirection: "column", gap: 20,
      }}>
        <SidePanel game={g} me={me} />
      </aside>

      {showActionModal ? <ActionModal /> : null}
    </div>
  );
}

// ── DorjeMark ─────────────────────────────────────────────────────
function DorjeMark({ size = 20 }) {
  // simplified vajra silhouette: two diamond pommels + center sphere
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="var(--accent)" strokeWidth="1.2">
      <circle cx="10" cy="10" r="2" fill="var(--accent)" />
      <path d="M10 2 L8 5 L10 8 L12 5 Z" />
      <path d="M10 18 L8 15 L10 12 L12 15 Z" />
      <path d="M2 10 L5 8 L8 10 L5 12 Z" />
      <path d="M18 10 L15 8 L12 10 L15 12 Z" />
    </svg>
  );
}

// ── Stat (top bar) ────────────────────────────────────────────────
function Stat({ label, value, tone, mono = false }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2, lineHeight: 1 }}>
      {label ? (
        <span style={{
          font: "var(--t-eyebrow)",
          letterSpacing: "var(--tracking-wide)",
          textTransform: "uppercase",
          color: "var(--fg-tertiary)",
          fontSize: 10,
        }}>{label}</span>
      ) : null}
      <span style={{
        fontSize: "var(--text-md)",
        fontWeight: 500,
        color: tone === "warning" ? PIG.likhri : "var(--fg-primary)",
        fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
        letterSpacing: mono ? "0" : "var(--tracking-tight)",
      }}>{value}</span>
    </div>
  );
}

function Divider() {
  return <span style={{ width: 1, height: 24, background: "var(--border-default)" }} />;
}

// ── Table layout ─────────────────────────────────────────────────
// Real-mahjong-table layout. From outside in:
//   each player's HAND (at edge) → their DISCARDS → the WALL (center) → plate.
// Discards sit between each player's hand and the wall — "above" their hand from
// their POV. Wall is in the very center, showing remaining tile count + dora.
function Table({ players, dora, wall, junme, round }) {
  const me = players[0], shimo = players[1], toi = players[2], kami = players[3];

  return (
    <div style={{
      position: "absolute", inset: 0,
      display: "flex", justifyContent: "center", alignItems: "center",
    }}>
      <div style={{
        width: 920, height: 600,
        position: "relative",
      }}>
        {/* opponent hand backs at edges */}
        <div style={{
          position: "absolute", top: 14, left: "50%",
          transform: "translateX(-50%) rotate(180deg)",
        }}>
          <TileRow tiles={Array(13).fill("?")} size="xs" gap={3} />
        </div>

        <div style={{
          position: "absolute", left: 14, top: "50%",
          transform: "translateY(-50%) rotate(90deg)",
          transformOrigin: "center center",
        }}>
          <TileRow tiles={Array(13).fill("?")} size="xs" gap={3} />
        </div>

        <div style={{
          position: "absolute", right: 14, top: "50%",
          transform: "translateY(-50%) rotate(-90deg)",
          transformOrigin: "center center",
        }}>
          <TileRow tiles={Array(13).fill("?")} size="xs" gap={3} />
        </div>

        {/* seat labels at table corners — each near their player's hand */}
        <SeatLabelInline {...toi}
          style={{ position: "absolute", top: 60, right: 36 }} />
        <SeatLabelInline {...kami}
          style={{ position: "absolute", top: 60, left: 36 }} />
        <SeatLabelInline {...shimo}
          style={{ position: "absolute", bottom: 60, right: 36 }} />
        <SeatLabelInline {...me} you
          style={{ position: "absolute", bottom: 60, left: 36 }} />

        {/* center cluster — wall in middle + discards radiating out */}
        <CenterCluster
          selfTiles={me.discards}
          oppTiles={toi.discards}
          leftTiles={kami.discards}
          rightTiles={shimo.discards}
          oppRiichiAt={toi.riichiAt}
          wall={wall}
          dora={dora}
          junme={junme}
          round={round}
        />

        {/* melds for kami/shimo near their hands */}
        <div style={{ position: "absolute", bottom: 96, left: 14, display: "flex", gap: 4, transform: "rotate(90deg)", transformOrigin: "left bottom" }}>
          {kami.melds.map((m, i) => <MeldGroup key={i} meld={m} />)}
        </div>
        <div style={{ position: "absolute", top: 96, right: 14, display: "flex", gap: 4, transform: "rotate(-90deg)", transformOrigin: "right top" }}>
          {shimo.melds.map((m, i) => <MeldGroup key={i} meld={m} />)}
        </div>
      </div>
    </div>
  );
}

// ── CenterCluster — wall in the middle, 4 discard piles radiating out ──
function CenterCluster({ selfTiles, oppTiles, leftTiles, rightTiles, oppRiichiAt, wall, dora, junme, round }) {
  // xs tile = 24×32. Discard pile: 6 cols × 3 rows = 6*24+5*3 = 159 × 3*32+2*3 = 102
  const DW = 159, DH = 102;
  const wallSize = 168;
  const gap = 16;

  return (
    <div style={{
      position: "absolute", left: "50%", top: "50%",
      transform: "translate(-50%, -50%)",
      display: "grid",
      gridTemplateColumns: `${DH}px ${gap}px ${wallSize}px ${gap}px ${DH}px`,
      gridTemplateRows: `${DH}px ${gap}px ${wallSize}px ${gap}px ${DH}px`,
      alignItems: "center", justifyItems: "center",
    }}>
      {/* opp (top, rotated 180°) */}
      <div style={{ gridRow: 1, gridColumn: 3, transform: "rotate(180deg)" }}>
        <DiscardPileFlat tiles={oppTiles.slice(0, 18)} riichiAt={oppRiichiAt ?? -1} />
      </div>

      {/* 上家 (left, rotated 90° CW) */}
      <div style={{ gridRow: 3, gridColumn: 1, transform: "rotate(90deg)" }}>
        <DiscardPileFlat tiles={leftTiles.slice(0, 18)} />
      </div>

      {/* central wall + plate */}
      <div style={{ gridRow: 3, gridColumn: 3 }}>
        <Wall remaining={wall} dora={dora} junme={junme} round={round} size={wallSize} />
      </div>

      {/* 下家 (right, rotated -90° CCW) */}
      <div style={{ gridRow: 3, gridColumn: 5, transform: "rotate(-90deg)" }}>
        <DiscardPileFlat tiles={rightTiles.slice(0, 18)} />
      </div>

      {/* self (bottom, normal orientation) */}
      <div style={{ gridRow: 5, gridColumn: 3 }}>
        <DiscardPileFlat tiles={selfTiles.slice(0, 18)} />
      </div>
    </div>
  );
}

// ── DiscardPileFlat — 6 cols × 3 rows, no internal rotation ────────
function DiscardPileFlat({ tiles, riichiAt = -1 }) {
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: "repeat(6, max-content)",
      gridAutoRows: "max-content",
      gap: 3,
    }}>
      {tiles.map((t, i) => (
        <Tile key={i} t={t} size="xs"
          state={i === riichiAt ? "riichi" : i === tiles.length - 1 ? "discarded-recent" : "normal"}
          rotate={i === riichiAt ? 90 : 0} />
      ))}
    </div>
  );
}

// ── Wall — 4 sides of stacked tile-back slots + center plate ───────
// Simulates drawing: as `remaining` decreases, slots empty from a corner.
function Wall({ remaining = 47, total = 70, dora, junme, round, size = 168 }) {
  const SLOTS_PER_SIDE = 11;
  const TOTAL_SLOTS = SLOTS_PER_SIDE * 4;
  const filled = Math.max(0, Math.round((remaining / total) * TOTAL_SLOTS));
  const wallThickness = 14;
  const margin = wallThickness + 2;

  // Drawing order goes CCW from a starting corner.
  // Slot 0..10 = top (L→R); 11..21 = right (T→B); 22..32 = bottom (R→L); 33..43 = left (B→T)
  // "filled" = number of remaining slots (drawn from the END of the sequence,
  // so empty slots appear at the END = the draw cursor).
  const isFilled = (i) => i < filled;

  // Draw cursor: where next tile will come from = position `filled`
  const cursorSide = filled < SLOTS_PER_SIDE ? "top"
    : filled < SLOTS_PER_SIDE * 2 ? "right"
    : filled < SLOTS_PER_SIDE * 3 ? "bottom"
    : "left";

  return (
    <div style={{ position: "relative", width: size, height: size }}>
      {/* top wall */}
      <div style={{
        position: "absolute", top: 0, left: wallThickness, right: wallThickness,
        height: wallThickness,
        display: "flex", gap: 1, alignItems: "center", justifyContent: "center",
      }}>
        {Array.from({ length: SLOTS_PER_SIDE }).map((_, i) => (
          <WallSlot key={i} filled={isFilled(i)} variant="horizontal"
            cursor={i === filled && cursorSide === "top"} />
        ))}
      </div>

      {/* right wall */}
      <div style={{
        position: "absolute", right: 0, top: wallThickness, bottom: wallThickness,
        width: wallThickness,
        display: "flex", flexDirection: "column", gap: 1, alignItems: "center", justifyContent: "center",
      }}>
        {Array.from({ length: SLOTS_PER_SIDE }).map((_, i) => {
          const idx = SLOTS_PER_SIDE + i;
          return <WallSlot key={i} filled={isFilled(idx)} variant="vertical"
            cursor={idx === filled && cursorSide === "right"} />;
        })}
      </div>

      {/* bottom wall (reversed direction) */}
      <div style={{
        position: "absolute", bottom: 0, left: wallThickness, right: wallThickness,
        height: wallThickness,
        display: "flex", gap: 1, alignItems: "center", justifyContent: "center",
      }}>
        {Array.from({ length: SLOTS_PER_SIDE }).map((_, i) => {
          const idx = SLOTS_PER_SIDE * 2 + (SLOTS_PER_SIDE - 1 - i);
          return <WallSlot key={i} filled={isFilled(idx)} variant="horizontal"
            cursor={idx === filled && cursorSide === "bottom"} />;
        })}
      </div>

      {/* left wall (reversed) */}
      <div style={{
        position: "absolute", left: 0, top: wallThickness, bottom: wallThickness,
        width: wallThickness,
        display: "flex", flexDirection: "column", gap: 1, alignItems: "center", justifyContent: "center",
      }}>
        {Array.from({ length: SLOTS_PER_SIDE }).map((_, i) => {
          const idx = SLOTS_PER_SIDE * 3 + (SLOTS_PER_SIDE - 1 - i);
          return <WallSlot key={i} filled={isFilled(idx)} variant="vertical"
            cursor={idx === filled && cursorSide === "left"} />;
        })}
      </div>

      {/* center plate inside the wall */}
      <div style={{
        position: "absolute",
        inset: margin,
        background: "var(--bg-deepest)",
        border: "1px solid var(--border-default)",
        borderRadius: "var(--radius-md)",
        padding: "10px 12px",
        boxSizing: "border-box",
        display: "flex", flexDirection: "column", justifyContent: "space-between",
        alignItems: "center", gap: 4,
        boxShadow: "var(--shadow-2)",
      }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 6 }}>
          <span style={{
            fontFamily: "var(--font-serif)", fontSize: 18,
            color: PIG.gser, letterSpacing: "var(--tracking-tight)",
            lineHeight: 1,
          }}>{round.wind} {round.num}</span>
          <span style={{ color: "var(--fg-tertiary)", fontSize: 11 }}>·</span>
          <span style={{ color: "var(--fg-secondary)", fontSize: 11 }}>{round.honba} 本</span>
        </div>

        <div style={{ textAlign: "center" }}>
          <div style={{
            fontFamily: "var(--font-mono)",
            fontSize: 28, fontWeight: 600, lineHeight: 1,
            color: remaining < 16 ? PIG.likhri : "var(--fg-primary)",
          }}>{remaining}</div>
          <div style={{
            font: "var(--t-eyebrow)",
            letterSpacing: "var(--tracking-widest)",
            textTransform: "uppercase",
            color: "var(--fg-tertiary)",
            fontSize: 9, marginTop: 2,
          }}>tiles · 山</div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <Tile t={dora} size="xs" />
          <span style={{
            color: "var(--fg-tertiary)", fontSize: 10,
            letterSpacing: "var(--tracking-wide)",
          }}>dora · 巡 {junme}</span>
        </div>
      </div>
    </div>
  );
}

// ── Single slot in the wall — filled (tile-back) or empty (drawn) ──
function WallSlot({ filled, variant, cursor }) {
  const isV = variant === "vertical";
  return (
    <div style={{
      width: isV ? 8 : 12,
      height: isV ? 12 : 8,
      background: filled
        ? "linear-gradient(180deg, #2A3A60 0%, #1F2A48 60%, #142038 100%)"
        : "transparent",
      border: filled
        ? "1px solid #0F1830"
        : `1px dashed ${cursor ? "rgba(210,180,80,0.6)" : "rgba(232,224,212,0.12)"}`,
      borderRadius: 1.5,
      boxSizing: "border-box",
      boxShadow: filled
        ? "0 1px 0 rgba(0,0,0,0.4), inset 0 0.5px 0 rgba(255,255,255,0.10)"
        : cursor ? "0 0 4px rgba(210,180,80,0.45)" : "none",
    }} />
  );
}

// ── Compact seat label (inline, no rotation — always upright) ────
function SeatLabelInline({ seat, name, score, riichi, dealer, you, style = {} }) {
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 10,
      padding: "5px 12px",
      background: "var(--bg-deep)",
      border: `1px solid ${you ? "var(--border-focus)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-md)",
      whiteSpace: "nowrap",
      ...style,
    }}>
      <span style={{
        fontFamily: "var(--font-serif)",
        fontSize: "var(--text-lg)",
        fontWeight: 500,
        color: you ? PIG.gser : dealer ? PIG.gser : "var(--fg-primary)",
        lineHeight: 1,
      }}>{seat}</span>
      <div style={{ display: "flex", flexDirection: "column", gap: 0, lineHeight: 1.15 }}>
        <div style={{ fontSize: 11, color: "var(--fg-tertiary)", display: "flex", gap: 6, alignItems: "center" }}>
          {name}
          {dealer ? <span style={{ color: PIG.gser, fontWeight: 600 }}>庄</span> : null}
          {riichi ? <span style={{ color: PIG.mtshal, fontWeight: 600 }}>立直</span> : null}
        </div>
        <div style={{
          fontSize: "var(--text-md)", fontWeight: 600,
          fontFamily: "var(--font-mono)",
          color: you ? PIG.gser : "var(--fg-primary)",
        }}>{score.toLocaleString()}</div>
      </div>
    </div>
  );
}

// ── Compact central plate (no absolute positioning — for use in grid) ──
function CentralPlateCompact({ dora, wall, junme, round, w, h }) {
  return (
    <div style={{
      width: w, height: h,
      background: "var(--bg-deepest)",
      border: "1px solid var(--border-default)",
      borderRadius: "var(--radius-lg)",
      padding: "14px 16px",
      display: "flex", flexDirection: "column", gap: 10,
      justifyContent: "space-between",
      boxSizing: "border-box",
      boxShadow: "var(--shadow-2)",
    }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, justifyContent: "center" }}>
        <span style={{
          fontFamily: "var(--font-serif)", fontSize: "var(--text-xl)",
          color: PIG.gser, letterSpacing: "var(--tracking-tight)",
        }}>{round.wind} {round.num}</span>
        <span style={{ color: "var(--fg-tertiary)", fontSize: 12 }}>·</span>
        <span style={{ color: "var(--fg-secondary)", fontSize: 12 }}>{round.honba} 本場</span>
      </div>
      <div style={{
        display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8,
        textAlign: "center",
      }}>
        <KV label="Wall · 山" value={String(wall)} />
        <KV label="Junme · 巡" value={String(junme)} />
      </div>
      <div style={{ display: "flex", justifyContent: "center", gap: 6, alignItems: "center" }}>
        <Tile t={dora} size="xs" />
        <span style={{ color: "var(--fg-tertiary)", fontSize: 11, letterSpacing: "var(--tracking-wide)" }}>dora</span>
      </div>
    </div>
  );
}

function KV({ label, value }) {
  return (
    <div>
      <div style={{
        font: "var(--t-eyebrow)", letterSpacing: "var(--tracking-wide)",
        textTransform: "uppercase", color: "var(--fg-tertiary)", fontSize: 9,
      }}>{label}</div>
      <div style={{
        fontFamily: "var(--font-mono)", fontSize: "var(--text-lg)",
        color: "var(--fg-primary)", lineHeight: 1.1,
      }}>{value}</div>
    </div>
  );
}

// ── Meld group ───────────────────────────────────────────────────
function MeldGroup({ meld }) {
  return (
    <div style={{ display: "flex", gap: 1, alignItems: "flex-end" }}>
      {meld.tiles.map((t, i) => (
        <Tile key={i} t={t} size="xs"
          rotate={meld.type === "chi" && i === 0 ? -90 : meld.type === "pon" && i === 0 ? -90 : 0}
        />
      ))}
    </div>
  );
}

// ── Hand strip (own hand) ────────────────────────────────────────
function HandStrip({ me, mode, commandText }) {
  return (
    <>
      {/* hand row */}
      <div style={{ display: "flex", alignItems: "flex-end", gap: 20, justifyContent: "center" }}>
        <TileRow tiles={me.hand} size="md" gap={3} selected={6} />
        <div style={{
          width: 1, height: 52, background: "var(--border-default)",
        }} />
        <Tile t={me.draw} size="md" state="draw" />
      </div>

      {/* action band */}
      <div style={{
        display: "flex", alignItems: "center", justifyContent: "space-between",
        gap: 20, marginTop: 6,
      }}>
        <div style={{ display: "flex", alignItems: "center", gap: 16, flexWrap: "wrap" }}>
          {mode === "COMMAND" ? (
            <div style={{
              display: "flex", alignItems: "center", gap: 4,
              padding: "6px 12px",
              background: "var(--bg-surface0)",
              border: "1px solid var(--border-focus)",
              borderRadius: "var(--radius-md)",
              fontFamily: "var(--font-mono)",
              fontSize: 14, minWidth: 280,
            }}>
              <span style={{ color: PIG.gser, fontWeight: 600 }}>:</span>
              <span style={{ color: "var(--fg-primary)" }}>{commandText}</span>
              <span style={{
                display: "inline-block", width: 8, height: 16,
                background: PIG.gser, marginLeft: 2,
                animation: "blink 1s steps(2) infinite",
              }} />
            </div>
          ) : (
            <>
              <KeyBadge k="1-9" tone="primary" label="选牌" />
              <KeyBadge k="D" label="切" />
              <KeyBadge k="T" label="摸切" />
              <KeyBadge k="R" tone="danger" label="立直" />
              <KeyBadge k="W" tone="ok" label="自摸" />
              <KeyBadge k="K" label="暗杠" />
              <span style={{ width: 1, height: 18, background: "var(--border-default)" }} />
              <KeyBadge k=":" label="命令" />
              <KeyBadge k="M" label="菜单" />
            </>
          )}
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{
            font: "var(--t-eyebrow)", letterSpacing: "var(--tracking-widest)",
            textTransform: "uppercase",
            color: mode === "COMMAND" ? PIG.gser : PIG.ljangkhu,
            background: mode === "COMMAND" ? "var(--accent-soft)" : "var(--status-success-bg)",
            padding: "4px 12px",
            borderRadius: "var(--radius-sm)",
            fontWeight: 600, fontSize: 11,
          }}>
            {mode}
          </span>
          <span style={{ color: "var(--fg-tertiary)", fontSize: 12, fontFamily: "var(--font-mono)" }}>
            h j k l · ← →
          </span>
        </div>
      </div>
    </>
  );
}

// ── Side panel ───────────────────────────────────────────────────
function SidePanel({ game, me }) {
  return (
    <>
      {/* 得点 */}
      <section>
        <Eyebrow style={{ marginBottom: 10 }}>Scores · 得点</Eyebrow>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {game.players.map((p, i) => (
            <SidePlayer key={i} p={p} you={i === 0} />
          ))}
        </div>
      </section>

      <Hr />

      {/* tenpai / hand analysis */}
      <section>
        <Eyebrow style={{ marginBottom: 10 }}>Hand · 自家</Eyebrow>
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <Row label="Shanten · 向聴" value={
            <span style={{ color: PIG.ljangkhu, fontWeight: 600 }}>0 · 聴牌</span>
          } />
          <Row label="Tenpai · 待ち" value={
            <div style={{ display: "flex", gap: 3 }}>
              {me.tenpai.map((t, i) => <Tile key={i} t={t} size="xs" state="tenpai" />)}
            </div>
          } />
          <Row label="Tiles · 残" value="6 枚" mono />
          <Row label="Yaku · 役" value={
            <span style={{ color: PIG.ngonpo }}>立直 · 平和 · ドラ</span>
          } />
          <Row label="Score · 打点" value={
            <span style={{ fontFamily: "var(--font-mono)" }}>
              5,200 <span style={{ color: "var(--fg-tertiary)" }}>/</span> 7,700
            </span>
          } />
        </div>
      </section>

      <Hr />

      {/* danger */}
      <section>
        <Eyebrow style={{ marginBottom: 10, color: PIG.mtshal }}>Danger · 危険</Eyebrow>
        <div style={{ display: "flex", gap: 4 }}>
          {game.danger.map((t, i) => <Tile key={i} t={t} size="xs" state="danger" />)}
        </div>
        <div style={{ marginTop: 8, fontSize: 12, color: "var(--fg-tertiary)" }}>
          对家立直巡 · 至 8 巡前过的牌
        </div>
      </section>

      <Hr />

      {/* log */}
      <section style={{ flex: 1 }}>
        <Eyebrow style={{ marginBottom: 10 }}>Log · 対局</Eyebrow>
        <div style={{ display: "flex", flexDirection: "column", gap: 6, fontSize: 12 }}>
          {game.log.map((e, i) => (
            <div key={i} style={{
              display: "flex", alignItems: "center", gap: 8,
              color: e.emphasize ? "var(--fg-primary)" : "var(--fg-tertiary)",
            }}>
              <span style={{ fontFamily: "var(--font-mono)", color: "var(--fg-disabled)" }}>
                {String(e.junme).padStart(2, "0")}
              </span>
              <span style={{ color: e.emphasize ? PIG.gser : "var(--fg-secondary)" }}>{e.who}</span>
              <span>{e.action}</span>
              <Tile t={e.tile} size="xs" />
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

function SidePlayer({ p, you }) {
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
        fontFamily: "var(--font-serif)", fontSize: "var(--text-md)",
        color: you ? PIG.gser : p.riichi ? PIG.mtshal : "var(--fg-primary)",
        lineHeight: 1,
      }}>{p.seat}</span>
      <span style={{ color: "var(--fg-tertiary)", fontSize: 12 }}>{p.name}</span>
      <span style={{
        fontFamily: "var(--font-mono)", fontSize: "var(--text-base)",
        textAlign: "right",
        color: you ? PIG.gser : "var(--fg-primary)",
      }}>{p.score.toLocaleString()}</span>
      <div style={{ display: "flex", gap: 4, minWidth: 36, justifyContent: "flex-end" }}>
        {p.dealer ? <Mini t="庄" tone="accent" /> : null}
        {p.riichi ? <Mini t="立" tone="danger" /> : null}
      </div>
    </div>
  );
}

function Mini({ t, tone }) {
  const color = tone === "accent" ? PIG.gser : tone === "danger" ? PIG.mtshal : "var(--fg-primary)";
  return (
    <span style={{
      fontSize: 10, fontWeight: 600,
      color, border: `1px solid ${color}`, borderRadius: 2,
      padding: "1px 4px", lineHeight: 1,
    }}>{t}</span>
  );
}

function Row({ label, value, mono }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
      <span style={{ color: "var(--fg-tertiary)", fontSize: 12 }}>{label}</span>
      <span style={{
        color: "var(--fg-primary)", fontSize: 13,
        fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
      }}>{value}</span>
    </div>
  );
}

function Hr() {
  return <div style={{ height: 1, background: "var(--border-subtle)" }} />;
}

window.GameScreen = GameScreen;
window.DorjeMark = DorjeMark;
window.Table = Table;
