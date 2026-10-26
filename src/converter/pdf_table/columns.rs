//! 列结构：把候选区块切成若干列。
//!
//! # 列是什么
//!
//! 列不是「间隙够大的地方」，而是**一条没有任何词跨越的垂直空白带**：区块内所有
//! 词都完整落在某一列内，两个相邻列之间存在宽度 ≥ [`Profile::column_separation_em`]
//! 倍字号、且无词跨越的空白。这个定义没有需要试出来的 point 常量，也不依赖
//! 「行内间隙多大算列」，对无边框表格、密集数字矩阵、右对齐数字列同样成立。
//!
//! 空白带两侧的判定用「区间合并」完成：把区块内所有词的 x 区间按容差合并成占用段，
//! 相邻占用段之间的空隙就是候选列间距；再看是否有词跨越它（有则说明不是列界，
//! 而是某个跨列单元格），无则成列。
//!
//! # 为什么不做「列锚点聚类」
//!
//! 锚点聚类需要「锚点容差」这个参数，且右对齐列（同一列 x0 漂移 15pt 以上）与左
//! 对齐列要用不同锚点；空白带法直接落在「没有词跨越」这个可验证的事实上，不需要
//! 选择用哪条边。

use super::lines::Row;
use super::profile::Profile;

/// 一列的水平范围。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Column {
    pub x0: f32,
    pub x1: f32,
}

impl Column {
    /// 区间是否包含给定 x（含端点）。
    pub fn contains(&self, x: f32) -> bool {
        x >= self.x0 && x <= self.x1
    }
}

/// 检出区块的列结构；不足两列或存在空列时返回 `None`（沿用基线文本）。
///
/// 列界 = 一条**没有任何非跨列词跨越**的垂直空白带，宽度 ≥
/// [`Profile::column_separation_em`] 倍字号。
///
/// 「跨列行」需要排除在外：横跨整表的标签行、合并的父级表头会在每一列之间都放
/// 一个词，若让它参与塑形，整张表会被判成「无列界」。判据是行宽占区块宽度的
/// 比例（≥ [`Profile::spanning_row`] 即视为跨列行）——这仍然是结构比例，不是
/// 点数。
pub(crate) fn detect(rows: &[Row], em: f32, profile: &Profile) -> Option<Vec<Column>> {
    if rows.len() < profile.min_block_rows {
        return None;
    }
    // 塑形只用多词行：单行只有一个词的「跨列标签」（如「总收益率互换」）会把
    // 相邻两列连成一片；多词行才携带列间结构。
    let shaping: Vec<&Row> = rows.iter().filter(|row| row.words.len() >= 2).collect();
    let shaping: Vec<&Row> = if shaping.len() >= profile.min_column_rows {
        shaping
    } else {
        rows.iter().collect()
    };

    let tolerance = profile.interval_tolerance_em * em;
    let mut intervals: Vec<(f32, f32)> = shaping
        .iter()
        .flat_map(|row| row.words.iter().map(|w| (w.x0, w.x1)))
        .collect();
    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut occupied: Vec<(f32, f32)> = Vec::new();
    for (x0, x1) in intervals {
        match occupied.last_mut() {
            Some(last) if x0 <= last.1 + tolerance => last.1 = last.1.max(x1),
            _ => occupied.push((x0, x1)),
        }
    }
    if occupied.len() < 2 {
        return None;
    }

    let min_gap = profile.column_separation_em * em;
    let mut columns: Vec<Column> = Vec::new();
    let mut left = occupied[0].0;
    for pair in occupied.windows(2) {
        let (gutter_x0, gutter_x1) = (pair[0].1, pair[1].0);
        if gutter_x1 - gutter_x0 < min_gap {
            continue;
        }
        // 少数行跨越（跨列标签、单元格长文本换行）不否决列界：按「多数行留空」投票。
        if support_rate(rows, gutter_x0, gutter_x1) < profile.column_support {
            continue;
        }
        columns.push(Column {
            x0: left,
            x1: gutter_x0,
        });
        left = gutter_x1;
    }
    columns.push(Column {
        x0: left,
        x1: occupied.last()?.1,
    });
    if columns.len() < 2 {
        return None;
    }
    for column in &columns {
        let populated = rows
            .iter()
            .filter(|row| row.words.iter().any(|w| column.contains(w.center_x())))
            .count();
        if populated < profile.min_column_rows {
            return None;
        }
    }
    let columns = split_merged_columns(rows, columns, em, profile);
    if columns.len() < 2 {
        return None;
    }
    Some(columns)
}

/// 该空白带的支持率：没有词跨越它的行所占比例。
///
/// 一票否决太脆：跨列标签行、单元格长文本换行都会在个别行跨越列界。支持率低于
/// [`Profile::column_support`] 说明多数行都有词落在带内——那不是列界，而是同一列
/// 的内部空隙。
fn support_rate(rows: &[Row], gutter_x0: f32, gutter_x1: f32) -> f32 {
    if rows.is_empty() {
        return 0.0;
    }
    let supported = rows
        .iter()
        .filter(|row| {
            !row.words
                .iter()
                .any(|w| w.x0 < gutter_x1 && w.x1 > gutter_x0)
        })
        .count();
    supported as f32 / rows.len() as f32
}

/// 按「行首 x 的两簇分布」把被并进来的列再切开。
///
/// 判据全是结构性的：某个区间内若存在两簇行首（每簇至少 [`Profile::min_column_rows`]
/// 行、簇间空隙 ≥ [`Profile::column_separation_em`] 倍字号），说明这是两列被并成了
/// 一个区间，按簇间中点切开。正常的单列不会有两簇——同一列的格首方向一致
/// （左对齐对齐左缘、右对齐对齐右缘，右对齐列的左缘虽然参差，但不会出现
/// 「中间空一大段」的双簇）。
fn split_merged_columns(
    rows: &[Row],
    columns: Vec<Column>,
    em: f32,
    profile: &Profile,
) -> Vec<Column> {
    let mut out: Vec<Column> = Vec::with_capacity(columns.len());
    for column in columns {
        let mut starts: Vec<f32> = Vec::new();
        for row in rows {
            let mut words: Vec<&super::word::PlacedWord> = row
                .words
                .iter()
                .filter(|w| column.contains(w.center_x()))
                .collect();
            words.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(first) = words.first() {
                starts.push(first.x0);
            }
        }
        starts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut split: Option<f32> = None;
        let min_gap = profile.column_separation_em * em;
        for pair in starts.windows(2) {
            let (left_count, right_count) = (
                starts.iter().filter(|x| **x <= pair[0]).count(),
                starts.iter().filter(|x| **x >= pair[1]).count(),
            );
            if pair[1] - pair[0] >= min_gap
                && left_count >= profile.min_column_rows
                && right_count >= profile.min_column_rows
            {
                split = Some((pair[0] + pair[1]) / 2.0);
                break;
            }
        }
        match split {
            Some(boundary) => {
                out.push(Column {
                    x0: column.x0,
                    x1: boundary,
                });
                out.push(Column {
                    x0: boundary,
                    x1: column.x1,
                });
            }
            None => out.push(column),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::pdf_table::word::PlacedWord;

    fn word(text: &str, x0: f32, x1: f32) -> PlacedWord {
        PlacedWord {
            text: text.to_string(),
            x0,
            y0: 600.0,
            x1,
            y1: 610.0,
            font_size: 10.0,
            sequence: 0,
        }
    }

    fn row(words: Vec<PlacedWord>) -> Row {
        let y0 = words.iter().map(|w| w.y0).fold(f32::INFINITY, f32::min);
        let y1 = words.iter().map(|w| w.y1).fold(f32::NEG_INFINITY, f32::max);
        Row { words, y0, y1 }
    }

    /// 三列结构：列界由无词跨越的空白带给出，与右对齐数字的 x0 漂移无关。
    #[test]
    fn detects_columns_by_uncrossed_gutters() {
        let profile = Profile::default();
        let rows = vec![
            row(vec![
                word("区域", 173.0, 195.0),
                word("数量", 271.0, 324.0),
                word("倍数", 390.0, 432.0),
            ]),
            row(vec![
                word("绿区", 174.0, 196.0),
                word("40-59", 274.0, 300.0),
                word("无", 391.0, 431.0),
            ]),
            row(vec![
                word("红区", 174.0, 196.0),
                word("200", 246.0, 320.0),
                word("3.0", 406.0, 420.0),
            ]),
        ];
        let columns = detect(&rows, 10.0, &profile).expect("应检出三列");
        assert_eq!(columns.len(), 3);
        assert!(columns[0].contains(180.0));
        assert!(columns[1].contains(280.0) && columns[1].contains(250.0));
        assert!(columns[2].contains(410.0));
    }

    /// 少数行跨越空白带（跨列标签行）不影响列界：按支持率投票，不再一票否决。
    #[test]
    fn a_few_crossing_rows_do_not_kill_the_gutter() {
        let profile = Profile::default();
        let rows = vec![
            row(vec![word("甲", 100.0, 110.0), word("乙", 300.0, 310.0)]),
            row(vec![word("跨列标题横跨两列", 100.0, 310.0)]),
            row(vec![word("丙", 100.0, 110.0), word("丁", 300.0, 310.0)]),
            row(vec![word("戊", 100.0, 110.0), word("己", 300.0, 310.0)]),
            row(vec![word("庚", 100.0, 110.0), word("辛", 300.0, 310.0)]),
        ];
        let columns = detect(&rows, 10.0, &profile).expect("多数行支持即可成列");
        assert_eq!(columns.len(), 2);
    }

    /// 多数行都被词跨越时，该空白带不是列界。
    #[test]
    fn majority_crossing_rows_reject_the_gutter() {
        let profile = Profile::default();
        let rows = vec![
            row(vec![word("甲跨两列的文字", 100.0, 310.0)]),
            row(vec![word("乙跨两列的文字", 100.0, 310.0)]),
            row(vec![word("丙", 100.0, 110.0), word("丁", 300.0, 310.0)]),
        ];
        assert!(detect(&rows, 10.0, &profile).is_none());
    }

    /// 只有一列（无列间空白）时不构成表格。
    #[test]
    fn single_column_is_not_a_table() {
        let profile = Profile::default();
        let rows = vec![
            row(vec![
                word("正文一行", 100.0, 200.0),
                word("继续", 204.0, 240.0),
            ]),
            row(vec![
                word("正文二行", 100.0, 200.0),
                word("继续", 204.0, 240.0),
            ]),
        ];
        assert!(detect(&rows, 10.0, &profile).is_none());
    }

    /// 只有一行有内容的列视为幽灵列，整块降级。
    #[test]
    fn column_needs_at_least_two_rows() {
        let profile = Profile::default();
        let rows = vec![
            row(vec![word("甲", 100.0, 110.0), word("乙", 300.0, 310.0)]),
            row(vec![word("丙", 100.0, 110.0)]),
            row(vec![word("丁", 100.0, 110.0)]),
        ];
        assert!(detect(&rows, 10.0, &profile).is_none());
    }
}
