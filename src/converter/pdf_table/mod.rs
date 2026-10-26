//! PDF 表格重建：从词坐标还原无边框表格的行列结构。
//!
//! # 目标与边界
//!
//! 只做一件事：**把文档自己的版式如实映射成纯文本**。因此
//!
//! - 只消费词包围盒与字号这些最底层的事实，不依赖任何黑盒表格算法；
//! - 不猜内容归属：跨行合并单元格留空、跨列标题留在起始列、公式不还原；
//! - 判定失败即回退基线文本（[`rebuild_page`] 返回 `None`），绝不输出半成品。
//!
//! # 判定链（每步都只依赖无量纲事实）
//!
//! ```text
//! 词 ──归一化──▶ 行 ──列间空白──▶ 区块 ──无词跨越的空白带──▶ 列
//!                                              │
//!                                              ├─▶ 网格 ──渲染──▶ 对齐纯文本
//!                                              └─▶ 自检（字符守恒 + 行宽有界）
//! ```
//!
//! 1. **归一化**（[`word`]）：用字体度量判定包围盒高度是否可信。《pdf_oxide 0.3.73
//!    在 `/Rotate` 页把词宽写进高度》这一类数据缺陷在入口处被修掉，旋转页因此不需要
//!    「猜方向」。
//! 2. **行**（[`lines`]）：垂直投影重叠即同一行，重叠比例是无量纲的。
//! 3. **区块**（[`lines::segments`]）：行内存在 ≥1 em 的列间空白才算表格行；连续的
//!    表格行构成区块，正文行是天然分隔符。
//! 4. **列**（[`columns`]）：列界是一条**没有任何词跨越**的垂直空白带（≥1 em）。
//!    这一定义同时覆盖左对齐文本列、右对齐数字列与密集数字矩阵。
//! 5. **对齐**（[`grid`]）：复制源几何——右边缘比左边缘稳定就右对齐，不看内容类型。
//! 6. **自检**（[`verify`]）：字符守恒与行宽有界。任一不过即整页回退。
//!
//! 全部可调量集中在 [`profile::Profile`]，一律无量纲并注明来源；`point` 常量在
//! 本模块不存在。
//!
//! # 已知边界
//!
//! - **多层表头**：两级表头列数不同时，父级列会因「有词跨越」而不成界，子列按
//!   第二级切分；内容不丢，但对齐只在子列层面成立。
//! - **正文双栏**：双栏正文的栏间空白同样满足列定义，会被渲染成两列网格——那仍是
//!   对原版式的忠实还原，但线性阅读不如单栏文本；若日后有样本证明这是问题，应在
//!   此处补一条结构性判据，而不是调阈值。
//! - **跨页表格**：不拼接。跨页属推断，违反「只还原，不推断」；两页各自对齐。
//! - **无列间空白的表**（单元格紧贴）：不构成列，整块按正文输出。
//! - **示意图**：图内的标签框若恰好构成两列网格，会被当作表格重排——位置忠实、
//!   内容不丢，但线性阅读不如原阅读顺序。这类块与「两行两列的小表」在几何上
//!   无法区分，故不做特判；真实文档里的两行小表（如风险权重表）因此能还原。

pub(crate) mod columns;
pub(crate) mod grid;
pub(crate) mod lines;
pub(crate) mod profile;
pub(crate) mod verify;

#[cfg(test)]
mod pages;
pub(crate) mod word;

pub(crate) use profile::Profile;
pub(crate) use word::RawWord;

/// 重建一页文本；未检出表格或自检失败时返回 `None`（调用方沿用基线文本）。
pub(crate) fn rebuild_page(input: Vec<RawWord>) -> Option<String> {
    rebuild_with(input, &Profile::default())
}

/// 带显式参数的版本：供评估脚本做参数对比实验。
pub(crate) fn rebuild_with(input: Vec<RawWord>, profile: &Profile) -> Option<String> {
    let words = word::normalize(input, profile);
    if words.is_empty() {
        return None;
    }
    let rows = lines::group_rows(words, profile);
    if rows.is_empty() {
        return None;
    }
    let em = lines::page_em(&rows);
    if em <= 0.0 {
        return None;
    }
    // 目录页整页按正文输出：引导线行占到三分之一以上，说明这页是目录，
    // 没有需要还原的表格；而按行重排反而会把左右两栏的条目并成一行。
    let leader_rows = rows.iter().filter(|row| lines::is_leader_row(row)).count();
    if leader_rows * 3 >= rows.len() {
        return None;
    }

    let mut output: Vec<String> = Vec::new();
    let mut tables = 0;
    let mut columns = 0;
    for range in lines::segments(&rows, em, profile) {
        let segment = &rows[range];
        let reconstruct = if segment.len() >= profile.min_block_rows {
            columns::detect(segment, em, profile)
                .map(|columns| grid::build(segment, columns, em, profile))
        } else {
            None
        };
        match reconstruct {
            Some(grid) => {
                tables += 1;
                columns = columns.max(grid.columns.len());
                if output.last().is_some_and(|line| !line.is_empty()) {
                    output.push(String::new());
                }
                output.extend(grid::render(&grid));
                output.push(String::new());
            }
            None => output.extend(
                segment
                    .iter()
                    .map(|row| grid::render_prose(row, em, profile)),
            ),
        }
    }
    if tables == 0 {
        return None;
    }

    while output.first().is_some_and(|line| line.is_empty()) {
        output.remove(0);
    }
    while output.last().is_some_and(|line| line.is_empty()) {
        output.pop();
    }
    let text = output.join("\n");
    match verify::check(&rows, &text, columns) {
        Ok(()) => Some(text),
        Err(reason) => {
            log::warn!("表格重建自检未通过，回退基线文本: {reason}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x0: f32, x1: f32, y_bottom: f32) -> RawWord {
        RawWord {
            text: text.to_string(),
            x0,
            y0: y_bottom,
            x1,
            y1: y_bottom + 10.0,
            font_size: 10.0,
            sequence: 0,
        }
    }

    /// 两行的表现在可以还原：区块门槛是 `min_block_rows`（结构定义），
    /// 不是「至少三行」这类经验值。参数化入口同时验证了调参路径可测。
    #[test]
    fn two_row_table_is_rebuilt_unless_profile_demands_more() {
        let input = vec![
            word("信用评级", 76.0, 119.0, 608.0),
            word("AAA 至 AA-", 133.0, 188.0, 608.0),
            word("未评级", 481.0, 513.0, 608.0),
            word("风险权重", 76.0, 119.0, 577.0),
            word("0%", 153.0, 168.0, 577.0),
            word("100%", 484.0, 511.0, 577.0),
        ];
        let rebuilt = rebuild_page(input.clone()).expect("两行表应被还原");
        assert!(rebuilt.contains("信用评级"));
        assert!(rebuilt.contains("AAA 至 AA-"));

        let strict = Profile {
            min_block_rows: 3,
            ..Profile::default()
        };
        assert!(
            rebuild_with(input, &strict).is_none(),
            "提高门槛后应回退基线"
        );
    }

    /// 正文页（无列间空白）返回 None，调用方沿用基线，实现「非表格页零改动」。
    #[test]
    fn prose_page_returns_none() {
        let input = vec![
            word("前一交易日的", 70.0, 200.0, 257.0),
            word("VaR", 206.0, 228.0, 257.0),
            word("值。", 231.0, 258.0, 257.0),
            word("后一交易日的", 70.0, 200.0, 275.0),
        ];
        assert!(rebuild_page(input).is_none());
    }

    /// 空输入返回 None。
    #[test]
    fn empty_page_returns_none() {
        assert!(rebuild_page(Vec::new()).is_none());
    }
}
