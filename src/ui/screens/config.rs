//! 配置屏幕: 编辑 GameRules 全部字段 + 思考时长 + 种子.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::engine::rules::{GameRules, LengthRule, MultiRonRule};
use crate::ui::Transition;

/// 庄 seed 的选择. `Fixed(n)` 用于复盘.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedChoice {
    Random,
    Fixed(u64),
}

const FIXED_SEEDS: &[u64] = &[0xC0FFEE, 0xDEADBEEF, 0xFEEDFACE];

const UMA_PRESETS: &[[i32; 4]] = &[
    [15, 5, -5, -15],
    [10, 5, -5, -10],
    [20, 10, -10, -20],
    [30, 10, -10, -30],
];

const THINKING_PRESETS: &[Option<u32>] = &[Some(10), Some(20), Some(30), Some(60), None];
const CALL_WINDOW_PRESETS: &[u8] = &[3, 5, 8];

#[derive(Debug, Clone)]
pub struct ConfigState {
    pub config: GameRules,
    pub seed_choice: SeedChoice,
    pub selected: usize,
}

impl ConfigState {
    pub fn from(config: &GameRules, seed_choice: &SeedChoice) -> Self {
        Self {
            config: config.clone(),
            seed_choice: *seed_choice,
            selected: 0,
        }
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        let row_count = ROWS.len();
        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                None
            }
            KeyCode::Down => {
                if self.selected + 1 < row_count {
                    self.selected += 1;
                }
                None
            }
            KeyCode::Left => {
                self.adjust(self.selected, -1);
                None
            }
            KeyCode::Right => {
                self.adjust(self.selected, 1);
                None
            }
            KeyCode::Enter => Some(Transition::StartGame),
            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 游戏配置 ")
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            "↑↓ 选行 · ←→ 切值 · Enter 开始",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        for (i, row) in ROWS.iter().enumerate() {
            let label = row.label;
            let value = (row.format)(self);
            let is_sel = i == self.selected;
            let prefix = if is_sel { "▶ " } else { "  " };
            let style_name = if is_sel {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let style_value = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::Cyan)
            };
            // 古役细分项 master 关时灰一下
            let style_value = if !self.config.kotekisai && is_kotekisai_sub(i) && !is_sel {
                Style::default().fg(Color::DarkGray)
            } else {
                style_value
            };

            lines.push(Line::from(vec![
                Span::raw(prefix),
                Span::styled(format!("{:<22}", label), style_name),
                Span::styled(format!("  {}", value), style_value),
            ]));
        }

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
    }

    fn adjust(&mut self, idx: usize, dir: i32) {
        if let Some(row) = ROWS.get(idx) {
            (row.adjust)(self, dir);
        }
    }
}

/// 把当前 SeedChoice 解析成具体 u64.
pub fn resolve_seed(choice: SeedChoice) -> u64 {
    match choice {
        SeedChoice::Random => rand::random::<u64>(),
        SeedChoice::Fixed(n) => n,
    }
}

// ============== 字段表 ==============

struct Row {
    label: &'static str,
    format: fn(&ConfigState) -> String,
    adjust: fn(&mut ConfigState, i32),
}

fn is_kotekisai_sub(idx: usize) -> bool {
    (11..=17).contains(&idx)
}

fn cycle_idx(current: usize, len: usize, dir: i32) -> usize {
    let len_i = len as i32;
    ((current as i32 + dir).rem_euclid(len_i)) as usize
}

fn toggle(b: &mut bool, _dir: i32) {
    *b = !*b;
}

fn step(v: &mut i32, dir: i32, lo: i32, hi: i32, step: i32) {
    let new = (*v + dir * step).clamp(lo, hi);
    *v = new;
}

const ROWS: &[Row] = &[
    // 0
    Row {
        label: "赛制长度",
        format: |s| match s.config.length {
            LengthRule::Hanchan => "半庄战 (8 局)".into(),
            LengthRule::Tonpuusen => "东风战 (4 局)".into(),
        },
        adjust: |s, _| {
            s.config.length = match s.config.length {
                LengthRule::Hanchan => LengthRule::Tonpuusen,
                LengthRule::Tonpuusen => LengthRule::Hanchan,
            };
        },
    },
    // 1
    Row {
        label: "食断 (Kuitan)",
        format: |s| bool_str(s.config.kuitan),
        adjust: |s, d| toggle(&mut s.config.kuitan, d),
    },
    // 2
    Row {
        label: "赤宝牌",
        format: |s| bool_str(s.config.aka_dora),
        adjust: |s, d| toggle(&mut s.config.aka_dora, d),
    },
    // 3
    Row {
        label: "一发",
        format: |s| bool_str(s.config.ippatsu),
        adjust: |s, d| toggle(&mut s.config.ippatsu, d),
    },
    // 4
    Row {
        label: "里宝牌",
        format: |s| bool_str(s.config.ura_dora),
        adjust: |s, d| toggle(&mut s.config.ura_dora, d),
    },
    // 5
    Row {
        label: "数役满",
        format: |s| bool_str(s.config.kazoe_yakuman),
        adjust: |s, d| toggle(&mut s.config.kazoe_yakuman, d),
    },
    // 6
    Row {
        label: "双倍役满",
        format: |s| bool_str(s.config.double_yakuman),
        adjust: |s, d| toggle(&mut s.config.double_yakuman, d),
    },
    // 7
    Row {
        label: "多家荣和",
        format: |s| match s.config.multi_ron {
            MultiRonRule::Atamahane => "头跳".into(),
            MultiRonRule::DoubleRon => "双家荣和".into(),
            MultiRonRule::TripleRon => "三家荣和".into(),
        },
        adjust: |s, d| {
            let opts = [
                MultiRonRule::Atamahane,
                MultiRonRule::DoubleRon,
                MultiRonRule::TripleRon,
            ];
            let cur = opts
                .iter()
                .position(|x| *x == s.config.multi_ron)
                .unwrap_or(0);
            s.config.multi_ron = opts[cycle_idx(cur, opts.len(), d)];
        },
    },
    // 8
    Row {
        label: "西入",
        format: |s| bool_str(s.config.west_round),
        adjust: |s, d| toggle(&mut s.config.west_round, d),
    },
    // 9
    Row {
        label: "击飞 (箱下)",
        format: |s| bool_str(s.config.minus_score_end),
        adjust: |s, d| toggle(&mut s.config.minus_score_end, d),
    },
    // 10
    Row {
        label: "古役 (master)",
        format: |s| bool_str(s.config.kotekisai),
        adjust: |s, d| toggle(&mut s.config.kotekisai, d),
    },
    // 11
    Row {
        label: "  人和",
        format: |s| bool_str(s.config.kotekisai_renhou),
        adjust: |s, d| toggle(&mut s.config.kotekisai_renhou, d),
    },
    // 12
    Row {
        label: "  三连刻",
        format: |s| bool_str(s.config.kotekisai_sanrenkou),
        adjust: |s, d| toggle(&mut s.config.kotekisai_sanrenkou, d),
    },
    // 13
    Row {
        label: "  四连刻",
        format: |s| bool_str(s.config.kotekisai_surenkou),
        adjust: |s, d| toggle(&mut s.config.kotekisai_surenkou, d),
    },
    // 14
    Row {
        label: "  大车轮",
        format: |s| bool_str(s.config.kotekisai_daisharin),
        adjust: |s, d| toggle(&mut s.config.kotekisai_daisharin, d),
    },
    // 15
    Row {
        label: "  大七星",
        format: |s| bool_str(s.config.kotekisai_daichisei),
        adjust: |s, d| toggle(&mut s.config.kotekisai_daichisei, d),
    },
    // 16
    Row {
        label: "  八连庄",
        format: |s| bool_str(s.config.kotekisai_parenchan),
        adjust: |s, d| toggle(&mut s.config.kotekisai_parenchan, d),
    },
    // 17
    Row {
        label: "  十三不塔",
        format: |s| bool_str(s.config.kotekisai_shisanputaa),
        adjust: |s, d| toggle(&mut s.config.kotekisai_shisanputaa, d),
    },
    // 18
    Row {
        label: "起始点棒",
        format: |s| format!("{}", s.config.starting_score),
        adjust: |s, d| step(&mut s.config.starting_score, d, 20_000, 50_000, 1_000),
    },
    // 19
    Row {
        label: "目标点棒",
        format: |s| format!("{}", s.config.target_score),
        adjust: |s, d| step(&mut s.config.target_score, d, 20_000, 50_000, 1_000),
    },
    // 20
    Row {
        label: "Uma (顺位奖罚)",
        format: |s| {
            let u = s.config.uma;
            format!("[{}, {}, {}, {}]", u[0], u[1], u[2], u[3])
        },
        adjust: |s, d| {
            let cur = UMA_PRESETS
                .iter()
                .position(|x| *x == s.config.uma)
                .unwrap_or(0);
            s.config.uma = UMA_PRESETS[cycle_idx(cur, UMA_PRESETS.len(), d)];
        },
    },
    // 21
    Row {
        label: "思考时长",
        format: |s| match s.config.thinking_time_secs {
            Some(t) => format!("{} 秒", t),
            None => "不限时".into(),
        },
        adjust: |s, d| {
            let cur = THINKING_PRESETS
                .iter()
                .position(|x| *x == s.config.thinking_time_secs)
                .unwrap_or(0);
            s.config.thinking_time_secs =
                THINKING_PRESETS[cycle_idx(cur, THINKING_PRESETS.len(), d)];
        },
    },
    // 22
    Row {
        label: "鸣牌窗口",
        format: |s| format!("{} 秒", s.config.call_window_secs),
        adjust: |s, d| {
            let cur = CALL_WINDOW_PRESETS
                .iter()
                .position(|x| *x == s.config.call_window_secs)
                .unwrap_or(1);
            s.config.call_window_secs =
                CALL_WINDOW_PRESETS[cycle_idx(cur, CALL_WINDOW_PRESETS.len(), d)];
        },
    },
    // 23
    Row {
        label: "种子",
        format: |s| match s.seed_choice {
            SeedChoice::Random => "随机".into(),
            SeedChoice::Fixed(n) => format!("固定 0x{:X}", n),
        },
        adjust: |s, d| {
            // 0 = Random, 1..=N = FIXED_SEEDS[i-1]
            let n = FIXED_SEEDS.len() + 1;
            let cur = match s.seed_choice {
                SeedChoice::Random => 0,
                SeedChoice::Fixed(v) => FIXED_SEEDS
                    .iter()
                    .position(|x| *x == v)
                    .map(|i| i + 1)
                    .unwrap_or(0),
            };
            let next = cycle_idx(cur, n, d);
            s.seed_choice = if next == 0 {
                SeedChoice::Random
            } else {
                SeedChoice::Fixed(FIXED_SEEDS[next - 1])
            };
        },
    },
];

fn bool_str(b: bool) -> String {
    (if b { "开" } else { "关" }).into()
}
