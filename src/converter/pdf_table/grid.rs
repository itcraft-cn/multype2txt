//! 网格构建与渲染：把区块的词装进「行 × 列」，按源版式对齐后输出纯文本。
//!
//! # 对齐方式是复制，不是重新决策
//!
//! 旧实现用字符白名单判断「数字列」（把「至/及/以/上/无」写进代码）再右对齐，
//! 这是拿内容猜版式。这里改为看源几何：同一列内所有格的**右边缘离散度**若小于
//! 左边缘离散度，说明原文档就是右对齐，照抄即可；反之左对齐。内容类型完全不
//! 参与判定，中文描述列、百分比列、区间列一视同仁。
//!
//! # 空单元格
//!
//! 跨行合并（rowspan）在纯文本里没有表达方式，下方行如实留空，不猜填。

use super::columns::Column;
use super::lines::Row;
use super::profile::Profile;
use super::word::{display_width, join_words, PlacedWord};

/// 网格：列结构与逐行单元格文本。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grid {
    pub columns: Vec<Column>,
    pub rows: Vec<GridRow>,
}

/// 网格的一行。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GridRow {
    /// 每列的单元格文本（已按词间空格规则拼接）。
    pub cells: Vec<String>,
    /// 每列非空内容的源 x 区间 `(x0, x1)`，用于复制对齐方式。
    pub extents: Vec<Option<(f32, f32)>>,
}

/// 把区块的词装入网格。
pub(crate) fn build(rows: &[Row], columns: Vec<Column>, em: f32, profile: &Profile) -> Grid {
    let mut grid_rows: Vec<GridRow> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut buckets: Vec<Vec<&PlacedWord>> = vec![Vec::new(); columns.len()];
        for word in &row.words {
            let index = column_index(&columns, word.center_x());
            buckets[index].push(word);
        }
        let mut cells: Vec<String> = Vec::with_capacity(columns.len());
        let mut extents: Vec<Option<(f32, f32)>> = Vec::with_capacity(columns.len());
        for bucket in &mut buckets {
            bucket.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
            cells.push(join_words(bucket, em, profile));
            extents.push(match (bucket.first(), bucket.last()) {
                (Some(first), Some(last)) => Some((first.x0, last.x1)),
                _ => None,
            });
        }
        grid_rows.push(GridRow { cells, extents });
    }
    Grid {
        columns,
        rows: grid_rows,
    }
}

/// 词中心落在哪一列；越界（浮点抖动）时取最近的列。
fn column_index(columns: &[Column], x: f32) -> usize {
    if let Some(index) = columns.iter().position(|c| c.contains(x)) {
        return index;
    }
    let mut best = 0;
    let mut best_distance = f32::INFINITY;
    for (index, column) in columns.iter().enumerate() {
        let distance = if x < column.x0 {
            column.x0 - x
        } else {
            x - column.x1
        };
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

/// 渲染为对齐的纯文本行。
///
/// 列宽取该列最宽单元格的显示宽度；列间固定两个空格；按复制来的对齐方式补空格。
pub(crate) fn render(grid: &Grid) -> Vec<String> {
    let align_right: Vec<bool> = (0..grid.columns.len())
        .map(|i| right_aligned(grid, i))
        .collect();
    let widths: Vec<usize> = (0..grid.columns.len())
        .map(|index| {
            grid.rows
                .iter()
                .map(|row| display_width(row.cells[index].trim()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    grid.rows
        .iter()
        .map(|row| {
            let mut line = String::new();
            for (index, cell) in row.cells.iter().enumerate() {
                if index > 0 {
                    line.push_str("  ");
                }
                let text = cell.trim();
                let pad = widths[index].saturating_sub(display_width(text));
                if align_right[index] {
                    line.push_str(&" ".repeat(pad));
                    line.push_str(text);
                } else {
                    line.push_str(text);
                    line.push_str(&" ".repeat(pad));
                }
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// 该列是否右对齐：右边缘比左边缘更稳定。
///
/// 至少要有两个非空单元格才能谈「稳定」，否则按左对齐（默认排版方向）。
fn right_aligned(grid: &Grid, index: usize) -> bool {
    let mut lefts: Vec<f32> = Vec::new();
    let mut rights: Vec<f32> = Vec::new();
    for row in &grid.rows {
        if let Some((x0, x1)) = row.extents[index] {
            lefts.push(x0);
            rights.push(x1);
        }
    }
    if lefts.len() < 2 {
        return false;
    }
    stdev(&rights) < stdev(&lefts)
}

/// 标准差（总体），用于比较边缘稳定性。
fn stdev(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let variance =
        values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / values.len() as f32;
    variance.sqrt()
}

/// 渲染一个非表格行：按词间空格规则拼接。
pub(crate) fn render_prose(row: &Row, em: f32, profile: &Profile) -> String {
    let words: Vec<&PlacedWord> = row.words.iter().collect();
    join_words(&words, em, profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[(&str, f32, f32)]) -> Row {
        let words: Vec<PlacedWord> = cells
            .iter()
            .map(|(text, x0, x1)| PlacedWord {
                text: text.to_string(),
                x0: *x0,
                y0: 600.0,
                x1: *x1,
                y1: 610.0,
                font_size: 10.0,
                sequence: 0,
            })
            .collect();
        Row {
            words,
            y0: 600.0,
            y1: 610.0,
        }
    }

    fn columns() -> Vec<Column> {
        vec![
            Column {
                x0: 170.0,
                x1: 200.0,
            },
            Column {
                x0: 240.0,
                x1: 330.0,
            },
            Column {
                x0: 380.0,
                x1: 440.0,
            },
        ]
    }

    /// 单元格按列归属，跨行合并的空格如实留空。
    #[test]
    fn cells_land_in_their_columns() {
        let profile = Profile::default();
        let rows = vec![
            row(&[
                ("区域", 173.0, 195.0),
                ("数量", 271.0, 324.0),
                ("倍数", 390.0, 432.0),
            ]),
            row(&[
                ("绿区", 174.0, 196.0),
                ("40-59", 273.0, 300.0),
                ("无", 391.0, 431.0),
            ]),
            row(&[("40-59", 273.0, 300.0), ("2.0", 400.0, 420.0)]),
        ];
        let grid = build(&rows, columns(), 10.0, &profile);
        assert_eq!(grid.rows[1].cells[0], "绿区");
        assert_eq!(grid.rows[1].cells[1], "40-59");
        assert_eq!(grid.rows[2].cells[0], "", "rowspan 的下方行留空");
    }

    /// 右对齐数字列按源几何复制对齐，不依据内容类型。
    #[test]
    fn alignment_is_copied_from_source_geometry() {
        let profile = Profile::default();
        let rows = vec![
            row(&[
                ("甲", 170.0, 190.0),
                ("0-19", 279.0, 303.0),
                ("无", 390.0, 431.0),
            ]),
            row(&[
                ("乙", 170.0, 190.0),
                ("100-119", 262.0, 300.0),
                ("扣减", 390.0, 431.0),
            ]),
            row(&[
                ("丙", 170.0, 190.0),
                ("200", 246.0, 320.0),
                ("扣减", 390.0, 431.0),
            ]),
        ];
        let grid = build(&rows, columns(), 10.0, &profile);
        let lines = render(&grid);
        // 第二列右边缘稳定 -> 右对齐：三行的数值结束位置一致。
        let pos = |line: &str, needle: &str| display_width(&line[..line.find(needle).unwrap()]);
        assert_eq!(
            pos(&lines[1], "100-119") + display_width("100-119"),
            pos(&lines[2], "200") + display_width("200")
        );
    }

    /// 行尾填充被裁掉，不留下不可见空格。
    #[test]
    fn rendering_trims_trailing_padding() {
        let profile = Profile::default();
        let rows = vec![row(&[("甲", 170.0, 190.0)])];
        let grid = build(&rows, columns(), 10.0, &profile);
        let lines = render(&grid);
        assert_eq!(lines[0], "甲");
    }
}
