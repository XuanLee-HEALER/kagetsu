// Sample game state — copied verbatim from prototype core.jsx (GAME).
// Used by GameScreen / ZeroTrustGameScreen / etc. so the prototype shows
// realistic data without a backend.
export const GAME = {
  round: { wind: '東', num: 2, honba: 1, riichi: 1 },
  wall: 47,
  junme: 8,
  dora: ['m5'],
  uradora: ['?'],
  players: [
    {
      seat: '東',
      name: '你',
      score: 26500,
      dealer: true,
      hand: ['m1', 'm2', 'm3', 'p3', 'p4', 'p5r', 'p6', 's2', 's3', 's4', 'z1', 'z1', 'z5'],
      draw: 'p7',
      discards: ['z3', 's9', 'p1', 'm9', 'z4', 's7', 'p8', 's8', 'm8', 's5', 'p2', 'z6', 'm7'],
      melds: [],
      tenpai: ['p3', 'p6'],
      shanten: 0,
    },
    {
      seat: '南',
      name: '下家',
      score: 24000,
      hand: Array(13).fill('?'),
      discards: ['z2', 'm1', 'p9', 's1', 's8', 'm7', 'z6', 'p3', 'm4', 's6', 'z5', 'p7', 'z4'],
      melds: [{ type: 'pon', tiles: ['z3', 'z3', 'z3'], from: '上' }],
    },
    {
      seat: '西',
      name: '对家',
      score: 24500,
      riichi: true,
      hand: Array(13).fill('?'),
      discards: ['m9', 'z1', 's9', 'p9', 'p8', 'm1', 's2', 'm6', 'p4', 'z7', 's3', 'p5'],
      riichiAt: 5,
      melds: [],
    },
    {
      seat: '北',
      name: '上家',
      score: 25000,
      hand: Array(13).fill('?'),
      discards: ['s5', 'p2', 'z7', 'm3', 's4', 'p6', 'm5', 'z2', 'p1', 's7', 'm4'],
      melds: [{ type: 'chi', tiles: ['s5', 's6', 's7'], from: '对' }],
    },
  ],
  danger: ['p5', 'p8'],
  log: [
    { junme: 7, who: '对家', action: '立直 · 切', tile: 'p5' },
    { junme: 7, who: '上家', action: '碰', tile: 'z3' },
    { junme: 8, who: '下家', action: '打', tile: 'z4' },
    { junme: 8, who: '你', action: '摸', tile: 'p7', emphasize: true },
  ],
};
