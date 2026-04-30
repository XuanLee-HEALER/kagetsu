# 和牌型与拆解算法

## 1. 三种和牌结构

任意完整 14 张手牌(含鸣牌)和成下列之一即可:

### 标准型 (4 + 1)
- 4 个**面子** (mentsu): 顺子(shuntsu) / 刻子(koutsu) / 杠子(kantsu)
- 1 个**雀头** (jantou): 一对
- 杠子虽然是 4 张,但占一个面子位

### 七对子型 (Chiitoitsu)
- 7 组**不同的**对子(同种对不可重复)。
- 不与对对和、二杯口复合(规则禁止)。

### 国士无双型 (Kokushi Musou)
- 13 种幺九字牌各一张 + 任一种幺九字牌再一张作雀头。

## 2. 拆解算法

输入:34 维计数数组 `hand[34]`(忽略花色名,索引 0-8 = 1m-9m,9-17 = 1p-9p,18-26 = 1s-9s,27-30 = 东南西北,31-33 = 中發白)。
输出:所有可能的 (面子集 + 雀头) 拆解。

### 算法概要

```
fn decompose(hand: &mut [u8; 34], melds_open: &[Meld]) -> Vec<Decomposition>
    // 优先尝试七对子和国士
    if let Some(d) = try_chiitoitsu(hand) { results.push(d) }
    if let Some(d) = try_kokushi(hand) { results.push(d) }

    // 标准型: 枚举雀头位置, 递归拆面子
    for i in 0..34 {
        if hand[i] >= 2 {
            hand[i] -= 2
            let mentsu_sets = enumerate_mentsu(hand, melds_open)
            for set in mentsu_sets {
                results.push(Decomposition { pair: i, mentsu: set })
            }
            hand[i] += 2
        }
    }

    results
```

### 枚举面子(回溯)

```
fn enumerate_mentsu(hand: &mut [u8; 34], existing: &[Meld]) -> Vec<Vec<Mentsu>>
    let needed = 4 - existing.len()
    let mut results = vec![]
    backtrack(hand, &mut existing.to_vec(), needed, &mut results)
    return results

fn backtrack(hand: &mut [u8; 34], chosen: &mut Vec<Mentsu>, need: usize, out: &mut Vec<Vec<Mentsu>>)
    if need == 0 {
        if hand.iter().all(|&c| c == 0) {
            out.push(chosen.clone())
        }
        return
    }
    let i = first_nonzero_index(hand)
    if i.is_none() return
    let i = i.unwrap()

    // 尝试刻子
    if hand[i] >= 3 {
        hand[i] -= 3; chosen.push(Koutsu(i))
        backtrack(hand, chosen, need-1, out)
        chosen.pop(); hand[i] += 3
    }

    // 尝试顺子(仅数牌, i 不为字牌, i % 9 ≤ 6)
    if is_suupai(i) && i % 9 <= 6 && hand[i] >= 1 && hand[i+1] >= 1 && hand[i+2] >= 1 {
        hand[i] -= 1; hand[i+1] -= 1; hand[i+2] -= 1
        chosen.push(Shuntsu(i))
        backtrack(hand, chosen, need-1, out)
        chosen.pop(); hand[i] += 1; hand[i+1] += 1; hand[i+2] += 1
    }
```

### 多解处理

某些手牌可能有多种拆法(如 23344m 可拆 234m+34m 雀头 或 33m雀头+44m搭子...)。
**和牌时** (即多于 1 解时),应:
1. 计算每种拆解下的 **番数 + 符**。
2. 选**总分(基本点)最大**的那个作为最终结果。
3. 番数相同时选符高的;再相同任选一个(用户偏好可作为 tiebreaker)。

## 3. 听牌检测

```
fn tenpai_tiles(hand: &[u8; 34], melds: &[Meld]) -> Vec<TileIndex>
    let mut waits = vec![]
    for i in 0..34 {
        if hand[i] >= 4 { continue }  // 全部 4 张已用,不可能等
        let mut h = hand.clone()
        h[i] += 1
        if can_win(&h, melds) {
            waits.push(i)
        }
    }
    waits
```

`can_win` 即 `decompose` 返回非空。

## 4. 待牌类型识别

判定符所需(单骑/嵌张/边张/两面/双碰):

| 类型 | 判定 |
|---|---|
| 单骑 (Tanki) | 和牌张作雀头 |
| 嵌张 (Kanchan) | 和牌张为顺子中间(如等 5 完成 4-5-6) |
| 边张 (Penchan) | 和牌张为 3 完成 1-2-3,或为 7 完成 7-8-9 |
| 两面 (Ryanmen) | 和牌张为顺子两端,且非边张 |
| 双碰 (Shanpon) | 和牌张完成两对中的一刻 |

某种和牌可能多种类型成立(如对碰+嵌张),按符高的算。

## 5. 国士无双 13 面听 / 九莲 9 面听

- 国士 13 面:13 种幺九字牌每种 ≥ 1,且总和正好 13(再摸任一种幺九字牌即和)。
- 纯九莲 9 面:同花色 1112345678999,等同花色任一张数牌。

## 6. 实现选择

- 用 `[u8; 34]` 表示手牌(性能、对称性好)。
- 但由于副露的暗刻 vs 明刻、暗杠 vs 明杠对符计算有影响,**面子结构需要保留来源信息** (`Meld { tiles, source_seat, kind }`)。
- 拆解结果用 `Decomposition { pair: TileIndex, mentsu: Vec<Mentsu> }`,每个 `Mentsu` 标注 open/closed。
- 拆解后的"是否暗刻"判定:荣和时,**和牌张所组成的刻子按明刻算**(因为和牌张是从他家拿的);自摸则按暗刻。这影响四暗刻是否成立 → 四暗刻必须自摸;若是荣和则只是三暗刻 + 对对和。
