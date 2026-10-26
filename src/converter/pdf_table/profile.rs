//! 版式参数：形状全部无量纲，取值来源逐条注明。
//!
//! # 为什么单独成模块
//!
//! 表格重建最容易滑向「为一份样本试出一组 point 值」——那样的参数跨文档不可移植，
//! 也无法判断一次修改是改善还是过拟合。本模块把**所有**可调量集中在此，并强制
//! 满足两条约束：
//!
//! 1. 一律无量纲（em、比例、行数），不出现 PDF point；
//! 2. 每一项都写明来源类别：`字体度量` / `排版常识` / `结构定义` / `产品决策`。
//!
//! 数值型参数只允许来自前三类；出现需要「看样本试出来」的量时，宁可把判定改成
//! 结构性的（如「列边界不允许被任何词跨越」），也不新增 point 常量。

/// 一页版式判定的全部可调量。
///
/// 默认值即生产值；字段独立可变，便于在评估脚本中做对比实验。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Profile {
    /// 行盒高度上限，单位 em。
    ///
    /// 来源：字体度量。西文 ascent+descent 约 1.2 em，汉字约 1.15 em，取 1.3 为
    /// 上限。词包围盒高度一旦超过该值，说明这份数据的高度字段不可信（实测
    /// `pdf_oxide 0.3.73` 在 `/Rotate` 页把词宽写进了高度，见 `word::repair`），
    /// 此时不再使用高度，改由字号推导。
    pub line_box_em: f32,

    /// 两个词判为「同一行」所需的垂直投影重叠比例。
    ///
    /// 来源：结构定义。两词的垂直投影有重叠是同一行的必要条件；取 0.2 是
    /// 「有重叠」与「无重叠」的分界，留出的余量用于吸收列间基线偏移与上标
    /// （实测同排词带可错开 7.4pt，重叠仍达 30%）。判定只看相对关系，与字号无关。
    pub row_overlap: f32,

    /// 列间空白下限，单位 em。
    ///
    /// 来源：排版常识。西文词间空格约 0.25 em，整字宽 1 em；两个**列**之间的空白
    /// 必然大于一个词间空格，取 1 em 为下限。同一个值还用于把行分成「表格行」与
    /// 「正文行」——行内最大间隙不到 1 em 的行，不可能是多列结构。
    pub column_separation_em: f32,

    /// x 区间合并的容差，单位 em。
    ///
    /// 来源：字体度量。字距微调（Tc/kern）与亚像素取整造成的同列词边缘抖动远小于
    /// 一个字符，取 0.15 em 合并；超过则视为真正的列间空白。
    pub interval_tolerance_em: f32,

    /// 一列成立所需的最少行数。
    ///
    /// 来源：结构定义。一个「列」由多个格共享同一 x 区间构成，单行无法构成列。
    /// 取 2 即「至少两行在同一 x 区间有内容」，不含经验成分。
    pub min_column_rows: usize,

    /// 一个表格区域成立所需的最少行数。
    ///
    /// 来源：结构定义。与 [`Self::min_column_rows`] 同源：两行即可验证列的复现，
    /// 因此两行的表也应当还原（旧实现固定 3 行会漏掉两行小表）。
    pub min_block_rows: usize,

    /// 词间补空格的阈值，单位 em。
    ///
    /// 来源：排版常识。西文空格字宽约 0.25 em，两端对齐会拉伸到数 em，但不小于
    /// 一个空格；低于该值只能是字距微调或紧排。中文之间不补空格（见
    /// [`super::word::needs_space`]），因为中文没有词间分隔符，补空格等于凭空
    /// 制造词边界。
    pub space_em: f32,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            line_box_em: 1.3,
            row_overlap: 0.2,
            column_separation_em: 1.0,
            interval_tolerance_em: 0.15,
            min_column_rows: 2,
            min_block_rows: 2,
            space_em: 0.25,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认参数必须全部无量纲且为正，防止有人把 point 值塞进来。
    #[test]
    fn default_profile_is_dimensionless() {
        let p = Profile::default();
        assert!(p.line_box_em > 1.0 && p.line_box_em < 2.0);
        assert!(p.row_overlap > 0.0 && p.row_overlap < 1.0);
        assert!(p.column_separation_em >= 1.0);
        assert!(p.interval_tolerance_em < p.column_separation_em);
        assert!(p.min_column_rows >= 2);
        assert!(p.min_block_rows >= 2);
        assert!(p.space_em > 0.0 && p.space_em < p.column_separation_em);
    }
}
