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
pub(crate) fn detect(rows: &[Row], em: f32, profile: &Profile) -> Option<Vec<Column>> {
    if rows.len() < profile.min_block_rows {
        return None;
    }
    let tolerance = profile.interval_tolerance_em * em;
    let mut intervals: Vec<(f32, f32)> = rows
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
        if gutter_x1 - gutter_x0 < min_gap || has_crossing_word(rows, gutter_x0, gutter_x1) {
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
        let populated: Vec<&Row> = rows
            .iter()
            .filter(|row| row.words.iter().any(|w| column.contains(w.center_x())))
            .collect();
        if populated.len() < profile.min_column_rows {
            return None;
        }
    }
    Some(columns)
}

/// 是否有词跨越给定空白带（严格跨越：完全落在带内或刚好贴边都不算）。
fn has_crossing_word(rows: &[Row], gutter_x0: f32, gutter_x1: f32) -> bool {
    rows.iter().any(|row| {
        row.words
            .iter()
            .any(|w| w.x0 < gutter_x1 && w.x1 > gutter_x0)
    })
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

    /// 有词跨越空白带时该空白带不能成为列界（跨列单元格）。
    #[test]
    fn crossing_word_blocks_the_gutter() {
        let profile = Profile::default();
        let rows = vec![
            row(vec![word("甲", 100.0, 110.0), word("乙", 300.0, 310.0)]),
            row(vec![word("跨列标题横跨两列", 100.0, 310.0)]),
            row(vec![word("丙", 100.0, 110.0), word("丁", 300.0, 310.0)]),
        ];
        assert!(detect(&rows, 10.0, &profile).is_none(), "跨列词使列不成立");
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
