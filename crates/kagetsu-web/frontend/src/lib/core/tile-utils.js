// Tile parsing + SVG path resolution + sizing tables.
// Paths use absolute `/tiles/...` because vite serves public/ at /.

import { PIG } from './pigments.js';

export const NUMERALS = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
export const HONORS = ['', '東', '南', '西', '北', '白', '發', '中'];

export function parseTile(t) {
  if (!t || t === '?') return { back: true };
  const suit = t[0];
  const red = t.endsWith('r');
  const rank = parseInt(red ? t.slice(1, -1) : t.slice(1), 10);
  return { suit, rank, red };
}

export function tileFace(t) {
  const p = parseTile(t);
  if (p.back) return { back: true };
  if (p.suit === 'z') return { numeral: HONORS[p.rank], suit: '', honor: true, rank: p.rank };
  return {
    numeral: NUMERALS[p.rank],
    suit: p.suit === 'm' ? '萬' : p.suit === 'p' ? '筒' : '索',
    rank: p.rank,
    red: p.red,
    suitKey: p.suit,
  };
}

export function tileTextColor(t) {
  const p = parseTile(t);
  if (p.back) return 'var(--fg-primary)';
  if (p.red) return PIG.mtshal;
  if (p.suit === 'm') return '#1B1B2A';
  if (p.suit === 'p') return PIG.ngonpo;
  if (p.suit === 's') return PIG.spangma;
  if (p.rank === 7) return PIG.mtshal;
  if (p.rank === 6) return PIG.ljangkhu;
  if (p.rank === 5) return PIG.ngonpo;
  return '#1B1B2A';
}

export const TILE_SIZES = {
  xs: { w: 24, h: 32, radius: 3, edge: 2 },
  sm: { w: 30, h: 40, radius: 4, edge: 2.5 },
  md: { w: 42, h: 56, radius: 5, edge: 3 },
  lg: { w: 60, h: 80, radius: 6, edge: 4 },
};

export function tileSvgPath(t) {
  if (!t || t === '?') return '/tiles/Back.svg';
  if (t === '  ' || t === 'blank') return '/tiles/Blank.svg';
  const suit = t[0];
  const red = t.endsWith('r');
  const rank = parseInt(red ? t.slice(1, -1) : t.slice(1), 10);
  if (suit === 'm') return red && rank === 5 ? '/tiles/Man5-Dora.svg' : `/tiles/Man${rank}.svg`;
  if (suit === 'p') return red && rank === 5 ? '/tiles/Pin5-Dora.svg' : `/tiles/Pin${rank}.svg`;
  if (suit === 's') return red && rank === 5 ? '/tiles/Sou5-Dora.svg' : `/tiles/Sou${rank}.svg`;
  if (suit === 'z') {
    const names = ['', 'Ton', 'Nan', 'Shaa', 'Pei', 'Haku', 'Hatsu', 'Chun'];
    return `/tiles/${names[rank]}.svg`;
  }
  return '/tiles/Back.svg';
}
