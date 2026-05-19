// tui-majo · web · core (SakyaHuman edition)
//
// Uses raw design-system tokens from colors_and_type.css. Single dark theme.
//
// Tile color mapping (mineral palette):
//   萬   #1B1B2A on Karpo cream  (kept dark — the most-common suit reads neutral)
//   筒   Ngonpo  · azurite blue
//   索   Spangma · malachite green  (README said purple; we substitute with the palette's green)
//   中   Mtshal  · cinnabar red
//   發   Ljangkhu· malachite green
//   白   Ngonpo  · azurite (haku = white but drawn blue in convention)
//   風   charcoal
//   red 5  Mtshal cinnabar number, normal black 五
//
// Components map cleanly to Svelte SFCs.

const PIG = {
  gser:     "#D2B450",
  ngonpo:   "#5B8AB8",
  ljangkhu: "#527559",
  spangma:  "#527559",
  karpo:    "#EFE2BE",
  serpo:    "#A09058",
  mtshal:   "#BB4441",
  marpo:    "#BB4441",
  likhri:   "#E85600",
  mutsmen:  "#2F3B5B",
  tsingkha: "#2B3560",
  ngangpa:  "#9F7045",
  nyima:    "#E0C888",
  metok:    "#C88090",
  dri_med:  "#C4BCAE",
  dul_ba:   "#9E978C",
  stripeRed:   "#BE3A34",
  stripeWhite: "#E8E0D4",
  stripeBlue:  "#2E4878",
};

// ── tile parsing & color ──────────────────────────────────────────
const NUMERALS = ["〇","一","二","三","四","五","六","七","八","九"];
const HONORS = ["", "東", "南", "西", "北", "白", "發", "中"];

function parseTile(t) {
  if (!t || t === "?") return { back: true };
  const suit = t[0];
  const red = t.endsWith("r");
  const rank = parseInt(red ? t.slice(1, -1) : t.slice(1), 10);
  return { suit, rank, red };
}

function tileFace(t) {
  const p = parseTile(t);
  if (p.back) return { back: true };
  if (p.suit === "z") return { numeral: HONORS[p.rank], suit: "", honor: true, rank: p.rank };
  return {
    numeral: NUMERALS[p.rank],
    suit: p.suit === "m" ? "萬" : p.suit === "p" ? "筒" : "索",
    rank: p.rank, red: p.red, suitKey: p.suit,
  };
}

function tileTextColor(t) {
  const p = parseTile(t);
  if (p.back) return "var(--fg-primary)";
  if (p.red) return PIG.mtshal;
  if (p.suit === "m") return "#1B1B2A";
  if (p.suit === "p") return PIG.ngonpo;
  if (p.suit === "s") return PIG.spangma;
  // honors
  if (p.rank === 7) return PIG.mtshal;      // 中 red
  if (p.rank === 6) return PIG.ljangkhu;    // 發 green
  if (p.rank === 5) return PIG.ngonpo;      // 白 blue
  return "#1B1B2A";                          // winds black
}

// ── Tile (uses FluffyStuff CC0 SVG set + CSS 3D border) ────────────
// Sizes target 3:4 aspect ratio (matches the SVG source).
const TILE_SIZES = {
  xs: { w: 24, h: 32, radius: 3, edge: 2 },
  sm: { w: 30, h: 40, radius: 4, edge: 2.5 },
  md: { w: 42, h: 56, radius: 5, edge: 3 },
  lg: { w: 60, h: 80, radius: 6, edge: 4 },
};

function tileSvgPath(t) {
  if (!t || t === "?") return "tiles/Back.svg";
  if (t === "  " || t === "blank") return "tiles/Blank.svg";
  const suit = t[0];
  const red = t.endsWith("r");
  const rank = parseInt(red ? t.slice(1, -1) : t.slice(1), 10);
  if (suit === "m") return red && rank === 5 ? "tiles/Man5-Dora.svg" : `tiles/Man${rank}.svg`;
  if (suit === "p") return red && rank === 5 ? "tiles/Pin5-Dora.svg" : `tiles/Pin${rank}.svg`;
  if (suit === "s") return red && rank === 5 ? "tiles/Sou5-Dora.svg" : `tiles/Sou${rank}.svg`;
  if (suit === "z") {
    const names = ["", "Ton", "Nan", "Shaa", "Pei", "Haku", "Hatsu", "Chun"];
    return `tiles/${names[rank]}.svg`;
  }
  return "tiles/Back.svg";
}

function Tile({ t, size = "md", state = "normal", rotate = 0, style = {} }) {
  const d = TILE_SIZES[size];
  const isBack = !t || t === "?";
  const src = tileSvgPath(t);

  let lift = 0;
  let outline = `1px solid rgba(27,27,42,0.40)`;
  let outlineW = 1;
  let glow = "";
  let faceTint = "";

  if (state === "selected") {
    lift = -7;
    outline = `2px solid ${PIG.gser}`;
    outlineW = 2;
    glow = `, 0 0 0 1px rgba(210,180,80,0.20)`;
  } else if (state === "draw") {
    outline = `2px solid ${PIG.gser}`;
    outlineW = 2;
    glow = `, 0 0 8px rgba(210,180,80,0.45)`;
  } else if (state === "riichi" || state === "danger") {
    outline = `2px solid ${PIG.mtshal}`;
    outlineW = 2;
  } else if (state === "discarded-recent") {
    outline = `1.5px dashed ${PIG.gser}`;
  } else if (state === "tenpai") {
    outline = `1.5px solid ${PIG.ljangkhu}`;
  }

  // 3D edge effect via box-shadow stack:
  //   inset highlight on top, dark depth on bottom/right
  const e = d.edge;
  const shadow = [
    `inset 0 1px 0 rgba(255,255,255,0.55)`,           // top highlight on face
    `inset -1px 0 0 rgba(0,0,0,0.10)`,                 // right inner shadow
    `inset 0 -1px 0 rgba(0,0,0,0.18)`,                 // bottom inner shadow
    `0 ${e}px 0 -0.5px #9F7045`,                       // Ngangpa wood lip
    `0 ${e + 1}px 0 0 rgba(0,0,0,0.20)`,               // base shadow
    `0 ${e + 3}px ${e * 1.5}px -1px rgba(0,0,0,0.40)`, // soft drop
  ];
  if (glow) shadow.push(glow.replace(/^, /, ""));
  if (isBack) {
    shadow[0] = `inset 0 1px 0 rgba(255,255,255,0.10)`;
    shadow[1] = `inset -1px 0 0 rgba(0,0,0,0.25)`;
    shadow[2] = `inset 0 -1px 0 rgba(0,0,0,0.35)`;
  }

  return (
    <div style={{
      width: d.w, height: d.h,
      borderRadius: d.radius,
      background: isBack ? "#2F3B5B" : "#F7F0DD",
      border: outline,
      boxShadow: shadow.join(", "),
      boxSizing: "border-box",
      flexShrink: 0,
      transform: `translateY(${lift}px) rotate(${rotate}deg)`,
      transformOrigin: "center center",
      transition: "transform 220ms cubic-bezier(0.16,1,0.30,1)",
      overflow: "hidden",
      position: "relative",
      ...style,
    }}>
      {isBack ? (
        <BackInner d={d} />
      ) : (
        <img src={src} alt=""
          draggable="false"
          style={{
            width: "100%", height: "100%",
            display: "block",
            objectFit: "contain",
            padding: 1,
            boxSizing: "border-box",
            pointerEvents: "none",
            userSelect: "none",
          }} />
      )}
    </div>
  );
}

// Back design — Mutsmen lapis ground + Sakya 3-stripe + gold Dorje hint
function BackInner({ d }) {
  return (
    <svg width="100%" height="100%" viewBox="0 0 30 40" preserveAspectRatio="xMidYMid meet"
      style={{ display: "block" }}>
      <defs>
        <linearGradient id="bk-grad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#3A4A78" />
          <stop offset="50%" stopColor="#2F3B5B" />
          <stop offset="100%" stopColor="#1F2A48" />
        </linearGradient>
      </defs>
      <rect x="0" y="0" width="30" height="40" fill="url(#bk-grad)" />
      {/* center vajra-ish glyph */}
      <g stroke="#D2B450" strokeWidth="0.6" fill="none" opacity="0.65">
        <circle cx="15" cy="20" r="1.4" fill="#D2B450" />
        <path d="M15 11 L13 16 L15 19 L17 16 Z" />
        <path d="M15 29 L13 24 L15 21 L17 24 Z" />
        <path d="M7 20 L11 18 L14 20 L11 22 Z" />
        <path d="M23 20 L19 18 L16 20 L19 22 Z" />
      </g>
      {/* corner dots for richer texture */}
      <g fill="#D2B450" opacity="0.35">
        <circle cx="4" cy="4" r="0.6" />
        <circle cx="26" cy="4" r="0.6" />
        <circle cx="4" cy="36" r="0.6" />
        <circle cx="26" cy="36" r="0.6" />
      </g>
    </svg>
  );
}

// ── Hand row of tiles ─────────────────────────────────────────────
function TileRow({ tiles, size = "md", gap = 4, selected = -1, drawIdx = -1, riichiAt = -1, style = {}, getState }) {
  return (
    <div style={{ display: "flex", gap, alignItems: "flex-end", ...style }}>
      {tiles.map((t, i) => {
        let state = "normal";
        if (getState) state = getState(t, i) || state;
        else if (i === selected) state = "selected";
        else if (i === drawIdx) state = "draw";
        else if (i === riichiAt) state = "riichi";
        return <Tile key={i} t={t} size={size} state={state} />;
      })}
    </div>
  );
}

// ── Discard pile (6 col grid) ─────────────────────────────────────
function DiscardPile({ tiles, riichiAt = -1, rotation = 0, style = {} }) {
  const cols = 6;
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: `repeat(${cols}, max-content)`,
      gap: 3,
      transform: `rotate(${rotation}deg)`,
      transformOrigin: "center center",
      ...style,
    }}>
      {tiles.map((t, i) => (
        <Tile key={i} t={t} size="xs"
          state={i === riichiAt ? "riichi" : "normal"}
          rotate={i === riichiAt ? 90 : 0} />
      ))}
    </div>
  );
}

// ── KeyBadge ─────────────────────────────────────────────────────
// Mono-typeface keycap, gold for primary action, ngonpo for ok, mtshal for danger
function KeyBadge({ k, label, tone = "default", disabled = false, size = "md" }) {
  const sizing = size === "sm"
    ? { kp: "1px 6px", kf: 11, lf: 11 }
    : { kp: "2px 8px", kf: 12, lf: 13 };
  const toneStyle = {
    default: { color: "var(--fg-primary)", border: "var(--border-default)", bg: "var(--bg-surface0)" },
    primary: { color: PIG.gser, border: "rgba(210,180,80,0.55)", bg: "rgba(210,180,80,0.10)" },
    danger:  { color: PIG.mtshal, border: PIG.mtshal, bg: "rgba(187,68,65,0.10)" },
    ok:      { color: PIG.ljangkhu, border: PIG.ljangkhu, bg: "rgba(82,117,89,0.12)" },
    info:    { color: PIG.ngonpo, border: PIG.ngonpo, bg: "rgba(91,138,184,0.12)" },
  }[tone];
  return (
    <span style={{
      display: "inline-flex", alignItems: "center", gap: 8,
      opacity: disabled ? 0.40 : 1, fontFamily: "var(--font-sans)",
    }}>
      <span style={{
        border: `1px solid ${toneStyle.border}`,
        background: toneStyle.bg, color: toneStyle.color,
        padding: sizing.kp,
        borderRadius: "var(--radius-sm)",
        fontWeight: 600, fontSize: sizing.kf,
        fontFamily: "var(--font-mono)",
        minWidth: 18, textAlign: "center", display: "inline-block",
      }}>
        {k}
      </span>
      {label ? (
        <span style={{
          color: disabled ? "var(--fg-disabled)" : "var(--fg-secondary)",
          fontSize: sizing.lf,
        }}>{label}</span>
      ) : null}
    </span>
  );
}

// ── Eyebrow ──────────────────────────────────────────────────────
function Eyebrow({ children, style = {} }) {
  return (
    <div style={{
      font: "var(--t-eyebrow)",
      letterSpacing: "var(--tracking-widest)",
      textTransform: "uppercase",
      color: "var(--fg-tertiary)",
      ...style,
    }}>{children}</div>
  );
}

// ── Card / Panel ─────────────────────────────────────────────────
function Card({ children, style = {}, raised = false, accent = false, padding }) {
  return (
    <div style={{
      background: accent ? "var(--accent-soft)" : "var(--bg-surface0)",
      border: `1px solid ${accent ? "var(--border-focus)" : "var(--border-default)"}`,
      borderRadius: "var(--radius-lg)",
      boxShadow: raised ? "var(--shadow-2)" : "var(--shadow-0)",
      padding: padding ?? "var(--space-5)",
      ...style,
    }}>{children}</div>
  );
}

// ── Bilingual label ──────────────────────────────────────────────
// e.g. "Riichi · 立直 · リーチ"
function Trilingual({ en, zh, ja, style = {} }) {
  return (
    <span style={{ color: "var(--fg-secondary)", fontSize: "var(--text-sm)", ...style }}>
      {en}
      {zh ? <span style={{ color: "var(--fg-tertiary)", margin: "0 8px" }}>·</span> : null}
      {zh}
      {ja ? <span style={{ color: "var(--fg-tertiary)", margin: "0 8px" }}>·</span> : null}
      {ja}
    </span>
  );
}

// ── Hr / Row utility ─────────────────────────────────────────────
function Hr({ style = {} }) {
  return <div style={{ height: 1, background: "var(--border-subtle)", ...style }} />;
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

// ── Sample game state ────────────────────────────────────────────
const GAME = {
  round: { wind: "東", num: 2, honba: 1, riichi: 1 },
  wall: 47, junme: 8,
  dora: ["m5"],
  uradora: ["?"],
  players: [
    { seat: "東", name: "你", score: 26500, dealer: true,
      hand: ["m1","m2","m3","p3","p4","p5r","p6","s2","s3","s4","z1","z1","z5"],
      draw: "p7",
      discards: ["z3","s9","p1","m9","z4","s7","p8","s8","m8","s5","p2","z6","m7"],
      melds: [],
      tenpai: ["p3","p6"], shanten: 0 },
    { seat: "南", name: "下家", score: 24000,
      hand: Array(13).fill("?"),
      discards: ["z2","m1","p9","s1","s8","m7","z6","p3","m4","s6","z5","p7","z4"],
      melds: [{ type: "pon", tiles: ["z3","z3","z3"], from: "上" }] },
    { seat: "西", name: "对家", score: 24500, riichi: true,
      hand: Array(13).fill("?"),
      discards: ["m9","z1","s9","p9","p8","m1","s2","m6","p4","z7","s3","p5"],
      riichiAt: 5,
      melds: [] },
    { seat: "北", name: "上家", score: 25000,
      hand: Array(13).fill("?"),
      discards: ["s5","p2","z7","m3","s4","p6","m5","z2","p1","s7","m4"],
      melds: [{ type: "chi", tiles: ["s5","s6","s7"], from: "对" }] },
  ],
  danger: ["p5","p8"],
  log: [
    { junme: 7, who: "对家", action: "立直 · 切", tile: "p5" },
    { junme: 7, who: "上家", action: "碰", tile: "z3" },
    { junme: 8, who: "下家", action: "打", tile: "z4" },
    { junme: 8, who: "你", action: "摸", tile: "p7", emphasize: true },
  ],
};

Object.assign(window, {
  PIG, Tile, TileRow, DiscardPile, KeyBadge, Eyebrow, Card, Trilingual,
  Hr, Row,
  parseTile, tileFace, tileTextColor, GAME,
});
